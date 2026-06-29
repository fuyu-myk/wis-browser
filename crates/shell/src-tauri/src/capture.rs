//! "Save password?" capture.
//!
//! Security spine: external page webviews are not in any Tauri capability, so a
//! page can never call a vault command. To learn that a login was submitted,
//! a *narrow*, dedicated native channel is used, `WKScriptMessageHandler`
//! (message name `wisCapture`), wired only to forward `{username, password}` to
//! Rust. It is never the Tauri command IPC.
//!
//! Trust model:
//! - The **host** is taken from the WebView's own URL natively, never from the
//!   page's JS, so a page can only ever offer to save a login for its *own*
//!   origin (no cross-origin poisoning).
//! - The plaintext password is held only in [`AppState::pending_capture`] and
//!   never crosses back out to any webview: the consent event carries just
//!   `{host, username}`. Saving happens entirely backend-side via
//!   [`capture_save`]. Secrets are never logged.
//! - Private/ephemeral tabs never install the handler (see
//!   `create_tab_webview`), so nothing is captured while browsing privately.

use std::sync::OnceLock;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use wis_core::{WisError, WisResult};
use zeroize::Zeroizing;

use crate::state::AppState;

/// Tauri event emitted to the chrome to offer saving a captured login.
const CAPTURE_EVENT: &str = "wis-capture-login";

/// A login captured from a page, awaiting the user's save/dismiss decision.
/// The password is zeroized on drop.
pub struct PendingCapture {
    pub host: String,
    pub username: String,
    pub password: Zeroizing<String>,
}

#[derive(Debug, Serialize, Clone)]
struct CaptureEvent {
    host: String,
    username: String,
}

/// The pending capture's display fields (no password), for a surface that
/// mounts after the event fired.
#[derive(Debug, Serialize, Clone)]
pub struct CaptureInfo {
    host: String,
    username: String,
}

/// App handle for the native message handler to reach the async runtime.
static APP: OnceLock<AppHandle> = OnceLock::new();

/// Record the app handle once, during setup, so the WKScriptMessageHandler can
/// forward captures into the async runtime.
pub fn set_app_handle(app: AppHandle) {
    let _ = APP.set(app);
}

/// Forward a captured login from a parsed `{ username, password }` payload (the
/// Windows dispatcher's entry; macOS reaches `intake` through its `wisCapture`
/// script-message handler). The **host is read from the webview's own URL**,
/// never the page, so a page can only ever offer to save a login for its own
/// origin. The dispatcher gates this to non-ephemeral tabs.
#[cfg(windows)]
pub(crate) fn dispatch_message(label: &str, username: String, password: String) {
    if password.is_empty() {
        return;
    }
    let Some(app) = APP.get() else {
        return;
    };
    let Some(webview) = app.webviews().get(label).cloned() else {
        return;
    };
    let Some(host) = webview
        .url()
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
    else {
        return;
    };

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        intake(app, host, username, password).await;
    });
}

fn strip_www(host: &str) -> &str {
    host.strip_prefix("www.").unwrap_or(host)
}

/// Dedup, stash the pending capture, and emit the consent event. Skips logins
/// whose `(host, username)` is already saved so the user isn't nagged.
async fn intake(app: AppHandle, host: String, username: String, password: String) {
    let host = strip_www(&host).to_string();
    let state = app.state::<AppState>();

    let storage = state.storage.clone();
    let lookup_host = host.clone();
    let existing =
        tokio::task::spawn_blocking(move || storage.list_credentials(Some(&lookup_host)))
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default();
    if existing.iter().any(|c| c.username == username) {
        return;
    }

    {
        let mut pending = state.pending_capture.lock().await;
        *pending = Some(PendingCapture {
            host: host.clone(),
            username: username.clone(),
            password: Zeroizing::new(password),
        });
    }

    let _ = app.emit(CAPTURE_EVENT, CaptureEvent { host, username });
}

