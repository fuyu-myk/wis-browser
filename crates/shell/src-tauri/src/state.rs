//! Application state container managed by Tauri.
//!
//! Holds:
//! - the in-memory [`BrowserState`] behind an async mutex;
//! - the persistent [`Storage`] backend;
//! - the long-lived DoH [`Resolver`] used by the privacy decision engine;
//! - hot-swappable [`TrackerBlocker`] + content-filter rule specs behind a
//!   sync `RwLock`, so a background EasyPrivacy refresh can replace them
//!   without restarting the app;
//! - the filter-list cache directory;
//! - one [`ProxyServer`] per workspace, started lazily on first use. Each
//!   tab webview is pointed at its workspace's proxy via
//!   `WebviewBuilder::proxy_url`.
//!
//! Commands acquire the async mutex, compute the new state, persist it, then
//! replace the in-memory copy: see `mutate` in `commands`.

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use tokio::sync::Mutex;
use wis_core::{BrowserState, PrivacyMode, WorkspaceId};
use wis_network::{
    DohResolver, ProxyServer, RequestPolicy, Resolver, StubResolver, TrackerBlocker,
};
use wis_storage::Storage;

use crate::content_filter::{build_rule_specs_from, RuleSpec};
use crate::error_compat::WisShellError;

/// A closed tab worth offering to reopen.
pub struct ClosedTab {
    pub workspace_id: WorkspaceId,
    pub url: String,
    /// The folder the tab lived in at close (`None` = loose).
    pub folder_id: Option<wis_core::TabFolderId>,
    /// Position in the workspace's `tab_order` at close (clamped if the
    /// workspace shrank; `usize::MAX` when unknown = append).
    pub index: usize,
    /// Pinned at close; reopen re-pins (pin-strip slot not preserved).
    pub pinned: bool,
    /// Serialized WKWebView `interactionState` captured at close, so reopen
    /// restores history/scroll/forms instead of cold-loading. `None` when the
    /// webview was already discarded (slept) at close time.
    pub interaction_state: Option<String>,
}

/// A window's on-screen geometry in **logical** pixels (the unit the window
/// builder and `set_position`/`set_size` use), persisted per workspace so a
/// (re)opened window restores where and how big it last was.
#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct WindowGeometry {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Meta key under which the per-workspace geometry map is persisted (JSON, keyed
/// by workspace uuid string so it is a valid JSON object).
pub const WINDOW_GEOMETRY_META: &str = "window_geometry";

/// Parse the persisted geometry map (uuid-string keys -> geometry). Unparsable
/// entries are dropped; a bad blob yields an empty map.
pub fn parse_window_geometry(json: &str) -> HashMap<WorkspaceId, WindowGeometry> {
    let raw: HashMap<String, WindowGeometry> = serde_json::from_str(json).unwrap_or_default();
    raw.into_iter()
        .filter_map(|(k, v)| {
            uuid::Uuid::parse_str(&k)
                .ok()
                .map(|u| (WorkspaceId::from(u), v))
        })
        .collect()
}

