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

use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2WebMessageReceivedEventArgs,
};
use webview2_com::{CoTaskMemPWSTR, WebMessageReceivedEventHandler};

/// Document-start shim (Windows): rebuild `window.webkit.messageHandlers` so the
/// shared page scripts post over WebView2's single channel unchanged. Each named
/// handler forwards a `{ handler, body }` JSON string; listeners read the channel
/// at click time, so this runs at document start alongside the feature scripts.
pub const SHIM_SCRIPT: &str = r#"(function () {
  if (!window.chrome || !window.chrome.webview) return;
  var names = ['wisOpenInTab', 'wisNavGuard', 'wisPeek', 'wisCosmetic',
               'wisZap', 'wisMediaPermission', 'wisCapture', 'wisPopout', 'wisFindResult',
               'wisKeybind', 'wisPageText'];
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

#[derive(serde::Deserialize)]
struct Envelope {
    handler: String,
    #[serde(default)]
    body: serde_json::Value,
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
fn dispatch(label: &str, ephemeral: bool, cosmetic_enabled: bool, top_frame: bool, text: &str) {
    let env: Envelope = match serde_json::from_str(text) {
        Ok(env) => env,
        Err(err) => {
            log::warn!("win_dispatch: undecodable message: {err}");
            return;
        }
    };

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
                crate::link_open::dispatch_message(href);
            }
        }
        "wisNavGuard" => {
            crate::navguard::dispatch_message(str_field("href"), str_field("text"));
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
        "wisCapture" => {
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
