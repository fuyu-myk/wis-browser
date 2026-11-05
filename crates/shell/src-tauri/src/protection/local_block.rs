//! Keeps a Tor or SOCKS workspace's pages off this machine on macOS, where
//! WebKit connects to loopback around the data store's proxy. A rule list
//! blocks every load, and the navigation hooks refuse a webview's first
//! document, which can start before the list is attached. Elsewhere the
//! engine sends loopback to the proxy, which refuses it.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Url};
use wis_core::Workspace;

/// Per-window frequency.
const TOAST_EVERY: Duration = Duration::from_secs(30);
const TOAST: &str = "Local addresses are blocked in Tor and SOCKS workspaces";

static TOASTED: LazyLock<Mutex<HashMap<String, Instant>>> = LazyLock::new(Default::default);

/// Whether `workspace`'s pages need keeping off this machine here rather
/// than by its proxy.
pub fn applies(workspace: &Workspace) -> bool {
    cfg!(target_os = "macos") && workspace.upstream_proxy.is_some()
}

/// For a navigation hook in such a workspace: true when `url` is this
/// machine, after telling `window` why it did not load.
pub fn refuses(app: &AppHandle, window: &str, url: &Url) -> bool {
    if !wis_network::is_loopback(url) {
        return false;
    }

    let due = due(
        &mut TOASTED.lock().unwrap_or_else(PoisonError::into_inner),
        window,
        Instant::now(),
    );
    if due {
        crate::chrome::remote_toast::show(app, window, TOAST);
    }
    true
}

/// Whether `window` gets the explanation now, which then counts as given.
fn due(toasted: &mut HashMap<String, Instant>, window: &str, now: Instant) -> bool {
    let due = toasted
        .get(window)
        .is_none_or(|at| now.duration_since(*at) >= TOAST_EVERY);
    if due {
        toasted.insert(window.to_owned(), now);
    }
    due
}

#[cfg(test)]
mod tests {
    use super::*;
    use wis_core::{AccentColor, UpstreamProxy};

    #[test]
    fn a_window_hears_why_once_while_a_page_keeps_trying() {
        let mut toasted = HashMap::new();
        let start = Instant::now();
        assert!(due(&mut toasted, "main", start));
        assert!(!due(&mut toasted, "main", start + Duration::from_secs(1)));
        assert!(due(&mut toasted, "win-2", start + Duration::from_secs(1)));
        assert!(due(&mut toasted, "main", start + TOAST_EVERY));
    }

    #[test]
    fn only_a_proxied_workspace_on_macos_needs_it() {
        let workspace = Workspace::new("w".into(), AccentColor::Primary);
        assert!(!applies(&workspace));
        let proxied = workspace.with_upstream_proxy(Some(UpstreamProxy::Tor));
        assert_eq!(applies(&proxied), cfg!(target_os = "macos"));
    }
}
