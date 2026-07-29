//! Fractional page-load progress for the URL bar strip.
//!
//! WKWebView (macOS) and WebKitGTK (Linux) expose an estimated-progress
//! property but wry surfaces no event for it, so a per-load poller reads it
//! every 100ms between the Started and Finished page-load events and emits
//! `wis-load-progress` to the chrome. WebView2 exposes no fractional
//! progress; the chrome plays an indeterminate ramp there off `Tab.loading`
//! alone, so no poller is spawned.

#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::collections::HashSet;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::sync::{LazyLock, Mutex};

use tauri::AppHandle;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use tauri::{Emitter, Manager};

use wis_core::TabId;

#[cfg(any(target_os = "macos", target_os = "linux"))]
const POLL_MS: u64 = 100;
/// Hard cap so a poller cannot outlive a load whose Finished event never fires.
#[cfg(any(target_os = "macos", target_os = "linux"))]
const MAX_POLL_MS: u64 = 30_000;

#[cfg(any(target_os = "macos", target_os = "linux"))]
const LOAD_PROGRESS_EVENT: &str = "wis-load-progress";

/// One poller per tab at a time; Started can fire repeatedly across redirects.
#[cfg(any(target_os = "macos", target_os = "linux"))]
static POLLING: LazyLock<Mutex<HashSet<TabId>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressPayload {
    tab_id: TabId,
    progress: f64,
}

/// Spawn the progress poller for one load. Ends when the tab's `loading`
/// flag clears, the tab or webview disappears, or the cap elapses.
pub fn start(app: &AppHandle, tab_id: TabId) {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        if !POLLING
            .lock()
            .expect("load_progress poisoned")
            .insert(tab_id)
        {
            return;
        }

        // Starter event: the chrome's `loading` flag rides a coalesced
        // full-state refetch and can skip the brief true state of a fast
        // load entirely; this event is the reliable "load began" signal.
        let starter = ProgressPayload {
            tab_id,
            progress: 0.02,
        };
        if let Err(err) = app.emit(LOAD_PROGRESS_EVENT, starter) {
            log::warn!("emit {LOAD_PROGRESS_EVENT} failed: {err}");
        }

        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let label = format!("tab-{}", tab_id.as_uuid());
            let mut elapsed = 0u64;
            let mut last = 0.0f64;
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(POLL_MS)).await;
                elapsed += POLL_MS;
                if elapsed > MAX_POLL_MS {
                    break;
                }

                let loading = {
                    let state = app.state::<crate::state::AppState>();
                    let browser = state.browser.lock().await;
                    browser.tab(tab_id).map(|t| t.loading).unwrap_or(false)
                };
                // Grace before honoring "not loading".
                if !loading && elapsed > 300 {
                    break;
                }
                let Some(webview) = app.webviews().get(&label).cloned() else {
                    break;
                };

                if let Some(progress) = read_progress(&webview).await {
                    if progress > last {
                        last = progress;
                        let payload = ProgressPayload { tab_id, progress };
                        if let Err(err) = app.emit(LOAD_PROGRESS_EVENT, payload) {
                            log::warn!("emit {LOAD_PROGRESS_EVENT} failed: {err}");
                        }
                    }
                }
            }

            POLLING
                .lock()
                .expect("load_progress poisoned")
                .remove(&tab_id);
        });
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (app, tab_id);
    }
}

#[cfg(target_os = "macos")]
async fn read_progress<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) -> Option<f64> {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    let (tx, rx) = tokio::sync::oneshot::channel::<f64>();
    // SAFETY: the closure runs on the main thread via `with_webview`;
    // `inner()` is the live WKWebView pointer, null-checked before use.
    let _ = webview.with_webview(move |w| unsafe {
        let wk: *mut AnyObject = w.inner() as *mut AnyObject;
        if wk.is_null() {
            return;
        }

        let progress: f64 = msg_send![wk, estimatedProgress];
        let _ = tx.send(progress);
    });

    tokio::time::timeout(std::time::Duration::from_millis(100), rx)
        .await
        .ok()?
        .ok()
}

#[cfg(target_os = "linux")]
async fn read_progress<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) -> Option<f64> {
    let (tx, rx) = tokio::sync::oneshot::channel::<f64>();
    let _ = webview.with_webview(move |w| {
        use webkit2gtk::WebViewExt;
        let _ = tx.send(w.inner().estimated_load_progress());
    });

    tokio::time::timeout(std::time::Duration::from_millis(100), rx)
        .await
        .ok()?
        .ok()
}
