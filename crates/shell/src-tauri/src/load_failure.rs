//! Native page-load failure hooks for Windows and Linux.
//!
//! WebView2 and WebKitGTK expose their own per-webview failure callbacks, so
//! each registers one here and reports the failure into tab state. The chrome
//! then renders an error page in the tab's place (`SplitPaneHost`).

use tauri::{AppHandle, Manager};
use wis_core::{LoadError, TabId};

use crate::state::AppState;

/// Set (or clear) a tab's load error from a native callback. `failure`
/// carries the normalized fault with a placeholder URL; the failing URL is
/// filled from tab state (the target URL, written before navigation). Runs
/// the state mutation off the callback.
pub fn report(app: &AppHandle, tab_id: TabId, failure: Option<LoadError>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let result = crate::commands::mutate_unit(&app, &state, |s| {
            let error = failure.map(|err| {
                let url = s.tab(tab_id).map(|t| t.url.to_string()).unwrap_or_default();
                LoadError { url, ..err }
            });

            s.with_tab_load_error(tab_id, error)
        })
        .await;

        if let Err(err) = result {
            log::warn!("load-failure state update for {tab_id} failed: {err}");
        }
    });
}

/// Windows: a second `NavigationCompleted` handler reads `IsSuccess`/`WebErrorStatus`
/// and reports a failure. It never clears on success. Called per tab webview.
#[cfg(windows)]
pub fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>, tab_id: TabId) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2, ICoreWebView2NavigationCompletedEventArgs,
        COREWEBVIEW2_WEB_ERROR_STATUS_UNKNOWN,
    };
    use webview2_com::NavigationCompletedEventHandler;

    let app = webview.app_handle().clone();
    crate::win_webview2::with_core_webview2(webview, move |core| {
        let handler = NavigationCompletedEventHandler::create(Box::new(
            move |_sender: Option<ICoreWebView2>,
                  args: Option<ICoreWebView2NavigationCompletedEventArgs>|
                  -> windows::core::Result<()> {
                let Some(args) = args else {
                    return Ok(());
                };

                let mut success = windows::core::BOOL(0);
                // SAFETY: UI thread (with_core_webview2); out-params are stack locals.
                unsafe { args.IsSuccess(&mut success)? };
                if success.as_bool() {
                    return Ok(());
                }

                let mut status = COREWEBVIEW2_WEB_ERROR_STATUS_UNKNOWN;
                // SAFETY: as above.
                unsafe { args.WebErrorStatus(&mut status)? };
                if let Some(err) = LoadError::from_web_error_status(status.0 as u32, String::new())
                {
                    report(&app, tab_id, Some(err));
                }

                Ok(())
            },
        ));
        let mut token = 0_i64;

        // SAFETY: runs on the UI thread via with_core_webview2.
        if let Err(err) = unsafe { core.add_NavigationCompleted(&handler, &mut token) } {
            log::warn!("load_failure: add_NavigationCompleted failed: {err}");
        }
    });
}

/// Linux: connect WebKitGTK's `load-failed` and `load-failed-with-tls-errors`
/// signals. Returning `true` suppresses the engine's built-in error page so
/// the chrome error page is the only one shown. Called per tab webview.
#[cfg(target_os = "linux")]
pub fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>, tab_id: TabId) {
    use webkit2gtk::{NetworkError, WebViewExt};

    let app = webview.app_handle().clone();
    let app_tls = app.clone();
    let _ = webview.with_webview(move |w| {
        let wv = w.inner();

        wv.connect_load_failed(move |_wv, _event, uri, error| {
            let cancelled = error.matches(NetworkError::Cancelled);
            if let Some(err) = LoadError::from_glib(cancelled, None, uri.to_string()) {
                report(&app, tab_id, Some(err));
            }

            // Suppress the engine's default error page only when we showed ours.
            !cancelled
        });

        wv.connect_load_failed_with_tls_errors(move |_wv, uri, _cert, flags| {
            if let Some(err) = LoadError::from_glib(false, Some(flags.bits()), uri.to_string()) {
                report(&app_tls, tab_id, Some(err));
            }
            true
        });
    });
}
