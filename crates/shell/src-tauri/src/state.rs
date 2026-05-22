//! Application state container managed by Tauri.
//!
//! Holds:
//! - the in-memory [`BrowserState`] behind an async mutex;
//! - the persistent [`Storage`] backend;
//! - the long-lived [`TrackerBlocker`] and DoH [`Resolver`] used by the
//!   privacy decision engine.
//!
//! Commands acquire the async mutex, compute the new state, persist it, then
//! replace the in-memory copy — see `mutate` in `commands`. The network
//! handles are built once at startup and queried by the (future) transport
//! layer that intercepts WebView traffic.

use std::sync::Arc;

use tokio::sync::Mutex;
use wis_core::BrowserState;
use wis_network::{DohResolver, RequestPolicy, Resolver, StubResolver, TrackerBlocker};
use wis_storage::Storage;

pub struct AppState {
    pub browser: Mutex<BrowserState>,
    pub storage: Arc<dyn Storage>,
    pub tracker_blocker: Arc<TrackerBlocker>,
    pub resolver: Arc<dyn Resolver>,
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
    use wis_core::PrivacyMode;
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
    fn build_resolver_handles_unknown_doh_endpoints() {
        let _ = build_resolver(Some("https://my-doh.example/dns-query"));
        let _ = build_resolver(None);
    }
}
