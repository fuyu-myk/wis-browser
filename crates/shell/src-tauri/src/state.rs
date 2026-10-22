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
    DohResolver, ProxyServer, RequestPolicy, Resolver, SwappableResolver, TrackerBlocker,
};
use wis_storage::Storage;

use crate::content_filter::{RuleSpec, RuleSpecs};
use crate::error_compat::WisShellError;

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
    /// Swappable so a DoH provider change takes effect on the next lookup:
    /// running proxies hold this handle, not the resolver behind it.
    pub resolver: Arc<SwappableResolver>,
    pub content_rules: RwLock<RuleSpecs>,
    filter_rebuild: Mutex<()>,
    pub filter_refresh: Mutex<()>,
    /// See [`crate::uncloak`].
    pub cloaking: std::sync::Mutex<HashMap<WorkspaceId, Arc<wis_network::cloaking::Cloaking>>>,
    pub webview_workspaces: std::sync::RwLock<HashMap<String, WorkspaceId>>,
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
    /// Logins captured from pages, one per tab, awaiting the user's save or
    /// dismiss decision. The plaintext passwords live here only; they never
    /// cross back to a webview.
    pub pending_captures: Mutex<HashMap<wis_core::TabId, crate::capture::PendingCapture>>,
    /// Live sessions (WebKit `interactionState`) of tabs closed in this run,
    /// keyed by their `closed_tabs` row id. The rows live in storage and
    /// survive a restart.
    pub closed_tab_sessions: Mutex<HashMap<i64, String>>,
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
    /// Label of the focused Wis window, from window focus events.
    pub focused_window: std::sync::Mutex<Option<String>>,
    /// Site usage being metered and not yet stored (see [`crate::usage`]).
    pub usage: std::sync::Mutex<wis_core::usage::UsageLedger>,
    /// Serialises usage flushes with clears.
    pub usage_io: tokio::sync::Mutex<()>,
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
    /// Sync mirror of the autoplay setting and its per-host overrides, read
    /// per navigation on the main thread.
    pub autoplay: std::sync::RwLock<crate::autoplay::AutoplayCache>,
    /// Sync mirror of the saved per-site protection, read per navigation on
    /// the main thread.
    pub site_protection: std::sync::RwLock<HashMap<String, wis_core::SiteProtection>>,
    /// Per-site protection changed in private workspaces, never saved.
    pub site_protection_session: std::sync::RwLock<HashMap<String, wis_core::SiteProtection>>,
    /// Sync mirror of the page-zoom setting and its per-host overrides, read
    /// per navigation on the main thread.
    pub zoom: std::sync::RwLock<crate::zoom::ZoomCache>,
    /// Camera/microphone requests awaiting the user's decision in the prompt
    /// overlay, keyed by a monotonic request id (`media_seq`).
    pub pending_media: std::sync::Mutex<HashMap<u64, crate::permissions::PendingMedia>>,
    pub media_seq: std::sync::atomic::AtomicU64,
    /// Persisted per-site custom-scheme launch decisions, keyed
    /// `"<scheme>|<initiator host>"`. A sync mirror of
    /// `UserSettings.app_launch_permissions`.
    pub app_launch_permissions: std::sync::RwLock<HashMap<String, bool>>,
    /// Custom-scheme launches awaiting the user's decision in the prompt
    /// overlay, keyed by a monotonic request id.
    pub pending_app_launch: std::sync::Mutex<HashMap<u64, crate::app_launch::PendingAppLaunch>>,
    pub app_launch_seq: std::sync::atomic::AtomicU64,
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
    /// Cancel flags for in-flight assistant completions, keyed by the request
    /// id the panel minted. Set by `ai_cancel` and dropped by `ai_chat` when
    /// its stream ends. (see [`crate::ai`])
    pub ai_cancels: std::sync::Mutex<HashMap<String, Arc<std::sync::atomic::AtomicBool>>>,
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
        resolver: Arc<SwappableResolver>,
        filter_cache_dir: PathBuf,
        content_rules: RuleSpecs,
    ) -> Self {
        let download_prefs = crate::downloads::DownloadPrefs::from_settings(&browser.settings);
        let initial_user_cosmetic_rules = browser.settings.user_cosmetic_rules.clone();
        let initial_userscripts = browser.settings.userscripts.clone();
        let initial_media_permissions = browser.settings.media_permissions.clone();
        let initial_autoplay = crate::autoplay::AutoplayCache {
            policy: browser.settings.autoplay_policy,
            overrides: browser.settings.autoplay_site_overrides.clone(),
        };
        let initial_site_protection = browser.settings.site_protection_overrides.clone();
        let initial_zoom = crate::zoom::ZoomCache {
            default_percent: browser.settings.default_zoom_percent,
            overrides: browser.settings.zoom_site_overrides.clone(),
        };
        let initial_app_launch_permissions = browser.settings.app_launch_permissions.clone();
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
            content_rules: RwLock::new(content_rules),
            filter_rebuild: Mutex::new(()),
            filter_refresh: Mutex::new(()),
            cloaking: std::sync::Mutex::new(HashMap::new()),
            webview_workspaces: std::sync::RwLock::new(HashMap::new()),
            user_cosmetic_rules: RwLock::new(initial_user_cosmetic_rules),
            userscripts: RwLock::new(initial_userscripts),
            filter_cache_dir,
            proxies: Mutex::new(HashMap::new()),
            vault_key: Mutex::new(None),
            pending_captures: Mutex::new(HashMap::new()),
            closed_tab_sessions: Mutex::new(HashMap::new()),
            pending_restore_states: Mutex::new(HashMap::new()),
            nav_guard_bypass: Mutex::new(HashSet::new()),
            restored_cookies: Mutex::new(HashSet::new()),
            tab_previews: std::sync::Mutex::new(Vec::new()),
            downloads: std::sync::Mutex::new(Vec::new()),
            download_prefs: std::sync::Mutex::new(download_prefs),
            video_popout: std::sync::Mutex::new(Default::default()),
            audible_tabs: std::sync::Mutex::new(HashSet::new()),
            focused_window: std::sync::Mutex::new(None),
            usage: std::sync::Mutex::new(wis_core::usage::UsageLedger::new(
                std::time::Duration::from_secs(
                    wis_core::UserSettings::default()
                        .usage_idle_timeout_secs
                        .into(),
                ),
            )),
            usage_io: tokio::sync::Mutex::new(()),
            muted_tabs: std::sync::Mutex::new(HashSet::new()),
            muted_widgets: std::sync::Mutex::new(HashSet::new()),
            media_permissions: std::sync::RwLock::new(initial_media_permissions),
            media_permissions_session: std::sync::RwLock::new(HashMap::new()),
            autoplay: std::sync::RwLock::new(initial_autoplay),
            site_protection: std::sync::RwLock::new(initial_site_protection),
            site_protection_session: std::sync::RwLock::new(HashMap::new()),
            zoom: std::sync::RwLock::new(initial_zoom),
            pending_media: std::sync::Mutex::new(HashMap::new()),
            media_seq: std::sync::atomic::AtomicU64::new(0),
            app_launch_permissions: std::sync::RwLock::new(initial_app_launch_permissions),
            pending_app_launch: std::sync::Mutex::new(HashMap::new()),
            app_launch_seq: std::sync::atomic::AtomicU64::new(0),
            session_restored: std::sync::atomic::AtomicBool::new(false),
            window_geometry: std::sync::Mutex::new(HashMap::new()),
            geometry_gen: std::sync::atomic::AtomicU64::new(0),
            swipe_accent: std::sync::Mutex::new("#6b77f8".to_string()),
            last_screenshot: std::sync::Mutex::new(None),
            ai_cancels: std::sync::Mutex::new(HashMap::new()),
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

    /// Forget a window's workspace binding (window closed, or a failed open),
    /// returning the workspace it showed.
    pub fn clear_window_workspace(&self, label: &str) -> Option<WorkspaceId> {
        let (shown, snapshot) = {
            let mut map = self
                .window_workspaces
                .lock()
                .expect("window_workspaces poisoned");
            let shown = map.remove(label);
            (shown, map.values().copied().collect::<Vec<_>>())
        };
        self.persist_windowed_workspaces(&snapshot);
        shown
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

    /// Replace the autoplay read-cache.
    pub fn refresh_autoplay_cache(&self, cache: crate::autoplay::AutoplayCache) {
        let mut guard = self.autoplay.write().expect("autoplay poisoned");
        *guard = cache;
    }

    pub async fn refresh_site_protection_cache(&self) {
        let browser = self.browser.lock().await;
        *self
            .site_protection
            .write()
            .expect("site_protection poisoned") =
            browser.settings.site_protection_overrides.clone();
    }

    pub fn site_protection_for(&self, url: &str) -> wis_core::SiteProtection {
        let saved = self
            .site_protection
            .read()
            .expect("site_protection poisoned");
        let session = self
            .site_protection_session
            .read()
            .expect("site_protection_session poisoned");
        crate::site_protection::for_url(&saved, &session, url)
    }

    pub fn wis_rule_list_ids(&self) -> Vec<String> {
        self.content_rules
            .read()
            .expect("content_rules poisoned")
            .values()
            .flatten()
            .map(|spec| spec.identifier.clone())
            .collect()
    }

    /// Re-read the page-zoom cache from the current settings.
    pub async fn refresh_zoom_cache(&self) {
        let browser = self.browser.lock().await;
        let mut guard = self.zoom.write().expect("zoom poisoned");
        *guard = crate::zoom::ZoomCache {
            default_percent: browser.settings.default_zoom_percent,
            overrides: browser.settings.zoom_site_overrides.clone(),
        };
    }

    /// The zoom percentage for the page at `url`.
    pub fn zoom_percent_for_url(&self, url: &str) -> u16 {
        let cache = self.zoom.read().expect("zoom poisoned");
        crate::zoom::percent_for_url(&cache, url)
    }

    /// The autoplay policy for the page at `url`.
    pub fn autoplay_policy_for(&self, url: &str) -> wis_core::AutoplayPolicy {
        let cache = self.autoplay.read().expect("autoplay poisoned");
        crate::autoplay::policy_for_url(&cache, url)
    }

    /// Replace the remembered app-launch read-cache.
    pub fn refresh_app_launch_permissions_cache(&self, perms: HashMap<String, bool>) {
        let mut guard = self
            .app_launch_permissions
            .write()
            .expect("app_launch_permissions poisoned");
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

    /// The rule lists a privacy mode attaches, none for Permissive privacy.
    /// Cloned out so the caller doesn't hold the lock.
    pub fn content_rule_specs(&self, mode: PrivacyMode) -> Vec<RuleSpec> {
        self.content_rules
            .read()
            .expect("content_rules poisoned")
            .get(&mode)
            .cloned()
            .unwrap_or_default()
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
            .with_global_privacy_control(browser.settings.global_privacy_control)
    }

    /// Lazily start (or look up) the workspace's loopback proxy and return
    /// its bound port. Idempotent: subsequent calls for the same workspace
    /// return the same port without re-binding.
    ///
    /// The proxy handles DoH resolution, host-level tracker blocking, and
    /// HTTPS-First upgrades. HTTPS is tunnelled raw; path-level filtering is
    /// enforced in the WebView engine (see `content_filter`).
    pub async fn ensure_proxy_for(&self, workspace_id: WorkspaceId) -> Result<u16, WisShellError> {
        let (privacy_mode, upstream_setting, global_privacy_control) = {
            let browser = self.browser.lock().await;
            let ws = browser
                .workspace(workspace_id)
                .map_err(WisShellError::from)?;
            (
                ws.privacy_mode,
                ws.upstream_proxy.clone(),
                browser.settings.global_privacy_control,
            )
        };

        let upstream = self.resolve_upstream(upstream_setting.clone()).await?;

        let mut proxies = self.proxies.lock().await;
        if let Some(server) = proxies.get(&workspace_id) {
            let port = server.port();
            let refusing = server.refusing();
            drop(proxies);

            if refusing {
                let _ = self.update_proxy_policy(workspace_id, true).await;
            }

            return Ok(port);
        }

        let policy = Arc::new(self.build_request_policy(
            workspace_id,
            privacy_mode,
            upstream,
            global_privacy_control,
        ));
        let bind: SocketAddr = ([127, 0, 0, 1], 0).into();
        let server = ProxyServer::start(policy, self.resolver.clone(), bind)
            .await
            .map_err(|err| WisShellError::Other(format!("proxy start: {err}")))?;
        let port = server.port();

        log::info!("workspace {workspace_id} proxy listening on 127.0.0.1:{port}");
        proxies.insert(workspace_id, server);
        drop(proxies);

        let current = {
            let browser = self.browser.lock().await;
            browser
                .workspace(workspace_id)
                .map(|ws| (ws.privacy_mode, ws.upstream_proxy.clone()))
                .ok()
        };
        if let Some((mode_now, upstream_now)) = current {
            if mode_now != privacy_mode || upstream_now != upstream_setting {
                let _ = self.update_proxy_policy(workspace_id, true).await;
            }
        }

        Ok(port)
    }

    /// Build the request policy for a workspace from its privacy mode + the
    /// current tracker blocker, routed through `upstream` when set.
    fn build_request_policy(
        &self,
        workspace_id: WorkspaceId,
        privacy_mode: PrivacyMode,
        upstream: Option<wis_core::UpstreamProxy>,
        global_privacy_control: bool,
    ) -> RequestPolicy {
        let mut policy = RequestPolicy::new(privacy_mode, self.current_tracker_blocker())
            .with_global_privacy_control(global_privacy_control);
        if crate::uncloak::SUPPORTED {
            policy = policy.with_cloaking(
                crate::uncloak::cloaking_for(self, workspace_id),
                crate::uncloak::unblocked_sites(self),
            );
        }
        match upstream {
            Some(up) => policy.with_upstream(up),
            None => policy,
        }
    }

    /// Re-point every running proxy at its workspace's current policy, for a
    /// global setting change. Open connections are kept.
    pub async fn refresh_all_proxy_policies(&self) {
        let ids: Vec<WorkspaceId> = self.proxies.lock().await.keys().cloned().collect();
        for id in ids {
            if let Err(err) = self.update_proxy_policy(id, false).await {
                log::warn!("workspace {id} proxy policy refresh failed: {err}");
            }
        }
    }

    /// Re-point a running proxy at the workspace's current policy. `sever`
    /// also cuts connections already open, which a route change requires.
    pub async fn update_proxy_policy(
        &self,
        workspace_id: WorkspaceId,
        sever: bool,
    ) -> Result<(), WisShellError> {
        let (privacy_mode, upstream_setting, global_privacy_control) = {
            let browser = self.browser.lock().await;
            match browser.workspace(workspace_id) {
                Ok(ws) => (
                    ws.privacy_mode,
                    ws.upstream_proxy.clone(),
                    browser.settings.global_privacy_control,
                ),
                Err(_) => return Ok(()),
            }
        };
        let upstream = match self.resolve_upstream(upstream_setting.clone()).await {
            Ok(up) => up,
            Err(err) => {
                if let Some(server) = self.proxies.lock().await.get(&workspace_id) {
                    server.update_policy(Arc::new(
                        self.build_request_policy(
                            workspace_id,
                            privacy_mode,
                            None,
                            global_privacy_control,
                        )
                        .refusing_all(),
                    ));
                    server.sever_open_connections();
                }
                log::warn!(
                    "workspace {workspace_id} upstream resolve failed: {err}; proxy refusing"
                );
                return Err(err);
            }
        };

        {
            let browser = self.browser.lock().await;
            if let Ok(ws) = browser.workspace(workspace_id) {
                if ws.privacy_mode != privacy_mode || ws.upstream_proxy != upstream_setting {
                    log::info!("workspace {workspace_id} route changed while resolving; skipping");
                    return Ok(());
                }
            }
        }

        let proxies = self.proxies.lock().await;
        if let Some(server) = proxies.get(&workspace_id) {
            let policy = Arc::new(self.build_request_policy(
                workspace_id,
                privacy_mode,
                upstream,
                global_privacy_control,
            ));
            server.update_policy(policy);
            if sever {
                server.sever_open_connections();
            }
            log::info!("workspace {workspace_id} proxy policy updated in place");
        }

        Ok(())
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

    /// Swap in a new filter list. Rebuilds the [`TrackerBlocker`] (host-level
    /// blocking used by the proxy) and the per-mode WebKit content-rule
    /// specs (engine-level blocking), then re-points every running proxy at
    /// the new blocker in place. Dropping them instead would strand the open
    /// tabs, which keep the port they were built with. Live tab webviews still hold
    /// the previous rule list attached by [`crate::content_filter`]; the
    /// caller is responsible for re-attaching the new specs to active tabs.
    /// Returns the number of bytes in the new filter text for diagnostics.
    pub async fn rebuild_filter_engine(&self, filter_text: String) -> Result<usize, WisShellError> {
        let _one_at_a_time = self.filter_rebuild.lock().await;
        let subscriptions = {
            let browser = self.browser.lock().await;
            crate::filter_engine::Subscriptions::of(&browser.settings)
        };
        let cache_dir = self.filter_cache_dir.clone();
        let size = filter_text.len();
        let (blocker, rules) = tokio::task::spawn_blocking(move || {
            crate::filter_engine::build(&cache_dir, &filter_text, &subscriptions)
        })
        .await
        .map_err(|err| WisShellError::Other(format!("rebuild blocker: {err}")))?
        .map_err(|err| WisShellError::Other(format!("rebuild blocker: {err}")))?;

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
        self.refresh_all_proxy_policies().await;

        log::info!("filter engine rebuilt from {size} bytes of filter text");
        Ok(size)
    }
}

/// Build a resolver for the settings' DoH endpoint. Every connection resolves
/// through this and there is no OS-DNS fallback, so an unset or unusable
/// endpoint still resolves, through the default provider.
pub fn build_resolver(doh_url: Option<&str>) -> Arc<dyn Resolver> {
    Arc::new(DohResolver::for_provider(doh_provider_of(doh_url)))
}

/// The provider an endpoint setting resolves through, unset included.
pub fn doh_provider_of(doh_url: Option<&str>) -> wis_network::DohProvider {
    match doh_url {
        Some(url) => wis_network::doh_provider_for(url),
        None => wis_network::DohProvider::Cloudflare,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wis_network::filter_lists::DEFAULT_TRACKER_LIST;
    use wis_network::StubResolver;
    use wis_storage::SqliteStorage;

    fn app_state_for_test() -> AppState {
        let storage: Arc<dyn Storage> =
            Arc::new(SqliteStorage::open_in_memory().expect("in-memory DB"));
        let blocker = Arc::new(TrackerBlocker::with_default_list().expect("default list"));
        let resolver = Arc::new(SwappableResolver::new(Arc::new(StubResolver::new())));
        AppState::new(
            BrowserState::new_with_defaults(),
            storage,
            blocker,
            resolver,
            std::env::temp_dir().join("wis-filter-test"),
            crate::content_filter::build_rule_specs_from(DEFAULT_TRACKER_LIST, None),
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

        // Clearing the binding reports what it showed and leaves the workspace
        // free (no owner, not occupied).
        assert_eq!(state.clear_window_workspace("main"), Some(primary));
        assert_eq!(state.clear_window_workspace("main"), None);
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
            !state.content_rule_specs(PrivacyMode::Strict).is_empty(),
            "strict gets an engine rule list"
        );
        assert!(
            !state.content_rule_specs(PrivacyMode::Balanced).is_empty(),
            "balanced gets an engine rule list"
        );
        assert!(
            state.content_rule_specs(PrivacyMode::Permissive).is_empty(),
            "permissive applies no engine filter"
        );
        assert_ne!(
            state.content_rule_specs(PrivacyMode::Strict)[0].identifier,
            state.content_rule_specs(PrivacyMode::Balanced)[0].identifier,
            "each mode has a distinct cache identifier"
        );
    }

    #[tokio::test]
    async fn rebuild_filter_engine_swaps_rules_and_blocker() {
        let state = app_state_for_test();
        let before = state.content_rule_specs(PrivacyMode::Balanced).remove(0);
        let new_text = "||example.test^\n||another.test^\n";
        state
            .rebuild_filter_engine(new_text.to_string())
            .await
            .expect("rebuild succeeds");
        let after = state.content_rule_specs(PrivacyMode::Balanced).remove(0);
        assert_ne!(
            before.identifier, after.identifier,
            "swap must produce a fresh identifier"
        );
    }

    #[test]
    fn sites_with_blocking_off_count_by_registrable_domain_and_session_wins() {
        let state = app_state_for_test();
        let off = wis_core::SiteProtection {
            blocking: false,
            ..Default::default()
        };
        {
            let mut saved = state.site_protection.write().unwrap();
            saved.insert("news.example.com".into(), off);
            saved.insert("on.test".into(), wis_core::SiteProtection::default());
        }
        {
            let mut session = state.site_protection_session.write().unwrap();
            session.insert("shop.example.org".into(), off);
            session.insert(
                "news.example.com".into(),
                wis_core::SiteProtection::default(),
            );
        }

        let sites = crate::uncloak::unblocked_sites(&state);
        assert_eq!(
            *sites,
            std::collections::HashSet::from(["example.org".to_owned()])
        );
    }

    #[test]
    fn each_workspace_keeps_its_own_pages_until_it_is_deleted() {
        let state = app_state_for_test();
        let (opening, other) = (WorkspaceId::new(), WorkspaceId::new());
        crate::uncloak::register(&state, "tab-opening", opening);
        crate::uncloak::opening_page(&state, "tab-opening", "https://go.shop.test/offer");
        crate::uncloak::opening_page(&state, "tab-unknown", "https://elsewhere.test/");

        assert_eq!(
            crate::uncloak::cloaking_for(&state, opening).is_page("go.shop.test"),
            crate::uncloak::SUPPORTED
        );
        assert!(!crate::uncloak::cloaking_for(&state, other).is_page("go.shop.test"));
        crate::uncloak::forget_workspace(&state, opening);
        assert!(!crate::uncloak::cloaking_for(&state, opening).is_page("go.shop.test"));
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

    /// Open tabs keep the port they were built with, so a rebuild that
    /// dropped the proxy would leave them refused on every new connection.
    #[tokio::test]
    async fn a_filter_rebuild_keeps_each_proxy_on_its_port() {
        let state = app_state_for_test();
        let ws_id = state.browser.lock().await.active_workspace_id;
        let port = state.ensure_proxy_for(ws_id).await.expect("start");

        state
            .rebuild_filter_engine("||example.test^\n".to_string())
            .await
            .expect("rebuild succeeds");

        assert_eq!(state.ensure_proxy_for(ws_id).await.expect("again"), port);
        tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("the open tabs' port still accepts connections");
    }
}
