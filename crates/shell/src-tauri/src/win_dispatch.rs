//! Single-channel web-message dispatcher for Windows (WebView2).
//!
//! WKWebView gives each feature its own named `WKScriptMessageHandler`
//! (`wisPeek`, `wisOpenInTab`, ...) reachable as
//! `window.webkit.messageHandlers.<name>.postMessage`. WebView2 has only one
//! channel, `window.chrome.webview.postMessage`, surfaced as a single
//! `add_WebMessageReceived` event. So the page scripts can stay identical across
//! platforms, [`SHIM_SCRIPT`] (a document-start init script, Windows only)
//! recreates `window.webkit.messageHandlers` with each named `postMessage`
//! forwarding a `{ handler, body }` envelope over the one channel, and [`install`]
//! registers the one receiver that parses the envelope and routes `body` to the
//! matching feature, mirroring what that feature's macOS handler does.
//!
//! The per-feature `install` functions are macOS-only (no-ops on Windows); this
//! module is their Windows replacement, wired once per tab webview.
//!
//! WebView2 has no isolated script world, so the channels whose message makes
//! the browser act are kept from the page another way: their scripts post
//! through [`private_script`]'s sender with a per-webview token, the shim does
//! not publish them, and the receiver drops their messages without the token.

use std::collections::HashMap;
use std::sync::Mutex;

use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2WebMessageReceivedEventArgs,
};
use webview2_com::{CoTaskMemPWSTR, WebMessageReceivedEventHandler};

/// The channels whose message makes the browser act.
const ACTING: &[&str] = &[
    "wisOpenInTab",
    "wisOpenInTabFg",
    "wisNavGuard",
    "wisPeek",
    "wisCapture",
    "wisPopout",
    "wisKeybind",
    "wisPageMenu",
    "wisSignup",
];

/// Each webview's token, by label.
static TOKENS: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

/// The token webview `label` posts its acting channels with.
pub fn token(label: &str) -> String {
    TOKENS
        .lock()
        .expect("win_dispatch tokens poisoned")
        .get_or_insert_with(HashMap::new)
        .entry(label.to_string())
        .or_insert_with(|| uuid::Uuid::new_v4().simple().to_string())
        .clone()
}

pub fn forget(label: &str) {
    if let Some(tokens) = TOKENS
        .lock()
        .expect("win_dispatch tokens poisoned")
        .as_mut()
    {
        tokens.remove(label);
    }
}

/// `source` with its acting channels behind a sender only it holds. The sender
/// adds `token` by string concatenation alone, with `postMessage` and
/// `JSON.stringify` taken before the page runs, so nothing the page could
/// replace ever sees it. Each of `frozen`, a one-argument function Rust calls,
/// is fixed in place and then only answers a call that brings the token.
pub fn private_script(token: &str, source: &str, frozen: &[&str]) -> String {
    let channels = ACTING
        .iter()
        .map(|name| format!("{name}: channel('{name}')"))
        .collect::<Vec<_>>()
        .join(", ");
    let source = source.replace("window.webkit", "webkit");
    let freeze: String = frozen
        .iter()
        .map(|name| format!("  freeze('{name}');\n"))
        .collect();
    format!(
        r#"(function () {{
  var wv = window.chrome && window.chrome.webview;
  if (!wv || !wv.postMessage) return;
  var send = wv.postMessage.bind(wv);
  var json = JSON.stringify;
  var token = '{token}';
  function channel(name) {{
    var head = '{{"handler":"' + name + '","token":"' + token + '","body":';
    return {{
      postMessage: function (body) {{
        try {{ send(head + json(body === undefined ? null : body) + '}}'); }} catch (e) {{}}
      }}
    }};
  }}
  function freeze(name) {{
    var fn = window[name];
    if (typeof fn !== 'function') return;
    try {{
      Object.defineProperty(window, name, {{
        value: function (arg, key) {{ if (key === token) fn(arg); }},
        writable: false,
        configurable: false
      }});
    }} catch (e) {{}}
  }}
  var webkit = {{ messageHandlers: {{ {channels} }} }};
{source}
{freeze}}})();"#
    )
}

/// Whether a message may be acted on: an acting channel needs the token.
fn authorised(handler: &str, sent: Option<&str>, token: &str) -> bool {
    !ACTING.contains(&handler) || sent == Some(token)
}