pub struct AppState {
    pub browser: Mutex<BrowserState>,
    /// Which workspace each OS window currently shows (window label ->
    /// workspace). The active workspace is **per-window**, not global: a window
    /// is a view over one workspace. `BrowserState.active_workspace_id` stays
    /// the persisted primary (what the `main` window restores to). Seeded with
    /// `main` at startup; [`get_browser_state`](super::commands::get_browser_state)
    /// returns the calling window's entry and [`set_active_workspace`](super::commands::set_active_workspace)
    /// keeps it in sync.
    pub window_workspaces: std::sync::Mutex<HashMap<String, WorkspaceId>>,
    pub storage: Arc<dyn Storage>,
    /// Debounced, ordered writer for [`BrowserState`] snapshots (see
    /// [`crate::save_coalescer`]). Destructive transitions bypass the
    /// debounce via [`write_now`](crate::save_coalescer::SaveCoalescer::write_now).
    pub save_coalescer: Arc<crate::save_coalescer::SaveCoalescer>,
    /// Per-tab debounced writer for serialized WKWebView `interactionState`, so
    /// a tab's back/forward history + scroll survive a full restart (see
    /// [`crate::tab_session`]).
    pub tab_session_saver: crate::tab_session::TabSessionSaver,
    pub tracker_blocker: RwLock<Arc<TrackerBlocker>>,
    pub resolver: Arc<dyn Resolver>,
    pub content_rules: RwLock<HashMap<PrivacyMode, Option<RuleSpec>>>,
    /// Sync read-cache of [`UserSettings.user_cosmetic_rules`](wis_core::UserSettings::user_cosmetic_rules)
    /// (host -> hidden selectors), so the synchronous cosmetic message handler can read them
    /// without locking the async browser mutex. Refreshed on every settings
    /// change and on each zapper add.
    pub user_cosmetic_rules: RwLock<HashMap<String, Vec<String>>>,
    /// Sync read-cache of `UserSettings.userscripts`, so the synchronous
    /// navigation hook can pick matching scripts without the async mutex.
    pub userscripts: RwLock<Vec<wis_core::userscript::Userscript>>,
    pub filter_cache_dir: PathBuf,
    pub proxies: Mutex<HashMap<WorkspaceId, ProxyServer>>,
    /// Password-vault master key, present only while the vault is unlocked.
    /// Cleared on lock/idle; zeroized on drop (see [`wis_secrets::MasterKey`]).
    pub vault_key: Mutex<Option<wis_secrets::MasterKey>>,
    /// A login captured from a page, awaiting the user's save/dismiss decision.
    /// The plaintext password lives here only; it never crosses back to a webview.
    pub pending_capture: Mutex<Option<crate::capture::PendingCapture>>,
    /// Most-recently-closed tabs (newest last), for "reopen closed tab". Capped
    /// in-memory and per-session; private/ephemeral tabs are never recorded.
    pub closed_tabs: Mutex<Vec<ClosedTab>>,
    /// Interaction-state blobs awaiting a reopened tab's webview creation:
    /// `restore_closed_tab` stashes the blob under the new tab id and
    /// `create_tab_webview` consumes it, restoring the closed tab's session.
    pub pending_restore_states: Mutex<HashMap<wis_core::TabId, String>>,
    /// Hosts the user chose to "proceed anyway" past the navigation safety
    /// interstitial this session, so they aren't re-prompted for that host.
    pub nav_guard_bypass: Mutex<HashSet<String>>,
    /// Workspaces whose saved session cookies have already been restored into
    /// their WebKit data store this run, these are only restored once per workspace.
    pub restored_cookies: Mutex<HashSet<WorkspaceId>>,
    /// Hover-preview snapshots (downscaled PNGs), newest last; capped (see
    /// [`crate::tab_preview`]). Captured on switch-away/sleep, served on hover.
    pub tab_previews: std::sync::Mutex<Vec<(wis_core::TabId, Vec<u8>)>>,
    /// Tracked downloads, oldest first (see [`crate::downloads`]). A sync
    /// mutex because wry's download callbacks are synchronous main-thread
    /// code that cannot await.
    pub downloads: std::sync::Mutex<Vec<crate::downloads::DownloadItem>>,
    /// Sync mirror of the download settings (ask-where + remembered dir),
    /// readable from the same synchronous download callbacks.
    pub download_prefs: std::sync::Mutex<crate::downloads::DownloadPrefs>,
    /// Video-popout bookkeeping (pending enter handshake + the live popout).
    /// A sync mutex because the `wisPopout` script-message handler is
    /// synchronous main-thread code that cannot await.
    pub video_popout: std::sync::Mutex<crate::video_popout::VideoPopoutState>,
    /// Tab uuids whose webview reported audible playback on the last audio
    /// poll (see [`crate::audio`]).
    pub audible_tabs: std::sync::Mutex<HashSet<String>>,
    /// Tab uuids the user muted (session-scoped; re-applied when a slept
    /// tab's webview is recreated).
    pub muted_tabs: std::sync::Mutex<HashSet<String>>,
    /// Widget ids whose panel the user muted (session-scoped; re-applied
    /// when a reclaimed panel is reopened).
    pub muted_widgets: std::sync::Mutex<HashSet<String>>,
    /// Persisted per-origin camera/microphone consent, keyed `"<kind>|<origin>"`.
    /// A sync mirror of `UserSettings.media_permissions` (seeded at startup,
    /// refreshed on change) read by the `wisMediaPermission` handler (main
    /// thread) without awaiting the state lock.
    pub media_permissions: std::sync::RwLock<HashMap<String, bool>>,
    /// Session-only ("just once") camera/microphone decisions, checked ahead of
    /// the persisted set and dropped on restart.
    pub media_permissions_session: std::sync::RwLock<HashMap<String, bool>>,
    /// Camera/microphone requests awaiting the user's decision in the prompt
    /// overlay, keyed by a monotonic request id (`media_seq`).
    pub pending_media: std::sync::Mutex<HashMap<u64, crate::permissions::PendingMedia>>,
    pub media_seq: std::sync::atomic::AtomicU64,
    /// Set once the main window has reopened the saved multi-window layout, so
    /// `restore_session_windows` spawns the secondary windows exactly once per run.
    pub session_restored: std::sync::atomic::AtomicBool,
    /// Current window geometry per workspace (logical px), updated on
    /// move/resize and persisted (debounced) so a (re)opened window restores its
    /// place/size. Loaded from storage at startup.
    pub window_geometry: std::sync::Mutex<HashMap<WorkspaceId, WindowGeometry>>,
    /// Generation counter debouncing geometry persistence: only the last
    /// move/resize of a burst writes to storage.
    pub geometry_gen: std::sync::atomic::AtomicU64,
    /// Colour of the swipe-nav indicator: matching the webview border,
    /// baked into new tab webviews and pushed to live ones on theme change.
    /// A plain string the chrome owns.
    pub swipe_accent: std::sync::Mutex<String>,
    /// PNG bytes of the most recent screenshot (already copied to the
    /// clipboard), held so the success toast's "Save" button can write it to
    /// disk without re-capturing. Replaced on each capture.
    pub last_screenshot: std::sync::Mutex<Option<Vec<u8>>>,
    /// The built-in Tor (Arti) SOCKS5 proxy, started lazily on first use.
    /// Only present with the `tor` feature.
    #[cfg(feature = "tor")]
    pub tor_proxy: Mutex<Option<wis_network::TorProxy>>,
}

