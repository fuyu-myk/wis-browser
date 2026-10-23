//! Page->chrome web-message dispatch for Linux (WebKitGTK).
//!
//! WebKitGTK, like WKWebView, exposes `window.webkit.messageHandlers.<name>`
//! natively once `name` is registered on the webview's `UserContentManager`, so
//! the shared page scripts post unchanged with no shim. This module registers
//! the named handlers on a tab webview and routes each received message to the
//! feature that owns it, mirroring that feature's macOS `install` handler.
//!
//! This module is the Linux replacement for the macOS per-feature `install`
//! functions, wired once per tab webview. As on macOS, the channels whose
//! message makes the browser act, and their scripts, live in Wis's own script
//! world (`content_world`).

use javascriptcore::ValueExt;
use webkit2gtk::{JavascriptResult, UserContentManager, UserContentManagerExt, WebViewExt};

use crate::platform::content_world::linux as content_world;

/// Register the page->chrome message handlers on a tab webview and route each
/// received message to its feature. Runs on the GTK main thread via `with_webview`.
pub fn install<R: tauri::Runtime>(
    webview: &tauri::webview::Webview<R>,
    ephemeral: bool,
    cosmetic_enabled: bool,
    canvas: bool,
    keybind_script: String,
) {
    let label = webview.label().to_string();
    let _ = webview.with_webview(move |w| {
        let wv = w.inner();

        // getUserMedia consent is enforced in JS (wisMediaPermission); the native
        // request that follows is post-consent, so allow it or WebKitGTK denies
        // camera/mic by default.
        if !canvas {
            crate::protection::permissions::install_media_bridge(&wv);
        }
        crate::pages::find::install_linux(&wv, &label);

        let Some(ucm) = wv.user_content_manager() else {
            log::warn!("gtk_dispatch: webview has no user content manager");
            return;
        };

        if !canvas {
            register(&ucm, "wisMediaPermission", &label);
            register(&ucm, "wisPageText", &label);
            register(&ucm, "wisPageSource", &label);
        }

        // Cosmetic filtering + the element-picker zapper.
        if cosmetic_enabled {
            register(&ucm, "wisCosmetic", &label);
            register(&ucm, "wisZap", &label);
        }

        let page_menu = crate::pages::context_menu::page_script();
        let mut isolated: Vec<(&'static str, &str)> = vec![
            (
                crate::pages::link_open::HANDLER_BACKGROUND,
                crate::pages::link_open::page_script(),
            ),
            ("wisNavGuard", crate::protection::navguard::page_script()),
            ("wisPeek", crate::chrome::peek::page_script()),
            ("wisKeybind", &keybind_script),
            ("wisSignup", crate::vault::signup::page_script()),
            ("wisAddress", crate::vault::address_field::page_script()),
            ("wisPageMenu", &page_menu),
        ];
        if !ephemeral {
            isolated.push(("wisCapture", crate::vault::capture::capture_script()));
        }
        for (name, source) in isolated {
            register_isolated(&ucm, name, &label);
            content_world::install_script(&wv, &ucm, &label, name, source, true);
        }
    });
}

/// Register only the two open-in-tab channels on a widget panel webview, so
/// the panel gains none of the tab-only channels.
pub fn install_link_open_only<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
    let label = webview.label().to_string();
    let _ = webview.with_webview(move |w| {
        let wv = w.inner();
        let Some(ucm) = wv.user_content_manager() else {
            log::warn!("gtk_dispatch: panel webview has no user content manager");
            return;
        };

        for (name, source) in [
            (
                crate::pages::link_open::HANDLER_BACKGROUND,
                crate::pages::link_open::page_script(),
            ),
            (
                crate::pages::link_open::HANDLER_FOREGROUND,
                crate::pages::link_open::panel_page_script(),
            ),
        ] {
            register_isolated(&ucm, name, &label);
            content_world::install_script(&wv, &ucm, &label, name, source, true);
        }
    });
}

