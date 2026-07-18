//! Full webview teardown. wry's macOS `Drop` deliberately leaks the
//! `WKWebView` (retains it after removing it from the view hierarchy), so
//! `Webview::close` alone strands the page's WebContent process. Closing
//! the page first via the `_close` SPI makes WebKit exit the content
//! process; the leaked NSObject that remains is a few KB of shell.

/// Close a webview's WebKit page so its content process exits.
///
/// Called immediately before `Webview::close` on every destroy path. Ordering
/// holds without blocking: `with_webview` and `close` are dispatched FIFO onto
/// the same main-thread queue. No-op off macOS, where teardown already works.
#[cfg(target_os = "macos")]
pub fn terminate_page(webview: &tauri::Webview) {
    use objc2::{msg_send, sel};

    let dispatched = webview.with_webview(|w| {
        let wk = w.inner() as *mut objc2::runtime::AnyObject;
        if wk.is_null() {
            return;
        }

        // SAFETY: `wk` is the live WKWebView handed over by `with_webview` on
        // the main thread; `_close` is gated on `respondsToSelector`.
        unsafe {
            let responds: bool = msg_send![wk, respondsToSelector: sel!(_close)];
            if responds {
                let _: () = msg_send![wk, _close];
            }
        }
    });
    if let Err(err) = dispatched {
        log::warn!("terminate_page dispatch failed (content process will leak): {err}");
    }
}

#[cfg(not(target_os = "macos"))]
pub fn terminate_page(_webview: &tauri::Webview) {}

/// Tear down every remote-content webview owned by `window_label` (tabs,
/// widget panels, peek). Runs on window close, where the OS window otherwise
/// drops its children through the same leaking `Drop`. Chrome and overlay
/// webviews are left alone: they are local app surfaces.
pub fn terminate_window_pages(app: &tauri::AppHandle, window_label: &str) {
    use tauri::Manager;

    for (label, wv) in app.webviews() {
        if wv.window().label() != window_label {
            continue;
        }

        if label.starts_with("tab-")
            || label.starts_with("widget-panel-")
            || label.starts_with("peek-page-")
        {
            terminate_page(&wv);
        }
    }
}