impl AppState {
    pub fn new(
        browser: BrowserState,
        storage: Arc<dyn Storage>,
        tracker_blocker: Arc<TrackerBlocker>,
        resolver: Arc<dyn Resolver>,
        filter_cache_dir: PathBuf,
        initial_filter_text: &str,
    ) -> Self {
        let download_prefs = crate::downloads::DownloadPrefs::from_settings(&browser.settings);
        let initial_user_cosmetic_rules = browser.settings.user_cosmetic_rules.clone();
        let initial_userscripts = browser.settings.userscripts.clone();
        let initial_media_permissions = browser.settings.media_permissions.clone();
        // The main window starts on the persisted active workspace.
        let initial_active_workspace = browser.active_workspace_id;
        Self {
            browser: Mutex::new(browser),
            window_workspaces: std::sync::Mutex::new(HashMap::from([(
                "main".to_string(),
                initial_active_workspace,
            )])),
            storage,
            save_coalescer: Arc::new(crate::save_coalescer::SaveCoalescer::new()),
            tab_session_saver: crate::tab_session::TabSessionSaver::new(),
            tracker_blocker: RwLock::new(tracker_blocker),
            resolver,
            content_rules: RwLock::new(build_rule_specs_from(initial_filter_text)),
            user_cosmetic_rules: RwLock::new(initial_user_cosmetic_rules),
            userscripts: RwLock::new(initial_userscripts),
            filter_cache_dir,
            proxies: Mutex::new(HashMap::new()),
            vault_key: Mutex::new(None),
            pending_capture: Mutex::new(None),
            closed_tabs: Mutex::new(Vec::new()),
            pending_restore_states: Mutex::new(HashMap::new()),
            nav_guard_bypass: Mutex::new(HashSet::new()),
            restored_cookies: Mutex::new(HashSet::new()),
            tab_previews: std::sync::Mutex::new(Vec::new()),
            downloads: std::sync::Mutex::new(Vec::new()),
            download_prefs: std::sync::Mutex::new(download_prefs),
            video_popout: std::sync::Mutex::new(Default::default()),
            audible_tabs: std::sync::Mutex::new(HashSet::new()),
            muted_tabs: std::sync::Mutex::new(HashSet::new()),
            muted_widgets: std::sync::Mutex::new(HashSet::new()),
            media_permissions: std::sync::RwLock::new(initial_media_permissions),
            media_permissions_session: std::sync::RwLock::new(HashMap::new()),
            pending_media: std::sync::Mutex::new(HashMap::new()),
            media_seq: std::sync::atomic::AtomicU64::new(0),
            session_restored: std::sync::atomic::AtomicBool::new(false),
            window_geometry: std::sync::Mutex::new(HashMap::new()),
            geometry_gen: std::sync::atomic::AtomicU64::new(0),
            swipe_accent: std::sync::Mutex::new("#6b77f8".to_string()),
            last_screenshot: std::sync::Mutex::new(None),
            #[cfg(feature = "tor")]
            tor_proxy: Mutex::new(None),
        }
    }

