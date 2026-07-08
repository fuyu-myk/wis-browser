//! Page->chrome web-message dispatch for Linux (WebKitGTK).
//!
//! WebKitGTK, like WKWebView, exposes `window.webkit.messageHandlers.<name>`
//! natively once `name` is registered on the webview's `UserContentManager`, so
//! the shared page scripts post unchanged with no shim. This module registers
//! the named handlers on a tab webview and routes each received message to the
//! feature that owns it, mirroring that feature's macOS `install` handler.
//!
//! This module is the Linux replacement for the macOS per-feature `install`
//! functions, wired once per tab webview.

use javascriptcore::ValueExt;
use webkit2gtk::{JavascriptResult, UserContentManager, UserContentManagerExt, WebViewExt};

/// Register the page->chrome message handlers on a tab webview and route each
/// received message to its feature. Runs on the GTK main thread via `with_webview`.
pub fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
    let _ = webview.with_webview(|w| {
        let Some(ucm) = w.inner().user_content_manager() else {
            log::warn!("gtk_dispatch: webview has no user content manager");
            return;
        };

        register(&ucm, "wisOpenInTab");
    });
}

/// Register one named handler and connect its `script-message-received::<name>`
/// signal, routing the decoded body to [`route`]. The connection lives on the
/// `UserContentManager` (owned by the webview), so it is dropped with the tab
/// and its `SignalHandlerId` is not tracked.
fn register(ucm: &UserContentManager, name: &'static str) {
    if !ucm.register_script_message_handler(name) {
        log::warn!("gtk_dispatch: failed to register handler {name}");
        return;
    }

    ucm.connect_script_message_received(Some(name), move |_ucm, result| {
        if let Some(body) = decode(result) {
            route(name, &body);
        }
    });
}

/// Decode a received message's JS value into a `serde_json::Value`. The page
/// posts the body directly, so this is the value's own JSON form.
fn decode(result: &JavascriptResult) -> Option<serde_json::Value> {
    let json = result.js_value()?.to_json(0)?;
    match serde_json::from_str(json.as_str()) {
        Ok(value) => Some(value),
        Err(err) => {
            log::warn!("gtk_dispatch: undecodable message: {err}");
            None
        }
    }
}

/// Route one decoded message body to the feature named by `name`. Channels are
/// added here as each feature is ported to Linux; the full set lives in
/// [`crate::win_dispatch`].
fn route(name: &str, body: &serde_json::Value) {
    match name {
        "wisOpenInTab" => {
            if let Some(href) = body.as_str() {
                crate::link_open::dispatch_message(href);
            }
        }
        other => log::debug!("gtk_dispatch: handler {other} not yet wired"),
    }
}