/// Document-start shim (Windows): rebuild `window.webkit.messageHandlers` so the
/// shared page scripts post over WebView2's single channel unchanged. Each named
/// handler forwards a `{ handler, body }` JSON string; listeners read the channel
/// at click time, so this runs at document start alongside the feature scripts.
pub const SHIM_SCRIPT: &str = r#"(function () {
  if (!window.chrome || !window.chrome.webview) return;
  var names = ['wisCosmetic', 'wisZap', 'wisMediaPermission', 'wisFindResult',
               'wisPageText', 'wisPageSource'];
  function forward(name, body) {
    try {
      window.chrome.webview.postMessage(
        JSON.stringify({ handler: name, body: body === undefined ? null : body })
      );
    } catch (e) {}
  }
  window.webkit = window.webkit || {};
  var mh = window.webkit.messageHandlers = window.webkit.messageHandlers || {};
  names.forEach(function (name) {
    if (!mh[name]) {
      mh[name] = { postMessage: function (body) { forward(name, body); } };
    }
  });
})();"#;

/// Document-start shim for the hidden AI fetch webview (Windows): the page is
/// untrusted, so only the `wisAiFetch` channel exists there instead of the
/// full tab shim.
pub const AI_FETCH_SHIM: &str = r#"(function () {
  if (!window.chrome || !window.chrome.webview) return;
  window.webkit = window.webkit || {};
  var mh = window.webkit.messageHandlers = window.webkit.messageHandlers || {};
  if (!mh.wisAiFetch) {
    mh.wisAiFetch = { postMessage: function (body) {
      try {
        window.chrome.webview.postMessage(
          JSON.stringify({ handler: 'wisAiFetch', body: body === undefined ? null : body })
        );
      } catch (e) {}
    } };
  }
})();"#;

#[derive(serde::Deserialize)]
struct Envelope {
    handler: String,
    #[serde(default)]
    body: serde_json::Value,
    #[serde(default)]
    token: Option<String>,
}

/// Install the single web-message receiver on a tab webview. The shim
/// ([`SHIM_SCRIPT`]) rides the builder as an init script; this registers the one
/// event that routes every forwarded message to its feature. `ephemeral` gates
/// the save-password channel off, matching macOS not installing it on private
/// tabs.
pub fn install<R: tauri::Runtime>(
    webview: &tauri::webview::Webview<R>,
    ephemeral: bool,
    cosmetic_enabled: bool,
) {
    let label = webview.label().to_string();
    let token = token(&label);
    crate::win_webview2::with_core_webview2(webview, move |core| {
        let handler = WebMessageReceivedEventHandler::create(Box::new(
            move |sender: Option<ICoreWebView2>,
                  args: Option<ICoreWebView2WebMessageReceivedEventArgs>|
                  -> windows::core::Result<()> {
                if let Some(args) = args {
                    let mut msg = windows::core::PWSTR::null();
                    // SAFETY: `args` is live for the callback; `CoTaskMemPWSTR`
                    // takes ownership of the returned buffer and frees it.
                    if unsafe { args.TryGetWebMessageAsString(&mut msg) }.is_ok() {
                        let top_frame = is_top_frame(sender.as_ref(), &args);
                        dispatch(
                            &label,
                            &token,
                            ephemeral,
                            cosmetic_enabled,
                            top_frame,
                            &CoTaskMemPWSTR::from(msg).to_string(),
                        );
                    }
                }

                Ok(())
            },
        ));
        let mut token = 0_i64;
        // SAFETY: runs on the UI thread via `with_core_webview2`.
        let _ = unsafe { core.add_WebMessageReceived(&handler, &mut token) };
    });
}

/// Install a find-scoped web-message receiver on the peek preview webview.
/// The preview is untrusted, so only the `wisFindResult` channel is routed:
/// the page gains none of the tab-only channels (`wisOpenInTab`, `wisPeek`,
/// `wisKeybind`, ...) the full dispatcher would expose.
pub fn install_find_only<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
    let label = webview.label().to_string();
    crate::win_webview2::with_core_webview2(webview, move |core| {
        let handler = WebMessageReceivedEventHandler::create(Box::new(
            move |_sender: Option<ICoreWebView2>,
                  args: Option<ICoreWebView2WebMessageReceivedEventArgs>|
                  -> windows::core::Result<()> {
                if let Some(args) = args {
                    let mut msg = windows::core::PWSTR::null();
                    // SAFETY: `args` is live for the callback; `CoTaskMemPWSTR`
                    // takes ownership of the returned buffer and frees it.
                    if unsafe { args.TryGetWebMessageAsString(&mut msg) }.is_ok() {
                        let raw = CoTaskMemPWSTR::from(msg).to_string();
                        if let Ok(env) = serde_json::from_str::<Envelope>(&raw) {
                            if env.handler == "wisFindResult" {
                                crate::find::dispatch_message(&label, &env.body);
                            }
                        }
                    }
                }

                Ok(())
            },
        ));

        let mut token = 0_i64;
        // SAFETY: runs on the UI thread via `with_core_webview2`.
        let _ = unsafe { core.add_WebMessageReceived(&handler, &mut token) };
    });
}