    /// The workspace shown by the window labelled `label`, if bound. `None`
    /// means the window has no workspace yet (e.g. a fresh window awaiting the
    /// picker); callers fall back to the persisted primary.
    pub fn window_workspace(&self, label: &str) -> Option<WorkspaceId> {
        self.window_workspaces
            .lock()
            .expect("window_workspaces poisoned")
            .get(label)
            .copied()
    }

    /// Bind the window labelled `label` to `id` (the workspace it now shows).
    pub fn set_window_workspace(&self, label: &str, id: WorkspaceId) {
        let snapshot = {
            let mut map = self
                .window_workspaces
                .lock()
                .expect("window_workspaces poisoned");
            map.insert(label.to_string(), id);
            map.values().copied().collect::<Vec<_>>()
        };
        self.persist_windowed_workspaces(&snapshot);
    }

    /// Forget a window's workspace binding (window closed, or a failed open).
    pub fn clear_window_workspace(&self, label: &str) {
        let snapshot = {
            let mut map = self
                .window_workspaces
                .lock()
                .expect("window_workspaces poisoned");
            map.remove(label);
            map.values().copied().collect::<Vec<_>>()
        };
        self.persist_windowed_workspaces(&snapshot);
    }

    /// Persist the current windowed-workspace set so the multi-window layout
    /// survives any exit. Called on every window bind/unbind, so the stored set
    /// is always current. Best-effort; failures only log.
    fn persist_windowed_workspaces(&self, ids: &[WorkspaceId]) {
        if let Err(err) = self.storage.save_windowed_workspaces(ids) {
            log::warn!("persist windowed workspaces: {err}");
        }
    }

    /// Record a window's geometry for `workspace_id` in the in-memory cache.
    /// Does not write to disk; pair with the debounced `persist_window_geometry`.
    pub fn set_window_geometry(&self, workspace_id: WorkspaceId, geo: WindowGeometry) {
        self.window_geometry
            .lock()
            .expect("window_geometry poisoned")
            .insert(workspace_id, geo);
    }

    /// Saved geometry for `workspace_id`, applied when its window (re)opens.
    pub fn window_geometry_for(&self, workspace_id: WorkspaceId) -> Option<WindowGeometry> {
        self.window_geometry
            .lock()
            .expect("window_geometry poisoned")
            .get(&workspace_id)
            .copied()
    }

    /// Replace the in-memory geometry cache from the persisted blob at startup,
    /// before any window is restored.
    pub fn load_window_geometry(&self, map: HashMap<WorkspaceId, WindowGeometry>) {
        *self
            .window_geometry
            .lock()
            .expect("window_geometry poisoned") = map;
    }

    /// Serialize the geometry cache to storage under [`WINDOW_GEOMETRY_META`].
    /// Best-effort; failures only log.
    pub fn persist_window_geometry(&self) {
        let snapshot: HashMap<String, WindowGeometry> = {
            let map = self
                .window_geometry
                .lock()
                .expect("window_geometry poisoned");
            map.iter()
                .map(|(k, v)| (k.as_uuid().to_string(), *v))
                .collect()
        };
        match serde_json::to_string(&snapshot) {
            Ok(json) => {
                if let Err(err) = self.storage.set_meta(WINDOW_GEOMETRY_META, &json) {
                    log::warn!("persist window geometry: {err}");
                }
            }
            Err(err) => log::warn!("serialize window geometry: {err}"),
        }
    }

    /// Workspace ids currently shown by some window, so the New Window picker can
    /// offer only the **free** ones (one workspace per window).
    pub fn occupied_workspaces(&self) -> std::collections::HashSet<WorkspaceId> {
        self.window_workspaces
            .lock()
            .expect("window_workspaces poisoned")
            .values()
            .copied()
            .collect()
    }

    /// Label of the window currently showing `id`, if any. The one-workspace-
    /// per-window invariant means at most one matches; used to focus the owner
    /// instead of duplicating a workspace into a second window.
    pub fn window_for_workspace(&self, id: WorkspaceId) -> Option<String> {
        self.window_workspaces
            .lock()
            .expect("window_workspaces poisoned")
            .iter()
            .find(|(_, &ws)| ws == id)
            .map(|(label, _)| label.clone())
    }

