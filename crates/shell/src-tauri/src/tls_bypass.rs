//! Session-scoped TLS-interstitial bypass: a per-host **certificate-pinned**
//! allowlist consulted by each engine's certificate hook, and the command
//! the interstitial's buried "proceed anyway" affordance invokes.
//!
//! Pinning is bound to the failing navigation, not to a mutable by-host
//! store. When a load fails on a TLS fault, the failing server's leaf
//! certificate is captured *from that navigation's own error* and frozen
//! against the tab (`record_candidate`). Proceeding pins that frozen
//! certificate (`pin`); the engine hooks then accept a later connection only
//! when it presents the identical certificate. A concurrent connection to
//! the same host cannot substitute different bytes into what gets pinned.
//!
//! The stores are module statics rather than `AppState` because the macOS
//! and Windows certificate callbacks cannot touch managed state or take an
//! async lock. Nothing is persisted; every entry dies with the process, so a
//! bypass never outlives the session that approved it.

use tauri::{AppHandle, State};
use url::Url;
use wis_core::{TabId, WisError, WisResult};

use crate::state::AppState;

/// Lowercase and strip IPv6 brackets, so `url::Url::host_str` (`"[::1]"`)
/// and macOS `NSURLProtectionSpace.host` (`"::1"`) key identically.
pub(crate) fn normalize_host(host: &str) -> String {
    host.trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase()
}

#[cfg(any(target_os = "macos", windows))]
mod pinned_store {
    use std::collections::HashMap;
    use std::sync::{LazyLock, Mutex, PoisonError};

    use wis_core::TabId;

    /// A frozen bypass candidate: the failing `(normalized host, certificate
    /// bytes)` for a tab's current TLS interstitial.
    type Candidate = (String, Vec<u8>);

    /// The failing certificate captured for a tab's current TLS interstitial
    /// (macOS: DER from the navigation's peer-trust userInfo; Windows: PEM
    /// from the certificate-error event).
    static CANDIDATE: LazyLock<Mutex<HashMap<TabId, Candidate>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));

    /// Certificates the user pinned via the interstitial, by host.
    static PINNED: LazyLock<Mutex<HashMap<String, Vec<u8>>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));

    /// Freeze `cert` as the candidate for `tab`'s current interstitial, bound
    /// to `host`. Called from the load-failure path with the certificate that
    /// produced *this* tab's failure.
    pub fn record_candidate(tab: TabId, host: &str, cert: Vec<u8>) {
        CANDIDATE
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(tab, (super::normalize_host(host), cert));
    }

    /// Drop a tab's frozen candidate (on navigation away).
    pub fn clear_candidate(tab: TabId) {
        CANDIDATE
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&tab);
    }

    /// Whether `presented` is the exact certificate the user pinned for
    /// `host` this session. Called from native TLS callbacks on every
    /// candidate connection; pure lookup, never mutates.
    pub fn is_pinned(host: &str, presented: &[u8]) -> bool {
        PINNED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&super::normalize_host(host))
            .is_some_and(|pinned| pinned == presented)
    }

    /// Pin `tab`'s frozen candidate certificate to `host`, consuming the
    /// candidate.
    pub fn pin(tab: TabId, host: &str) -> Result<(), ()> {
        let host = super::normalize_host(host);
        let (cand_host, cert) = CANDIDATE
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&tab)
            .ok_or(())?;
        if cand_host != host {
            return Err(());
        }

        PINNED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(host, cert);

        Ok(())
    }
}

#[cfg(any(target_os = "macos", windows))]
pub use pinned_store::{clear_candidate, is_pinned, record_candidate};

/// macOS evaluator registered with wry: accept a server-trust challenge only
/// when its `(host, leaf DER)` is already pinned.
#[cfg(target_os = "macos")]
pub fn evaluate(host: &str, presented: &[u8]) -> bool {
    is_pinned(host, presented)
}

/// Proceed past the TLS interstitial on `id`'s tab. The failing URL comes
/// from tab state. The URL is the navigation target: on a cross-host redirect
/// the interstitial's host is the redirector, and the engines fail closed on
/// the mismatch.
#[tauri::command]
pub async fn tls_bypass_proceed(
    app: AppHandle,
    state: State<'_, AppState>,
    id: TabId,
) -> WisResult<()> {
    let snapshot = crate::commands::read_state(&state).await;
    let tab = snapshot.tab(id)?;
    let error = tab
        .load_error
        .as_ref()
        .ok_or_else(|| WisError::InvalidInput {
            message: "tab has no failed load to bypass".into(),
        })?;
    if !error.bypassable() {
        return Err(WisError::InvalidInput {
            message: "this TLS failure cannot be bypassed".into(),
        });
    }

    let url = error.url.clone();
    let parsed = Url::parse(&url).map_err(|_| WisError::InvalidUrl { url: url.clone() })?;
    if parsed.scheme() != "https" {
        return Err(WisError::InvalidInput {
            message: "TLS bypass applies to https URLs only".into(),
        });
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| WisError::InvalidUrl { url: url.clone() })?;

    #[cfg(any(target_os = "macos", windows))]
    pinned_store::pin(id, host).map_err(|()| WisError::InvalidInput {
        message: "no certificate was captured for this failure; try the load again first".into(),
    })?;

    #[cfg(target_os = "linux")]
    crate::load_failure::allow_stashed_certificate(&app, id, host);

    crate::commands::navigate_tab(app, state, id, url).await
}