/// Install a link-scoped web-message receiver on a widget panel webview. Only
/// the two open-in-tab channels are routed, so the panel gains none of the
/// tab-only channels; the foreground channel is top frame only, since it moves
/// focus.
pub fn install_link_open_only<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
    let token = token(webview.label());
    crate::win_webview2::with_core_webview2(webview, move |core| {
        let handler = WebMessageReceivedEventHandler::create(Box::new(
            move |sender: Option<ICoreWebView2>,
                  args: Option<ICoreWebView2WebMessageReceivedEventArgs>|
                  -> windows::core::Result<()> {
                if let Some(args) = args {
                    let mut msg = windows::core::PWSTR::null();
                    // SAFETY: `args` is live for the callback; `CoTaskMemPWSTR`
                    // takes ownership of the returned buffer and frees it.
                    if unsafe { args.TryGetWebMessageAsString(&mut msg) }.is_ok() {
                        let raw = CoTaskMemPWSTR::from(msg).to_string();
                        if let Some(env) = serde_json::from_str::<Envelope>(&raw)
                            .ok()
                            .filter(|env| authorised(&env.handler, env.token.as_deref(), &token))
                        {
                            let background = env.handler == crate::link_open::HANDLER_BACKGROUND;
                            let foreground = env.handler == crate::link_open::HANDLER_FOREGROUND
                                && is_top_frame(sender.as_ref(), &args);

                            if background || foreground {
                                if let Some(href) = env.body.as_str() {
                                    crate::link_open::dispatch_message(&env.handler, href);
                                }
                            }
                        }
                    }
                }

                Ok(())
            },
        ));

        let mut token = 0_i64;
        // SAFETY: runs on the UI thread via `with_core_webview2`.
        let _ = unsafe { core.add_WebMessageReceived(&handler, &mut token) };
    });
}

/// Install a fetch-scoped web-message receiver on a hidden AI fetch webview.
/// Only the `wisAiFetch` channel is routed, top frame only: the fetched page
/// gains none of the tab channels, and a subframe cannot put its own text
/// forward under the top document's URL.
pub fn install_ai_fetch_only<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
    let label = webview.label().to_string();
    crate::win_webview2::with_core_webview2(webview, move |core| {
        let handler = WebMessageReceivedEventHandler::create(Box::new(
            move |sender: Option<ICoreWebView2>,
                  args: Option<ICoreWebView2WebMessageReceivedEventArgs>|
                  -> windows::core::Result<()> {
                if let Some(args) = args {
                    let mut msg = windows::core::PWSTR::null();
                    // SAFETY: `args` is live for the callback; `CoTaskMemPWSTR`
                    // takes ownership of the returned buffer and frees it.
                    if unsafe { args.TryGetWebMessageAsString(&mut msg) }.is_ok()
                        && is_top_frame(sender.as_ref(), &args)
                    {
                        let raw = CoTaskMemPWSTR::from(msg).to_string();
                        if let Ok(env) = serde_json::from_str::<Envelope>(&raw) {
                            if env.handler == "wisAiFetch" {
                                let field = |key: &str| {
                                    env.body
                                        .get(key)
                                        .and_then(|v| v.as_str())
                                        .unwrap_or_default()
                                        .to_string()
                                };
                                crate::ai_fetch::dispatch_message(
                                    &label,
                                    field("text"),
                                    field("title"),
                                );
                            }
                        }
                    }
                }

                Ok(())
            },
        ));

        let mut token = 0_i64;
        // SAFETY: runs on the UI thread via `with_core_webview2`.
        let _ = unsafe { core.add_WebMessageReceived(&handler, &mut token) };
    });
}

/// True when the message came from the top-level document, i.e. its source URI
/// equals the webview's top-level URI. WebView2 runs the page scripts in every
/// subframe (wry can't restrict to the main frame), so cosmetic/zap messages
/// from cross-origin iframes are filtered out here: the dispatcher can only eval
/// a reply into the main frame, and a zap must bind to the real tab host.
fn is_top_frame(
    sender: Option<&ICoreWebView2>,
    args: &ICoreWebView2WebMessageReceivedEventArgs,
) -> bool {
    let Some(sender) = sender else {
        return false;
    };

    // SAFETY: both getters write a COM-allocated string owned by `CoTaskMemPWSTR`.
    unsafe {
        let mut frame = windows::core::PWSTR::null();
        let mut top = windows::core::PWSTR::null();
        if args.Source(&mut frame).is_err() || sender.Source(&mut top).is_err() {
            return false;
        }

        CoTaskMemPWSTR::from(frame).to_string() == CoTaskMemPWSTR::from(top).to_string()
    }
}