    /// Replace the persisted camera/microphone consent read-cache (after a
    /// settings change: a remembered prompt decision or a settings-page edit).
    pub fn refresh_media_permissions_cache(&self, perms: HashMap<String, bool>) {
        let mut guard = self
            .media_permissions
            .write()
            .expect("media_permissions poisoned");
        *guard = perms;
    }

    /// Replace the cosmetic-rule read-cache (called after a settings change
    /// or a zapper add).
    pub fn refresh_user_cosmetic_cache(&self, rules: HashMap<String, Vec<String>>) {
        let mut guard = self
            .user_cosmetic_rules
            .write()
            .expect("user_cosmetic_rules poisoned");
        *guard = rules;
    }

    /// Hidden selectors a user has zapped on `host` (exact match).
    pub fn user_cosmetic_selectors(&self, host: &str) -> Vec<String> {
        self.user_cosmetic_rules
            .read()
            .expect("user_cosmetic_rules poisoned")
            .get(host)
            .cloned()
            .unwrap_or_default()
    }

    /// Replace the user-script read-cache (after a settings change).
    pub fn refresh_userscripts_cache(&self, scripts: Vec<wis_core::userscript::Userscript>) {
        let mut guard = self.userscripts.write().expect("userscripts poisoned");
        *guard = scripts;
    }

    /// Enabled user scripts matching `url`, split by run-at into
    /// `(document_start_sources, document_end_sources)`.
    pub fn matching_userscripts(&self, url: &str) -> (Vec<String>, Vec<String>) {
        use wis_core::userscript::RunAt;
        let guard = self.userscripts.read().expect("userscripts poisoned");
        let mut start = Vec::new();
        let mut end = Vec::new();

        for script in guard.iter() {
            if !script.matches_url(url) {
                continue;
            }

            match script.run_at {
                RunAt::DocumentStart => start.push(script.source.clone()),
                RunAt::DocumentEnd | RunAt::DocumentIdle => end.push(script.source.clone()),
            }
        }

        (start, end)
    }

    /// The content-filter rule spec for a privacy mode, or `None` when the
    /// mode applies no engine-level filtering. Cloned out so the caller
    /// doesn't hold the lock.
    pub fn content_rule_spec(&self, mode: PrivacyMode) -> Option<RuleSpec> {
        self.content_rules
            .read()
            .expect("content_rules poisoned")
            .get(&mode)
            .and_then(|spec| spec.clone())
    }

    fn current_tracker_blocker(&self) -> Arc<TrackerBlocker> {
        self.tracker_blocker
            .read()
            .expect("tracker_blocker poisoned")
            .clone()
    }

    /// Build a [`RequestPolicy`] for the currently-active workspace.
    ///
    /// Per-site overrides will be threaded through here once they're added to
    /// `BrowserState`.
    pub async fn request_policy_for_active_workspace(&self) -> RequestPolicy {
        let browser = self.browser.lock().await;
        let mode = browser.active_workspace().privacy_mode;
        RequestPolicy::new(mode, self.current_tracker_blocker())
    }

    /// Lazily start (or look up) the workspace's loopback proxy and return
    /// its bound port. Idempotent: subsequent calls for the same workspace
    /// return the same port without re-binding.
    ///
    /// The proxy handles DoH resolution, host-level tracker blocking, and
    /// HTTPS-First upgrades. HTTPS is tunnelled raw; path-level filtering is
    /// enforced in the WebView engine (see `content_filter`).
    pub async fn ensure_proxy_for(&self, workspace_id: WorkspaceId) -> Result<u16, WisShellError> {
        let (privacy_mode, upstream) = {
            let browser = self.browser.lock().await;
            let ws = browser
                .workspace(workspace_id)
                .map_err(WisShellError::from)?;
            (ws.privacy_mode, ws.upstream_proxy.clone())
        };

        let upstream = self.resolve_upstream(upstream).await?;

        let mut proxies = self.proxies.lock().await;
        if let Some(server) = proxies.get(&workspace_id) {
            return Ok(server.port());
        }

        let policy = Arc::new(self.build_request_policy(privacy_mode, upstream));
        let bind: SocketAddr = ([127, 0, 0, 1], 0).into();
        let server = ProxyServer::start(policy, self.resolver.clone(), bind)
            .await
            .map_err(|err| WisShellError::Other(format!("proxy start: {err}")))?;
        let port = server.port();

        log::info!("workspace {workspace_id} proxy listening on 127.0.0.1:{port}");
        proxies.insert(workspace_id, server);

        Ok(port)
    }

