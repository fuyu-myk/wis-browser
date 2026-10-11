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
//!   origin (no cross-origin poisoning). A login from a subframe counts only
//!   when the frame is on the page's own site.
//! - The plaintext password is held only in [`AppState::pending_captures`],
//!   one per tab, and never crosses back out to any webview: the consent event
//!   carries just `{tab, host, username}`. Saving happens entirely
//!   backend-side via [`capture_save`]. Secrets are never logged.
//! - Private/ephemeral tabs never install the handler (see
//!   `create_tab_webview`), so nothing is captured while browsing privately.

use std::sync::OnceLock;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use wis_core::{TabId, WisError, WisResult};
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
    /// Wis filled this password, so nothing but this prompt holds it.
    pub generated: bool,
    /// Saving replaces a password already stored under this username.
    pub replaces: bool,
}

#[derive(Debug, Serialize, Clone)]
struct CaptureEvent {
    tab: TabId,
    host: String,
    username: String,
    generated: bool,
    replaces: bool,
}

/// The pending capture's display fields (no password), for a surface that
/// mounts after the event fired.
#[derive(Debug, Serialize, Clone)]
pub struct CaptureInfo {
    host: String,
    username: String,
    generated: bool,
    replaces: bool,
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
#[cfg(any(windows, target_os = "linux"))]
pub(crate) fn dispatch_message(label: &str, username: String, password: String) {
    if password.is_empty() {
        return;
    }
    let Some(app) = APP.get() else {
        return;
    };
    let Some(tab) = tab_of(label) else {
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
        intake(app, tab, host, username, password).await;
    });
}

/// The tab behind webview `label` (`tab-<uuid>`).
fn tab_of(label: &str) -> Option<TabId> {
    label
        .strip_prefix("tab-")
        .and_then(|id| uuid::Uuid::parse_str(id).ok())
        .map(TabId::from_uuid)
}

/// Whether a login submitted in a frame on `frame_host` may be offered for the
/// page on `page_host`: from the main frame always, from a subframe only when
/// it is on the page's own site.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn frame_may_offer(main_frame: bool, page_host: &str, frame_host: &str) -> bool {
    main_frame || crate::popup_chrome::same_site_hosts(page_host, frame_host)
}

fn strip_www(host: &str) -> &str {
    host.strip_prefix("www.").unwrap_or(host)
}

/// Dedup, stash `tab`'s pending capture (replacing an earlier one from that
/// tab), and emit the consent event. Skips logins whose `(host, username)` is
/// already saved so the user isn't nagged.
async fn intake(app: AppHandle, tab: TabId, host: String, username: String, password: String) {
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
    let replaces = existing.iter().any(|c| c.username == username);
    let generated = crate::signup::is_generated(&host, &password);
    if !should_offer(replaces, generated) {
        return;
    }

    state.pending_captures.lock().await.insert(
        tab,
        PendingCapture {
            host: host.clone(),
            username: username.clone(),
            password: Zeroizing::new(password),
            generated,
            replaces,
        },
    );

    let _ = app.emit(
        CAPTURE_EVENT,
        CaptureEvent {
            tab,
            host,
            username,
            generated,
            replaces,
        },
    );
}

/// Whether a captured login is offered for saving. One already saved under
/// that username is skipped, except when the password is one Wis generated.
fn should_offer(already_saved: bool, generated: bool) -> bool {
    generated || !already_saved
}