/// Route one received envelope to the feature named by its discriminator. The
/// webview `label` (`tab-<uuid>`) identifies the source tab for handlers that
/// need it.
fn dispatch(
    label: &str,
    token: &str,
    ephemeral: bool,
    cosmetic_enabled: bool,
    top_frame: bool,
    text: &str,
) {
    let env: Envelope = match serde_json::from_str(text) {
        Ok(env) => env,
        Err(err) => {
            log::warn!("win_dispatch: undecodable message: {err}");
            return;
        }
    };
    if !authorised(&env.handler, env.token.as_deref(), token) {
        log::debug!("win_dispatch: dropped {} without its token", env.handler);
        return;
    }

    let str_field = |key: &str| {
        env.body
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    };

    match env.handler.as_str() {
        "wisOpenInTab" => {
            if let Some(href) = env.body.as_str() {
                crate::link_open::dispatch_message(&env.handler, href);
            }
        }
        "wisNavGuard" if top_frame => {
            let new_tab = env
                .body
                .get("newTab")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            crate::navguard::dispatch_message(label, str_field("href"), str_field("text"), new_tab);
        }
        "wisPeek" => {
            let x = env.body.get("x").and_then(|v| v.as_f64()).unwrap_or(-1.0);
            let y = env.body.get("y").and_then(|v| v.as_f64()).unwrap_or(-1.0);
            crate::peek::dispatch_message(str_field("href"), str_field("text"), x, y);
        }
        "wisMediaPermission" => {
            crate::permissions::dispatch_message(
                label,
                &str_field("id"),
                &str_field("origin"),
                &str_field("kind"),
            );
        }
        "wisCapture" if top_frame => {
            if !ephemeral {
                crate::capture::dispatch_message(
                    label,
                    str_field("username"),
                    str_field("password"),
                );
            }
        }
        "wisPageText" => {
            crate::page_text::dispatch_message(label, str_field("text"), str_field("title"));
        }
        "wisSignup" if top_frame => {
            if let Some(parsed) = crate::page_message::json_body(&env.body) {
                crate::signup::dispatch_message(label, &parsed);
            }
        }
        "wisPageSource" if top_frame => {
            if let Some(parsed) = crate::page_message::json_body(&env.body) {
                crate::page_source::dispatch_message(label, &parsed);
            }
        }
        "wisPageMenu" if top_frame => {
            if let Some(parsed) = crate::page_message::json_body(&env.body) {
                crate::page_menu::dispatch_message(label, &parsed);
            }
        }
        // Cosmetic + zap are top-frame only (an iframe's reply can't be targeted,
        // and a zap must bind to the real tab host).
        "wisCosmetic" => {
            if cosmetic_enabled && top_frame {
                crate::cosmetic::dispatch_cosmetic(label, &env.body);
            }
        }
        "wisZap" => {
            if cosmetic_enabled && top_frame {
                crate::cosmetic::dispatch_zap(label, str_field("selector"));
            }
        }
        "wisFindResult" => crate::find::dispatch_message(label, &env.body),
        "wisKeybind" => crate::keybind::dispatch_action(label, &env.body),
        other => {
            // wisPopout: deferred to the Windows runner (reparent plumbing).
            log::debug!("win_dispatch: handler {other} not yet wired");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_acting_channel_needs_the_webviews_token() {
        assert!(authorised("wisOpenInTab", Some("t1"), "t1"));
        assert!(!authorised("wisOpenInTab", None, "t1"));
        assert!(!authorised("wisKeybind", Some("guess"), "t1"));
    }

    #[test]
    fn page_world_channels_need_none() {
        assert!(authorised("wisCosmetic", None, "t1"));
        assert!(authorised("wisFindResult", None, "t1"));
    }

    #[test]
    fn a_wrapped_script_posts_only_through_its_own_sender() {
        let wrapped = private_script(
            "t1",
            "window.webkit.messageHandlers.wisPeek.postMessage(1);",
            &["__wisKeybindUpdate"],
        );
        assert!(!wrapped.contains("window.webkit"));
        assert!(wrapped.contains("webkit.messageHandlers.wisPeek.postMessage(1);"));
        assert!(wrapped.contains("var token = 't1';"));
        assert!(wrapped.contains("freeze('__wisKeybindUpdate');"));
    }

    #[test]
    fn a_label_keeps_its_token_until_forgotten() {
        let first = token("tab-test-token");
        assert_eq!(token("tab-test-token"), first);
        forget("tab-test-token");
        assert_ne!(token("tab-test-token"), first);
        forget("tab-test-token");
    }

    #[test]
    fn the_shim_no_longer_publishes_an_acting_channel() {
        for name in ACTING {
            assert!(!SHIM_SCRIPT.contains(&format!("'{name}'")), "{name}");
        }
    }
}