    /// Build the request policy for a workspace from its privacy mode + the
    /// current tracker blocker, routed through `upstream` when set.
    fn build_request_policy(
        &self,
        privacy_mode: PrivacyMode,
        upstream: Option<wis_core::UpstreamProxy>,
    ) -> RequestPolicy {
        let policy = RequestPolicy::new(privacy_mode, self.current_tracker_blocker());
        match upstream {
            Some(up) => policy.with_upstream(up),
            None => policy,
        }
    }

    /// Hot-swap a running workspace proxy's policy from current state (privacy
    /// mode + upstream proxy), without rebinding the port. No-op if the proxy
    /// hasn't been started yet.
    pub async fn update_proxy_policy(&self, workspace_id: WorkspaceId) {
        let (privacy_mode, upstream) = {
            let browser = self.browser.lock().await;
            match browser.workspace(workspace_id) {
                Ok(ws) => (ws.privacy_mode, ws.upstream_proxy.clone()),
                Err(_) => return,
            }
        };
        let upstream = match self.resolve_upstream(upstream).await {
            Ok(up) => up,
            Err(err) => {
                log::warn!("workspace {workspace_id} upstream resolve failed: {err}");
                return;
            }
        };
        let proxies = self.proxies.lock().await;
        if let Some(server) = proxies.get(&workspace_id) {
            let policy = Arc::new(self.build_request_policy(privacy_mode, upstream));
            server.update_policy(policy);
            log::info!("workspace {workspace_id} proxy policy updated in place");
        }
    }

    /// Resolve a workspace upstream for the transport layer: built-in Tor maps
    /// to the embedded Arti SOCKS5 port; everything else passes through unchanged.
    pub async fn resolve_upstream(
        &self,
        upstream: Option<wis_core::UpstreamProxy>,
    ) -> Result<Option<wis_core::UpstreamProxy>, WisShellError> {
        match upstream {
            Some(wis_core::UpstreamProxy::Tor) => {
                let port = self.ensure_tor().await?;
                Ok(Some(wis_core::UpstreamProxy::Socks5 {
                    host: "127.0.0.1".to_string(),
                    port,
                }))
            }
            other => Ok(other),
        }
    }

    /// Start (once) the built-in Tor SOCKS5 proxy and return its loopback port.
    /// Bootstrapping can take several seconds on first call.
    #[cfg(feature = "tor")]
    pub async fn ensure_tor(&self) -> Result<u16, WisShellError> {
        let mut guard = self.tor_proxy.lock().await;
        if let Some(proxy) = guard.as_ref() {
            return Ok(proxy.port());
        }
        log::info!("bootstrapping built-in Tor (this can take a few seconds)…");
        let tor_dir = self
            .filter_cache_dir
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| self.filter_cache_dir.clone())
            .join("tor");
        let proxy = wis_network::TorProxy::start(tor_dir)
            .await
            .map_err(|e| WisShellError::Other(format!("tor: {e}")))?;
        let port = proxy.port();
        *guard = Some(proxy);

