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
    /// Serialized WKWebView `interactionState` captured at close, so reopen
    /// restores history/scroll/forms instead of cold-loading. `None` when the
    /// webview was already discarded (slept) at close time.
    pub interaction_state: Option<String>,
}

pub struct AppState {
    pub browser: Mutex<BrowserState>,
    pub storage: Arc<dyn Storage>,
    /// Debounced, ordered writer for [`BrowserState`] snapshots (see
    /// [`crate::save_coalescer`]). Destructive transitions bypass the
    /// debounce via [`write_now`](crate::save_coalescer::SaveCoalescer::write_now).
    pub save_coalescer: Arc<crate::save_coalescer::SaveCoalescer>,
    pub tracker_blocker: RwLock<Arc<TrackerBlocker>>,
    pub resolver: Arc<dyn Resolver>,
    pub content_rules: RwLock<HashMap<PrivacyMode, Option<RuleSpec>>>,
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
        Self {
            browser: Mutex::new(browser),
            storage,
            save_coalescer: Arc::new(crate::save_coalescer::SaveCoalescer::new()),
            tracker_blocker: RwLock::new(tracker_blocker),
            resolver,
            content_rules: RwLock::new(build_rule_specs_from(initial_filter_text)),
            filter_cache_dir,
            proxies: Mutex::new(HashMap::new()),
            vault_key: Mutex::new(None),
            pending_capture: Mutex::new(None),
            closed_tabs: Mutex::new(Vec::new()),
            pending_restore_states: Mutex::new(HashMap::new()),
            nav_guard_bypass: Mutex::new(HashSet::new()),
            restored_cookies: Mutex::new(HashSet::new()),
            #[cfg(feature = "tor")]
            tor_proxy: Mutex::new(None),
        }
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
        let blocker =
            TrackerBlocker::from_filter_texts_with_cache(&[&filter_text], &self.filter_cache_dir)
                .map_err(|err| WisShellError::Other(format!("rebuild blocker: {err}")))?;
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