/// Encrypt and save the pending captured login. Requires the vault to be
/// unlocked (the chrome unlocks via Touch ID first); leaves the pending capture
/// in place if locked so the prompt can be retried.
#[tauri::command]
pub async fn capture_save(state: State<'_, AppState>) -> WisResult<()> {
    if state.vault_key.lock().await.is_none() {
        return Err(locked());
    }

    let pending = match state.pending_capture.lock().await.take() {
        Some(p) => p,
        None => return Ok(()),
    };

    let blob = {
        let guard = state.vault_key.lock().await;
        let key = guard.as_ref().ok_or_else(locked)?;
        wis_secrets::encrypt(key, pending.password.as_bytes()).map_err(|err| {
            WisError::InvalidInput {
                message: format!("vault: {err}"),
            }
        })?
    };

    let storage = state.storage.clone();
    let host = pending.host.clone();
    let username = pending.username.clone();

    tokio::task::spawn_blocking(move || {
        storage
            .save_credential(&host, &username, &blob.ciphertext, &blob.nonce)
            .map(|_| ())
    })
    .await
    .map_err(|err| WisError::PersistenceFailed {
        reason: format!("capture save task panicked: {err}"),
    })?
    .map_err(|err| WisError::PersistenceFailed {
        reason: err.to_string(),
    })
}

/// Discard the pending captured login (user clicked "Not now").
#[tauri::command]
pub async fn capture_dismiss(state: State<'_, AppState>) -> WisResult<()> {
    *state.pending_capture.lock().await = None;
    Ok(())
}

/// The pending capture (host + username, never the password), so a surface that
/// mounts after the `wis-capture-login` event can still render it.
#[tauri::command]
pub async fn capture_pending(state: State<'_, AppState>) -> WisResult<Option<CaptureInfo>> {
    let guard = state.pending_capture.lock().await;
    Ok(guard.as_ref().map(|p| CaptureInfo {
        host: p.host.clone(),
        username: p.username.clone(),
    }))
}

fn locked() -> WisError {
    WisError::InvalidInput {
        message: "vault is locked".to_string(),
    }
}