        log::info!("built-in Tor ready on 127.0.0.1:{port}");
        Ok(port)
    }

    /// Built-in Tor isn't compiled into this build.
    #[cfg(not(feature = "tor"))]
    pub async fn ensure_tor(&self) -> Result<u16, WisShellError> {
        Err(WisShellError::Other(
            "built-in Tor is not enabled in this build".to_string(),
        ))
    }

    /// Drop all running proxies. Next call to [`ensure_proxy_for`] for a
    /// workspace rebuilds its proxy from the current settings/mode.
    pub async fn clear_proxies(&self) {
        let mut proxies = self.proxies.lock().await;
        let count = proxies.len();
        proxies.clear();
        if count > 0 {
            log::info!("cleared {count} proxy(ies); next request rebuilds them");
        }
    }

    /// Swap in a new filter list. Rebuilds the [`TrackerBlocker`] (host-level
    /// blocking used by the proxy) and the per-mode WebKit content-rule
    /// specs (engine-level blocking), then drops every running proxy so the
    /// next request picks up the new policy. Live tab webviews still hold
    /// the previous rule list attached by [`crate::content_filter`]; the
    /// caller is responsible for re-attaching the new specs to active tabs.
    /// Returns the number of bytes in the new filter text for diagnostics.
    pub async fn rebuild_filter_engine(&self, filter_text: String) -> Result<usize, WisShellError> {
        // EasyList + user subscriptions feed the in-process engine only
        // (cosmetic filtering, scriptlets, proxy-level ad blocking); the
        // WKContentRuleList compiler keeps the smaller EasyPrivacy text
        // since the bigger lists would exceed its rule cap.
        let easylist = wis_network::filter_lists::load_cached_easylist(&self.filter_cache_dir);
        let sub_urls = {
            let browser = self.browser.lock().await;
            browser.settings.filter_subscriptions.clone()
        };
        let subs =
            wis_network::filter_lists::load_cached_subscriptions(&self.filter_cache_dir, &sub_urls);
        let mut texts: Vec<&str> = vec![&filter_text];
        if let Some(t) = easylist.as_deref() {
            texts.push(t);
        }
        for t in &subs {
            texts.push(t);
        }
        let mut blocker =
            TrackerBlocker::from_filter_texts_with_cache(&texts, &self.filter_cache_dir)
                .map_err(|err| WisShellError::Other(format!("rebuild blocker: {err}")))?;
        if let Some(json) = wis_network::filter_lists::load_cached_resources(&self.filter_cache_dir)
        {
            if let Err(err) = blocker.load_resources(&json) {
                log::warn!("filter_lists: scriptlet resources load failed: {err}");
            }
        }
        let rules = build_rule_specs_from(&filter_text);

        let size = filter_text.len();
        {
            let mut guard = self
                .tracker_blocker
                .write()
                .expect("tracker_blocker poisoned");
            *guard = Arc::new(blocker);
        }
        {
            let mut guard = self.content_rules.write().expect("content_rules poisoned");
            *guard = rules;
        }
        self.clear_proxies().await;

        log::info!("filter engine rebuilt from {size} bytes of filter text");
        Ok(size)
    }
}

