//! Application state container managed by Tauri.
//!
//! Holds:
//! - the in-memory [`BrowserState`] behind an async mutex;
//! - the persistent [`Storage`] backend;
//! - the long-lived [`TrackerBlocker`] and DoH [`Resolver`] used by the
//!   privacy decision engine;
//! - the precomputed per-mode content-filter rule specs;
//! - one [`ProxyServer`] per workspace, started lazily on first use.
//!   Each tab webview is pointed at its workspace's proxy via `WebviewBuilder::proxy_url`.
//!
//! Commands acquire the async mutex, compute the new state, persist it, then
//! replace the in-memory copy: see `mutate` in `commands`.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::sync::Mutex;
use wis_core::{BrowserState, PrivacyMode, WorkspaceId};
use wis_network::{
    DohResolver, ProxyServer, RequestPolicy, Resolver, StubResolver, TrackerBlocker,
};
use wis_storage::Storage;

use crate::content_filter::{build_rule_specs, RuleSpec};
use crate::error_compat::WisShellError;

pub struct AppState {
    pub browser: Mutex<BrowserState>,
    pub storage: Arc<dyn Storage>,
    pub tracker_blocker: Arc<TrackerBlocker>,
    pub resolver: Arc<dyn Resolver>,
    pub content_rules: HashMap<PrivacyMode, Option<RuleSpec>>,
    pub proxies: Mutex<HashMap<WorkspaceId, ProxyServer>>,
}

impl AppState {
    pub fn new(
        browser: BrowserState,
        storage: Arc<dyn Storage>,
        tracker_blocker: Arc<TrackerBlocker>,
        resolver: Arc<dyn Resolver>,
    ) -> Self {
        Self {
            browser: Mutex::new(browser),
            storage,
            tracker_blocker,
            resolver,
            content_rules: build_rule_specs(),
            proxies: Mutex::new(HashMap::new()),
        }
    }

    /// The content-filter rule spec for a privacy mode, or `None` when the
    /// mode applies no engine-level filtering.
    pub fn content_rule_spec(&self, mode: PrivacyMode) -> Option<&RuleSpec> {
        self.content_rules.get(&mode).and_then(|spec| spec.as_ref())
    }

    /// Build a [`RequestPolicy`] for the currently-active workspace.
    ///
    /// Per-site overrides will be threaded through here once they're added to
    /// `BrowserState`.
    pub async fn request_policy_for_active_workspace(&self) -> RequestPolicy {
        let browser = self.browser.lock().await;
        let mode = browser.active_workspace().privacy_mode;
        RequestPolicy::new(mode, self.tracker_blocker.clone())
    }

    /// Lazily start (or look up) the workspace's loopback proxy and return
    /// its bound port. Idempotent: subsequent calls for the same workspace
    /// return the same port without re-binding.
    ///
    /// The proxy handles DoH resolution, host-level tracker blocking, and
    /// HTTPS-First upgrades. HTTPS is tunnelled raw; path-level filtering is
    /// enforced in the WebView engine (see `content_filter`).
    pub async fn ensure_proxy_for(&self, workspace_id: WorkspaceId) -> Result<u16, WisShellError> {
        let privacy_mode = {
            let browser = self.browser.lock().await;
            browser
                .workspace(workspace_id)
                .map_err(WisShellError::from)?
                .privacy_mode
        };

        let mut proxies = self.proxies.lock().await;
        if let Some(server) = proxies.get(&workspace_id) {
            return Ok(server.port());
        }

        let policy = Arc::new(RequestPolicy::new(
            privacy_mode,
            self.tracker_blocker.clone(),
        ));
        let bind: SocketAddr = ([127, 0, 0, 1], 0).into();
        let server = ProxyServer::start(policy, self.resolver.clone(), bind)
            .await
            .map_err(|err| WisShellError::Other(format!("proxy start: {err}")))?;
        let port = server.port();

        log::info!("workspace {workspace_id} proxy listening on 127.0.0.1:{port}");
        proxies.insert(workspace_id, server);

        Ok(port)
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
        )
    }

    #[tokio::test]
    async fn request_policy_reflects_active_workspaces_privacy_mode() {
        let state = app_state_for_test();
        let policy = state.request_policy_for_active_workspace().await;
        // Default workspace ships with the Balanced privacy preset.
        assert_eq!(policy.privacy_mode(), PrivacyMode::Balanced);
    }

    #[tokio::test]
    async fn request_policy_picks_up_workspace_mode_changes() {
        let state = app_state_for_test();
        // Flip the active workspace to Strict mode in place.
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
                .map(|s| s.identifier.as_str()),
            state
                .content_rule_spec(PrivacyMode::Balanced)
                .map(|s| s.identifier.as_str()),
            "each mode has a distinct cache identifier"
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
        // After clear, ensure_proxy_for binds a fresh listener (which will
        // pick a new ephemeral port).
        let _new_port = state.ensure_proxy_for(ws_id).await.expect("rebind");
    }
}