/// Injected page script: forward a submitted login (password + best-effort
/// username) to the native handler. Modern sign-ins (Gmail, Twitch, Canvas, most
/// SPAs) don't fire a native form `submit`. So several signals are watched: form
/// submit, "Enter" in a password field, a click on a login/submit control, page
/// teardown, and the filled password field disappearing from the DOM (modal close
/// / view swap, via a MutationObserver). Field discovery descends into open shadow
/// roots and event handlers use `composedPath()` (the event target is retargeted to
/// the shadow host otherwise). The last typed password/username are remembered as a
/// fallback. A per-page dedup avoids re-posting the same credentials. No-ops when
/// the handler isn't present (private tabs).
#[cfg(any(target_os = "macos", windows))]
const CAPTURE_JS: &str = r#"(function () {
  if (window.__wisCaptureInstalled) return;
  window.__wisCaptureInstalled = true;

  var lastSent = '';
  var lastPassword = '';
  var lastUsername = '';
  var sawPassword = false;

  function send(username, password) {
    if (!password) return;
    var key = username + '\n' + password;
    if (key === lastSent) return;
    lastSent = key;
    try {
      if (window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.wisCapture) {
        window.webkit.messageHandlers.wisCapture.postMessage({ username: username, password: password });
      }
    } catch (e) {}
  }

  // Collect matching elements across the document.
  function deepQuery(selector, root, out) {
    root = root || document;
    out = out || [];
    var nodes;
    try { nodes = root.querySelectorAll(selector); } catch (e) { nodes = []; }
    for (var i = 0; i < nodes.length; i++) out.push(nodes[i]);
    var hosts;
    try { hosts = root.querySelectorAll('*'); } catch (e) { hosts = []; }
    for (var j = 0; j < hosts.length; j++) {
      if (hosts[j].shadowRoot) deepQuery(selector, hosts[j].shadowRoot, out);
    }
    return out;
  }

  function findPassword() {
    var pwds = deepQuery('input[type=password]');
    for (var i = 0; i < pwds.length; i++) {
      if (pwds[i].value) return pwds[i];
    }
    return null;
  }

  function anyPasswordEl() {
    return deepQuery('input[type=password]').length > 0;
  }

  function looksLikeUser(el) {
    var hay = ((el.name || '') + ' ' + (el.id || '') + ' ' + (el.autocomplete || '') +
      ' ' + ((el.getAttribute && el.getAttribute('aria-label')) || '')).toLowerCase();
    return /user|email|login|account|phone|tel/.test(hay);
  }

  function findUsername() {
    var inputs = deepQuery('input');
    for (var i = 0; i < inputs.length; i++) {
      var el = inputs[i];
      var t = (el.type || 'text').toLowerCase();
      if (t === 'password' || t === 'hidden') continue;
      if (el.value && (t === 'email' || looksLikeUser(el))) return el.value;
    }
    for (var k = 0; k < inputs.length; k++) {
      var v = inputs[k].value || '';
      if (/^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(v)) return v;
    }
    for (var m = 0; m < inputs.length; m++) {
      var tt = (inputs[m].type || 'text').toLowerCase();
      if ((tt === 'text' || tt === 'email' || tt === 'tel') && inputs[m].value) return inputs[m].value;
    }
    return lastUsername;
  }

  function capture() {
    var pwd = findPassword();
    var password = (pwd && pwd.value) || lastPassword;
    if (!password) return;
    send(findUsername(), password);
  }

  function isSubmitLike(el) {
    if (!el || !el.tagName) return false;
    var tag = el.tagName.toLowerCase();
    var type = (el.getAttribute && (el.getAttribute('type') || '')).toLowerCase();
    var role = (el.getAttribute && (el.getAttribute('role') || '')).toLowerCase();
    if (tag === 'input' && (type === 'submit' || type === 'button')) return true;
    if (tag !== 'button' && tag !== 'a' && role !== 'button') return false;
    if (type === 'submit') return true;
    var label = (((el.textContent || '') + ' ' +
      ((el.getAttribute && el.getAttribute('aria-label')) || '') + ' ' +
      ((el.getAttribute && el.getAttribute('data-a-target')) || '') + ' ' +
      (el.value || ''))).toLowerCase();
    return /log\s?in|sign\s?in|sign\s?on|log\s?on|continue|submit|next|done|confirm/.test(label);
  }

  // Remember the latest password/username as they're typed.
  document.addEventListener('input', function (e) {
    var path = (e.composedPath && e.composedPath()) || [e.target];
    for (var i = 0; i < path.length; i++) {
      var el = path[i];
      if (!el || el.tagName !== 'INPUT') continue;
      var t = (el.type || 'text').toLowerCase();
      if (t === 'password') {
        if (el.value) { lastPassword = el.value; sawPassword = true; }
      } else if ((t === 'email' || t === 'text' || t === 'tel') && el.value && looksLikeUser(el)) {
        lastUsername = el.value;
      }
      return;
    }
  }, true);

  document.addEventListener('submit', function () { capture(); }, true);

  document.addEventListener('keydown', function (e) {
    if (e.key !== 'Enter') return;
    var path = (e.composedPath && e.composedPath()) || [e.target];
    for (var i = 0; i < path.length; i++) {
      var el = path[i];
      if (el && el.tagName === 'INPUT' && (el.type || '').toLowerCase() === 'password' && el.value) {
        capture();
        return;
      }
    }
  }, true);

  document.addEventListener('click', function (e) {
    var path = (e.composedPath && e.composedPath()) || [e.target];
    for (var i = 0; i < path.length; i++) {
      var el = path[i];
      if (el === document || el === window) break;
      if (isSubmitLike(el)) {
        if (findPassword() || lastPassword) capture();
        return;
      }
    }
  }, true);

  window.addEventListener('pagehide', function () { capture(); }, true);

  var checkScheduled = false;
  function scheduleDisappearCheck() {
    if (checkScheduled || !sawPassword) return;
    checkScheduled = true;
    setTimeout(function () {
      checkScheduled = false;
      if (sawPassword && !anyPasswordEl()) {
        sawPassword = false;
        capture();
      }
    }, 200);
  }
  try {
    var mo = new MutationObserver(scheduleDisappearCheck);
    mo.observe(document.documentElement, { childList: true, subtree: true });
  } catch (e) {}
})();"#;