/// Encrypt and save `tab_id`'s pending captured login. Requires the vault to
/// be unlocked (the chrome unlocks via Touch ID first); leaves the pending
/// capture in place if locked so the prompt can be retried.
#[tauri::command]
pub async fn capture_save(state: State<'_, AppState>, tab_id: TabId) -> WisResult<()> {
    if state.vault_key.lock().await.is_none() {
        return Err(locked());
    }

    let pending = match state.pending_captures.lock().await.remove(&tab_id) {
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

    crate::signup::forget_generated(&pending.host);

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

/// Discard `tab_id`'s pending captured login (user clicked "Not now").
#[tauri::command]
pub async fn capture_dismiss(state: State<'_, AppState>, tab_id: TabId) -> WisResult<()> {
    if let Some(pending) = state.pending_captures.lock().await.remove(&tab_id) {
        crate::signup::forget_generated(&pending.host);
    }
    Ok(())
}

/// Drop a closed tab's pending capture.
pub async fn forget(state: &AppState, tab: TabId) {
    state.pending_captures.lock().await.remove(&tab);
}

/// `tab_id`'s pending capture (host + username, never the password), so a
/// surface that mounts after the `wis-capture-login` event can still render it.
#[tauri::command]
pub async fn capture_pending(
    state: State<'_, AppState>,
    tab_id: TabId,
) -> WisResult<Option<CaptureInfo>> {
    let guard = state.pending_captures.lock().await;
    Ok(guard.get(&tab_id).map(|p| CaptureInfo {
        host: p.host.clone(),
        username: p.username.clone(),
        generated: p.generated,
        replaces: p.replaces,
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
#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
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
    // Hidden fields stay in.
    for (var k = 0; k < inputs.length; k++) {
      if ((inputs[k].type || '').toLowerCase() === 'password') continue;
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
    return /log\s?in|sign\s?in|sign\s?on|log\s?on|sign\s?up|register|create\s+(an\s+)?account|join|continue|submit|next|done|confirm/.test(label);
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
#[cfg(any(windows, target_os = "linux"))]
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

    use super::{frame_may_offer, intake, tab_of, APP, CAPTURE_JS};
    use crate::content_world::macos as content_world;

    struct HandlerIvars {
        label: String,
    }

    declare_class!(
        struct CaptureHandler;

        unsafe impl ClassType for CaptureHandler {
            type Super = NSObject;
            type Mutability = mutability::InteriorMutable;
            const NAME: &'static str = "WisCaptureHandler";
        }

        impl DeclaredClass for CaptureHandler {
            type Ivars = HandlerIvars;
        }

        unsafe impl CaptureHandler {
            #[method(userContentController:didReceiveScriptMessage:)]
            fn did_receive(&self, _ucc: *mut AnyObject, message: *mut AnyObject) {
                let label = self.ivars().label.clone();
                unsafe { handle_message(&label, message) }
            }
        }
    );

    impl CaptureHandler {
        fn new(label: String) -> Id<Self> {
            let this = Self::alloc().set_ivars(HandlerIvars { label });
            unsafe { msg_send_id![super(this), init] }
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

    unsafe fn handle_message(label: &str, message: *mut AnyObject) {
        if message.is_null() {
            return;
        }
        let Some(tab) = tab_of(label) else {
            return;
        };

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

        let frame: *mut AnyObject = msg_send![message, frameInfo];
        if frame.is_null() {
            return;
        }
        let main_frame: bool = msg_send![frame, isMainFrame];
        let origin: *mut AnyObject = msg_send![frame, securityOrigin];
        let frame_host = if origin.is_null() {
            None
        } else {
            let origin_host: *mut AnyObject = msg_send![origin, host];
            nsstring_to_string(origin_host)
        };
        if !frame_may_offer(main_frame, &host, frame_host.as_deref().unwrap_or_default()) {
            return;
        }

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
                intake(app, tab, host, username, password).await;
            });
        }
    }

    /// Install the capture user script + message handler on a tab webview.
    pub fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
        let label = webview.label().to_string();
        let _ = webview.with_webview(move |w| unsafe {
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

            let handler = CaptureHandler::new(label.clone());
            content_world::add_handler(controller, &*handler, "wisCapture");
            content_world::install_script(wk, controller, &label, "wisCapture", CAPTURE_JS, false);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_login_from_the_main_frame_is_offered() {
        assert!(frame_may_offer(true, "bank.example", "ads.example"));
    }

    #[test]
    fn a_subframe_offers_only_from_the_page_site() {
        assert!(frame_may_offer(false, "apple.com", "idmsa.apple.com"));
        assert!(!frame_may_offer(false, "news.example", "ads.tracker.test"));
        assert!(!frame_may_offer(false, "news.example", ""));
    }

    #[test]
    fn a_new_login_is_offered() {
        assert!(should_offer(false, false));
    }

    #[test]
    fn a_login_already_saved_is_not_offered_again() {
        assert!(!should_offer(true, false));
    }

    /// A password Wis generated is new by construction, so a saved username
    /// means a password change. Skipping it would keep the old password
    /// saved and the new one nowhere.
    #[test]
    fn a_generated_password_is_offered_even_over_a_saved_login() {
        assert!(should_offer(true, true));
    }

    #[test]
    fn a_generated_password_for_a_new_login_is_offered() {
        assert!(should_offer(false, true));
    }
}

#[cfg(all(test, target_os = "macos"))]
mod sends {
    use objc2::runtime::{AnyObject, Bool};
    use objc2::{class, sel};

    type Obj = *mut AnyObject;

    /// A wrong message type aborts at runtime only, so check each frame send
    /// against its method.
    #[test]
    fn every_frame_send_matches_its_method() {
        class!(WKScriptMessage)
            .verify_sel::<(), Obj>(sel!(frameInfo))
            .unwrap();
        let frame = class!(WKFrameInfo);
        frame.verify_sel::<(), Bool>(sel!(isMainFrame)).unwrap();
        frame.verify_sel::<(), Obj>(sel!(securityOrigin)).unwrap();
        class!(WKSecurityOrigin)
            .verify_sel::<(), Obj>(sel!(host))
            .unwrap();
    }
}
