//! Application state container managed by Tauri.
//!
//! Holds:
//! - the in-memory [`BrowserState`] behind an async mutex;
//! - the persistent [`Storage`] backend;
//! - the long-lived [`TrackerBlocker`] and DoH [`Resolver`] used by the
//!   privacy decision engine;
//! - the local [`CertAuthority`] used by MITM proxies;
//! - one [`ProxyServer`] per workspace, started lazily on first use.
//!   Each tab webview is pointed at its workspace's proxy via `WebviewBuilder::proxy_url`.
//!
//! Commands acquire the async mutex, compute the new state, persist it, then
//! replace the in-memory copy: see `mutate` in `commands`.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use tokio::sync::Mutex;
use wis_core::{BrowserState, PrivacyMode, WorkspaceId};
use wis_network::{
    ca_dir, CertAuthority, DohResolver, MitmConfig, ProxyServer, RequestPolicy, Resolver,
    StubResolver, TrackerBlocker,
};
use wis_storage::Storage;

use crate::error_compat::WisShellError;

pub struct AppState {
    pub browser: Mutex<BrowserState>,
    pub storage: Arc<dyn Storage>,
    pub tracker_blocker: Arc<TrackerBlocker>,
    pub resolver: Arc<dyn Resolver>,
    pub ca: Arc<CertAuthority>,
    pub proxies: Mutex<HashMap<WorkspaceId, ProxyServer>>,
}

impl AppState {
    pub fn new(
        browser: BrowserState,
        storage: Arc<dyn Storage>,
        tracker_blocker: Arc<TrackerBlocker>,
        resolver: Arc<dyn Resolver>,
        ca: Arc<CertAuthority>,
    ) -> Self {
        Self {
            browser: Mutex::new(browser),
            storage,
            tracker_blocker,
            resolver,
            ca,
            proxies: Mutex::new(HashMap::new()),
        }
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
    /// MITM is engaged when the workspace is in `PrivacyMode::Strict` and
    /// the user has flipped `UserSettings::mitm_enabled` on. Otherwise the
    /// proxy tunnels HTTPS raw.
    pub async fn ensure_proxy_for(&self, workspace_id: WorkspaceId) -> Result<u16, WisShellError> {
        let (privacy_mode, mitm_enabled) = {
            let browser = self.browser.lock().await;
            let mode = browser
                .workspace(workspace_id)
                .map_err(WisShellError::from)?
                .privacy_mode;
            (mode, browser.settings.mitm_enabled)
        };

        let mut proxies = self.proxies.lock().await;
        if let Some(server) = proxies.get(&workspace_id) {
            return Ok(server.port());
        }

        let policy = Arc::new(RequestPolicy::new(
            privacy_mode,
            self.tracker_blocker.clone(),
        ));
        let mitm = if mitm_enabled && privacy_mode == PrivacyMode::Strict {
            match MitmConfig::new(self.ca.clone()) {
                Ok(cfg) => Some(Arc::new(cfg)),
                Err(err) => {
                    log::warn!("workspace {workspace_id} mitm setup failed ({err}); tunneling");
                    None
                }
            }
        } else {
            None
        };
        let bind: SocketAddr = ([127, 0, 0, 1], 0).into();
        let server = ProxyServer::start(policy, self.resolver.clone(), bind, mitm.clone())
            .await
            .map_err(|err| WisShellError::Other(format!("proxy start: {err}")))?;
        let port = server.port();

        log::info!(
            "workspace {workspace_id} proxy listening on 127.0.0.1:{port}{}",
            if mitm.is_some() { " (mitm)" } else { "" }
        );
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

/// Load (or generate) the Wis local CA from the canonical sub-directory of
/// the Tauri-resolved app-data dir. Failure is fatal at startup: without a
/// CA we cannot MITM, and the toggle in settings would silently no-op.
pub fn load_or_generate_ca(app_data_dir: &Path) -> Result<Arc<CertAuthority>, WisShellError> {
    let dir = ca_dir(app_data_dir);
    let ca = CertAuthority::load_or_generate(&dir)
        .map_err(|err| WisShellError::Other(format!("ca load: {err}")))?;
    log::info!("wis CA available at {}", dir.display());
    Ok(Arc::new(ca))
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
        let ca_dir = std::env::temp_dir().join(format!(
            "wis-state-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let ca = Arc::new(CertAuthority::load_or_generate(&ca_dir).expect("ca"));
        AppState::new(
            BrowserState::new_with_defaults(),
            storage,
            blocker,
            resolver,
            ca,
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