/// The capture page script, exposed so the Windows tab builder can inject it as
/// a document-start init script.
#[cfg(windows)]
pub fn capture_script() -> &'static str {
    CAPTURE_JS
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;

    use objc2::rc::Id;
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{
        class, declare_class, msg_send, msg_send_id, mutability, ClassType, DeclaredClass,
    };

    use super::{intake, APP, CAPTURE_JS};

    declare_class!(
        struct CaptureHandler;

        unsafe impl ClassType for CaptureHandler {
            type Super = NSObject;
            type Mutability = mutability::InteriorMutable;
            const NAME: &'static str = "WisCaptureHandler";
        }

        impl DeclaredClass for CaptureHandler {
            type Ivars = ();
        }

        unsafe impl CaptureHandler {
            #[method(userContentController:didReceiveScriptMessage:)]
            fn did_receive(&self, _ucc: *mut AnyObject, message: *mut AnyObject) {
                unsafe { handle_message(message) }
            }
        }
    );

    impl CaptureHandler {
        fn new() -> Id<Self> {
            unsafe { msg_send_id![Self::alloc(), init] }
        }
    }

    unsafe fn ns_string(s: &str) -> *mut AnyObject {
        let c = CString::new(s).unwrap_or_default();
        msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()]
    }

    unsafe fn nsstring_to_string(ns: *mut AnyObject) -> Option<String> {
        if ns.is_null() {
            return None;
        }

        let is_str: bool = msg_send![ns, isKindOfClass: class!(NSString)];
        if !is_str {
            return None;
        }

        let utf8: *const c_char = msg_send![ns, UTF8String];
        if utf8.is_null() {
            return None;
        }

        Some(CStr::from_ptr(utf8).to_string_lossy().into_owned())
    }

    unsafe fn dict_string(dict: *mut AnyObject, key: &str) -> Option<String> {
        let k = ns_string(key);
        let v: *mut AnyObject = msg_send![dict, objectForKey: k];
        nsstring_to_string(v)
    }

    unsafe fn handle_message(message: *mut AnyObject) {
        if message.is_null() {
            return;
        }

        let webview: *mut AnyObject = msg_send![message, webView];
        if webview.is_null() {
            return;
        }
        let url: *mut AnyObject = msg_send![webview, URL];
        if url.is_null() {
            return;
        }
        let host_ns: *mut AnyObject = msg_send![url, host];
        let Some(host) = nsstring_to_string(host_ns) else {
            return;
        };

        // Body must be a dictionary.
        let body: *mut AnyObject = msg_send![message, body];
        if body.is_null() {
            return;
        }
        let is_dict: bool = msg_send![body, isKindOfClass: class!(NSDictionary)];
        if !is_dict {
            return;
        }
        let username = dict_string(body, "username").unwrap_or_default();
        let Some(password) = dict_string(body, "password") else {
            return;
        };
        if password.is_empty() {
            return;
        }

        if let Some(app) = APP.get() {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                intake(app, host, username, password).await;
            });
        }
    }

    /// Install the capture user script + message handler on a tab webview.
    pub fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
        let _ = webview.with_webview(|w| unsafe {
            let wk: *mut AnyObject = w.inner() as *mut AnyObject;
            if wk.is_null() {
                return;
            }
            let config: *mut AnyObject = msg_send![wk, configuration];
            if config.is_null() {
                return;
            }
            let controller: *mut AnyObject = msg_send![config, userContentController];
            if controller.is_null() {
                return;
            }

            // Inject the capture script at document end, all frames.
            let src = ns_string(CAPTURE_JS);
            let script: *mut AnyObject = msg_send![class!(WKUserScript), alloc];
            let injection_time: isize = 1; // WKUserScriptInjectionTimeAtDocumentEnd
            let script: *mut AnyObject = msg_send![
                script,
                initWithSource: src,
                injectionTime: injection_time,
                forMainFrameOnly: false,
            ];
            let _: () = msg_send![controller, addUserScript: script];
            let _: () = msg_send![script, release];

            // Register the narrow message handler.
            let handler = CaptureHandler::new();
            let name = ns_string("wisCapture");
            let _: () = msg_send![controller, addScriptMessageHandler: &*handler, name: name];
        });
    }
}

/// Install the capture channel on a (non-ephemeral) tab webview. No-op off macOS.
pub fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
    #[cfg(target_os = "macos")]
    imp::install(webview);
    #[cfg(not(target_os = "macos"))]
    let _ = webview;
}
