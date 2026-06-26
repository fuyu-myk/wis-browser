//! Shared access to the WebView2 handles behind a Tauri webview on Windows.
//!
//! Tauri's `PlatformWebview` exposes the WebView2 `controller`/`environment`
//! only on Windows, and `with_webview` runs its closure on the UI thread, which
//! is where COM calls must happen. These helpers centralize the
//! controller -> CoreWebView2 plumbing the per-feature Windows ports share,
//! so each feature only writes the one or two calls unique to it.

use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2, ICoreWebView2Controller};

/// Run `f` with the webview's live `ICoreWebView2`, on the UI thread. A no-op
/// if the controller has no `CoreWebView2` yet.
pub fn with_core_webview2<R, F>(webview: &tauri::webview::Webview<R>, f: F)
where
    R: tauri::Runtime,
    F: FnOnce(&ICoreWebView2) + Send + 'static,
{
    let _ = webview.with_webview(move |w| {
        // SAFETY: dispatched to the UI thread by `with_webview`; `CoreWebView2`
        // is a plain getter on the controller and the handle never leaves here.
        if let Ok(core) = unsafe { w.controller().CoreWebView2() } {
            f(&core);
        }
    });
}

/// Run `f` with the webview's `ICoreWebView2Controller`, on the UI thread.
pub fn with_controller<R, F>(webview: &tauri::webview::Webview<R>, f: F)
where
    R: tauri::Runtime,
    F: FnOnce(&ICoreWebView2Controller) + Send + 'static,
{
    // SAFETY: dispatched to the UI thread by `with_webview`; the controller
    // handle stays inside the closure.
    let _ = webview.with_webview(move |w| f(&w.controller()));
}