/// Register only the `wisAiFetch` channel on a hidden AI fetch webview and
/// route it to the fetch dispatcher. WebKitGTK script messages also carry
/// no frame identity.
pub fn install_ai_fetch_only<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
    let label = webview.label().to_string();
    let _ = webview.with_webview(move |w| {
        let wv = w.inner();
        let Some(ucm) = wv.user_content_manager() else {
            log::warn!("gtk_dispatch: fetch webview has no user content manager");
            return;
        };
        if !ucm.register_script_message_handler("wisAiFetch") {
            log::warn!("gtk_dispatch: failed to register handler wisAiFetch");
            return;
        }

        ucm.connect_script_message_received(Some("wisAiFetch"), move |_ucm, result| {
            if let Some(body) = decode(result) {
                let str_field = |key: &str| {
                    body.get(key)
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string()
                };

                crate::ai::fetch::dispatch_message(&label, str_field("text"), str_field("title"));
            }
        });
    });
}

/// Register one named handler and connect its `script-message-received::<name>`
/// signal, routing the decoded body to [`route`]. The connection lives on the
/// `UserContentManager` (owned by the webview), so it is dropped with the tab
/// and its `SignalHandlerId` is not tracked.
fn register(ucm: &UserContentManager, name: &'static str, label: &str) {
    if !ucm.register_script_message_handler(name) {
        log::warn!("gtk_dispatch: failed to register handler {name}");
        return;
    }
    connect(ucm, name, label);
}

/// [`register`], for a handler only scripts in Wis's own world can post to.
fn register_isolated(ucm: &UserContentManager, name: &'static str, label: &str) {
    if !content_world::add_handler(ucm, name) {
        log::warn!("gtk_dispatch: failed to register handler {name}");
        return;
    }
    connect(ucm, name, label);
}

fn connect(ucm: &UserContentManager, name: &'static str, label: &str) {
    let label = label.to_string();
    ucm.connect_script_message_received(Some(name), move |_ucm, result| {
        if let Some(body) = decode(result) {
            route(name, &label, &body);
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
/// [`crate::platform::win_dispatch`].
fn route(name: &str, label: &str, body: &serde_json::Value) {
    let str_field = |key: &str| {
        body.get(key)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    };

    match name {
        "wisOpenInTab" | "wisOpenInTabFg" => {
            if let Some(href) = body.as_str() {
                crate::pages::link_open::dispatch_message(name, href);
            }
        }
        "wisNavGuard" => {
            let new_tab = body
                .get("newTab")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            crate::protection::navguard::dispatch_message(
                label,
                str_field("href"),
                str_field("text"),
                new_tab,
            );
        }
        "wisPeek" => {
            let x = body.get("x").and_then(|v| v.as_f64()).unwrap_or(-1.0);
            let y = body.get("y").and_then(|v| v.as_f64()).unwrap_or(-1.0);
            crate::chrome::peek::dispatch_message(str_field("href"), str_field("text"), x, y);
        }
        "wisMediaPermission" => {
            crate::protection::permissions::dispatch_message(
                label,
                &str_field("id"),
                &str_field("origin"),
                &str_field("kind"),
            );
        }
        "wisCapture" => {
            crate::vault::capture::dispatch_message(
                label,
                str_field("username"),
                str_field("password"),
            );
        }
        "wisPageText" => {
            crate::pages::text::dispatch_message(label, str_field("text"), str_field("title"));
        }
        "wisSignup" => {
            if let Some(parsed) = crate::platform::page_message::json_body(body) {
                crate::vault::signup::dispatch_message(label, &parsed);
            }
        }
        "wisAddress" => {
            if let Some(parsed) = crate::platform::page_message::json_body(body) {
                crate::vault::address_field::dispatch_message(label, &parsed);
            }
        }
        "wisPageSource" => {
            if let Some(parsed) = crate::platform::page_message::json_body(body) {
                crate::pages::source::dispatch_message(label, &parsed);
            }
        }
        "wisPageMenu" => {
            if let Some(parsed) = crate::platform::page_message::json_body(body) {
                crate::pages::context_menu::dispatch_message(label, &parsed);
            }
        }
        "wisKeybind" => crate::chrome::keybind::dispatch_action(label, body),
        "wisCosmetic" => crate::protection::cosmetic::dispatch_cosmetic(label, body),
        "wisZap" => crate::protection::cosmetic::dispatch_zap(label, str_field("selector")),
        other => log::debug!("gtk_dispatch: handler {other} not yet wired"),
    }
}