/// Build a resolver from the current settings' DoH endpoint. Falls back to a
/// stub resolver (returning errors) if no DoH URL is configured.
pub fn build_resolver(doh_url: Option<&str>) -> Arc<dyn Resolver> {
    match doh_url {
        Some(url) => match DohResolver::from_settings_url(url) {
            Ok(resolver) => Arc::new(resolver),
            Err(err) => {
                log::warn!("failed to build DoH resolver from {url}: {err}; using stub");
                Arc::new(StubResolver::new())
            }
        },
        None => {
            log::info!("no DoH resolver configured; using stub");
            Arc::new(StubResolver::new())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wis_network::filter_lists::DEFAULT_TRACKER_LIST;
    use wis_storage::SqliteStorage;

    fn app_state_for_test() -> AppState {
        let storage: Arc<dyn Storage> =
            Arc::new(SqliteStorage::open_in_memory().expect("in-memory DB"));
        let blocker = Arc::new(TrackerBlocker::with_default_list().expect("default list"));
        let resolver: Arc<dyn Resolver> = Arc::new(StubResolver::new());
        AppState::new(
            BrowserState::new_with_defaults(),
            storage,
            blocker,
            resolver,
            std::env::temp_dir().join("wis-filter-test"),
            DEFAULT_TRACKER_LIST,
        )
    }

    #[tokio::test]
    async fn request_policy_reflects_active_workspaces_privacy_mode() {
        let state = app_state_for_test();
        let policy = state.request_policy_for_active_workspace().await;
        assert_eq!(policy.privacy_mode(), PrivacyMode::Balanced);
    }

    #[tokio::test]
    async fn active_workspace_is_tracked_per_window() {
        let state = app_state_for_test();
        let primary = state.browser.lock().await.active_workspace_id;

        // The main window is seeded with the persisted primary at startup.
        assert_eq!(state.window_workspace("main"), Some(primary));
        // A window with no binding yet has no entry (callers fall back to it).
        assert_eq!(state.window_workspace("win-2"), None);

        // Binding a window records its workspace without disturbing others.
        state.set_window_workspace("win-2", primary);
        assert_eq!(state.window_workspace("win-2"), Some(primary));
        assert_eq!(state.window_workspace("main"), Some(primary));
    }

    #[tokio::test]
    async fn window_for_workspace_resolves_the_owning_window() {
        let state = app_state_for_test();
        let primary = state.browser.lock().await.active_workspace_id;

        // The seeded main window owns the primary workspace.
        assert_eq!(state.window_for_workspace(primary).as_deref(), Some("main"));
        assert!(state.occupied_workspaces().contains(&primary));

        // Clearing the binding leaves the workspace free (no owner, not occupied).
        state.clear_window_workspace("main");
        assert_eq!(state.window_for_workspace(primary), None);
        assert!(!state.occupied_workspaces().contains(&primary));

        // Rebinding to another window moves ownership; the reverse lookup follows.
        state.set_window_workspace("win-7", primary);
        assert_eq!(
            state.window_for_workspace(primary).as_deref(),
            Some("win-7")
        );
    }

    #[tokio::test]
    async fn window_geometry_persists_and_parses_back() {
        let state = app_state_for_test();
        let ws = state.browser.lock().await.active_workspace_id;

        assert!(state.window_geometry_for(ws).is_none());
        let geo = WindowGeometry {
            x: 120.0,
            y: 240.0,
            width: 1100.0,
            height: 720.0,
        };
        state.set_window_geometry(ws, geo);
        state.persist_window_geometry();

        // Round-trips through storage + the uuid-keyed JSON encoding.
        let json = state
            .storage
            .get_meta(WINDOW_GEOMETRY_META)
            .expect("get")
            .expect("present");
        let parsed = parse_window_geometry(&json);
        let got = parsed.get(&ws).expect("workspace present");
        assert_eq!(
            (got.x, got.y, got.width, got.height),
            (120.0, 240.0, 1100.0, 720.0)
        );
    }

    #[tokio::test]
    async fn request_policy_picks_up_workspace_mode_changes() {
        let state = app_state_for_test();
        {
            let mut guard = state.browser.lock().await;
            let active_id = guard.active_workspace_id;
            let workspaces: Vec<_> = guard
                .workspaces
                .iter()
                .map(|w| {
                    if w.id == active_id {
                        w.with_privacy_mode(PrivacyMode::Strict)
                    } else {
                        w.clone()
                    }
                })
                .collect();
            *guard = BrowserState {
                workspaces,
                ..guard.clone()
            };
        }
        let policy = state.request_policy_for_active_workspace().await;
        assert_eq!(policy.privacy_mode(), PrivacyMode::Strict);
    }

    #[test]
    fn content_rule_specs_track_privacy_mode() {
        let state = app_state_for_test();
        assert!(
            state.content_rule_spec(PrivacyMode::Strict).is_some(),
            "strict gets an engine rule list"
        );
        assert!(
            state.content_rule_spec(PrivacyMode::Balanced).is_some(),
            "balanced gets an engine rule list"
        );
        assert!(
            state.content_rule_spec(PrivacyMode::Permissive).is_none(),
            "permissive applies no engine filter"
        );
        assert_ne!(
            state
                .content_rule_spec(PrivacyMode::Strict)
                .map(|s| s.identifier),
            state
                .content_rule_spec(PrivacyMode::Balanced)
                .map(|s| s.identifier),
            "each mode has a distinct cache identifier"
        );
    }

    #[tokio::test]
    async fn rebuild_filter_engine_swaps_rules_and_blocker() {
        let state = app_state_for_test();
        let before = state.content_rule_spec(PrivacyMode::Balanced).unwrap();
        let new_text = "||example.test^\n||another.test^\n";
        state
            .rebuild_filter_engine(new_text.to_string())
            .await
            .expect("rebuild succeeds");
        let after = state.content_rule_spec(PrivacyMode::Balanced).unwrap();
        assert_ne!(
            before.identifier, after.identifier,
            "swap must produce a fresh identifier"
        );
    }

    #[test]
    fn build_resolver_handles_unknown_doh_endpoints() {
        let _ = build_resolver(Some("https://my-doh.example/dns-query"));
        let _ = build_resolver(None);
    }

    #[tokio::test]
    async fn ensure_proxy_for_is_idempotent_per_workspace() {
        let state = app_state_for_test();
        let ws_id = state.browser.lock().await.active_workspace_id;
        let port_a = state.ensure_proxy_for(ws_id).await.expect("start a");
        let port_b = state.ensure_proxy_for(ws_id).await.expect("start b");
        assert_eq!(port_a, port_b, "second call must reuse the same proxy");
        assert!(port_a > 0);
    }

    #[tokio::test]
    async fn clear_proxies_forces_rebuild_on_next_ensure() {
        let state = app_state_for_test();
        let ws_id = state.browser.lock().await.active_workspace_id;
        let _ = state.ensure_proxy_for(ws_id).await.expect("start");
        state.clear_proxies().await;
        assert!(
            state.proxies.lock().await.is_empty(),
            "clear_proxies must drop all entries"
        );
        let _new_port = state.ensure_proxy_for(ws_id).await.expect("rebind");
    }
}
