//! Signup detection: tell the chrome when a new-password field takes focus,
//! so it can offer a generated password.
//!
//! Focus is the trigger: it is both the moment the offer is useful and
//! cheaper than watching the DOM.
//!
//! A field qualifies on `autocomplete="new-password"`, on declaring
//! `passwordrules`, on more than one password input in its form, or on a field
//! elsewhere on the page being marked `current-password` while this one is bare:
//! the sign-in side gets marked and the register side is left as it is. A field
//! marked `current-password` never qualifies.
//!
//! Text heuristics are deliberately absent.
//!
//! The tab and host are read natively from the webview, never from the page.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

const SIGNUP_EVENT: &str = "wis-signup-field";

static APP: std::sync::OnceLock<AppHandle> = std::sync::OnceLock::new();

pub fn set_app_handle(app: AppHandle) {
    let _ = APP.set(app);
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SignupEvent {
    /// Events reach every window, so the chrome filters on its own.
    window: String,
    tab_id: String,
    host: String,
}

pub fn page_script() -> &'static str {
    SIGNUP_JS
}

const SIGNUP_JS: &str = r#"(function () {
  if (window.__wisSignupInstalled) return;
  window.__wisSignupInstalled = true;

  function post() {
    try {
      var h = window.webkit && window.webkit.messageHandlers;
      if (h && h.wisSignup) {
        h.wisSignup.postMessage({});
      } else if (window.wisSignup && window.wisSignup.postMessage) {
        window.wisSignup.postMessage('{}');
      }
    } catch (e) {}
  }

  function passwordCount(root) {
    try {
      return root.querySelectorAll('input[type=password]').length;
    } catch (e) {
      return 0;
    }
  }

  function signInFieldElsewhere(el) {
    try {
      var marked = document.querySelectorAll(
        'input[type=password][autocomplete~="current-password"]');
      for (var i = 0; i < marked.length; i++) {
        if (marked[i] !== el && marked[i].form !== el.form) return true;
      }
    } catch (e) {}
    return false;
  }

  function qualifies(el) {
    if (!el || el.type !== 'password' || el.disabled || el.readOnly) return false;
    var declared = (el.autocomplete || el.getAttribute('autocomplete') || '').toLowerCase();
    if (declared.indexOf('current-password') !== -1) return false;
    if (declared.indexOf('new-password') !== -1) return true;
    if (el.hasAttribute && el.hasAttribute('passwordrules')) return true;
    // Signing in never asks twice.
    if (passwordCount(el.form || document) > 1) return true;
    return signInFieldElsewhere(el);
  }

  document.addEventListener('focusin', function (e) {
    if (window.top !== window.self) return;
    if (qualifies(e.target)) post();
  }, true);
})();"#;

/// Tell the chrome that a signup field in `label`'s tab took focus.
fn announce(label: &str) {
    let Some(app) = APP.get() else {
        return;
    };
    let Some(tab_id) = label.strip_prefix("tab-") else {
        return;
    };
    let Some(webview) = app.webviews().get(label).cloned() else {
        return;
    };
    let Some(host) = webview
        .url()
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
    else {
        return;
    };

    let payload = SignupEvent {
        window: webview.window().label().to_string(),
        tab_id: tab_id.to_string(),
        host,
    };
    if let Err(err) = app.emit(SIGNUP_EVENT, payload) {
        log::warn!("emit {SIGNUP_EVENT} failed: {err}");
    }
}

#[cfg(any(windows, target_os = "linux"))]
pub(crate) fn dispatch_message(label: &str) {
    announce(label);
}

/// Put `password` into every password field of a signup form.
///
/// Only empty fields are written, so accepting an offer can never overwrite a
/// password already typed.
#[tauri::command]
pub async fn fill_generated_password(
    app: AppHandle,
    tab_id: wis_core::TabId,
    password: String,
) -> wis_core::WisResult<()> {
    let label = format!("tab-{}", tab_id.as_uuid());
    let webview =
        app.webviews()
            .get(&label)
            .cloned()
            .ok_or_else(|| wis_core::WisError::InvalidInput {
                message: format!("no webview labelled {label}"),
            })?;

    let literal = serde_json::to_string(&password).unwrap_or_else(|_| "\"\"".to_string());
    let script = format!(
        r#"(function(){{
  try {{
    var fields = document.querySelectorAll(
      'input[type=password]:not([disabled]):not([readonly])');
    for (var i = 0; i < fields.length; i++) {{
      var el = fields[i];
      if (el.value) continue;
      var d = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(el), 'value');
      if (d && d.set) {{ d.set.call(el, {literal}); }} else {{ el.value = {literal}; }}
      el.dispatchEvent(new Event('input', {{ bubbles: true }}));
      el.dispatchEvent(new Event('change', {{ bubbles: true }}));
    }}
  }} catch (e) {{}}
}})();"#
    );

    webview
        .eval(&script)
        .map_err(|err| wis_core::WisError::InvalidInput {
            message: format!("password fill failed: {err}"),
        })
}

pub fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
    #[cfg(target_os = "macos")]
    macos::install(webview, webview.label().to_string());

    #[cfg(not(target_os = "macos"))]
    let _ = webview;
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::CString;

    use objc2::rc::Id;
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{
        class, declare_class, msg_send, msg_send_id, mutability, ClassType, DeclaredClass,
    };

    use super::announce;

    struct HandlerIvars {
        label: String,
    }

    declare_class!(
        struct SignupHandler;

        unsafe impl ClassType for SignupHandler {
            type Super = NSObject;
            type Mutability = mutability::InteriorMutable;
            const NAME: &'static str = "WisSignupHandler";
        }

        impl DeclaredClass for SignupHandler {
            type Ivars = HandlerIvars;
        }

        unsafe impl SignupHandler {
            #[method(userContentController:didReceiveScriptMessage:)]
            fn did_receive(&self, _ucc: *mut AnyObject, message: *mut AnyObject) {
                let label = self.ivars().label.clone();
                unsafe {
                    if message.is_null() {
                        return;
                    }

                    let frame: *mut AnyObject = msg_send![message, frameInfo];
                    if frame.is_null() {
                        return;
                    }

                    let is_main_frame: bool = msg_send![frame, isMainFrame];
                    if !is_main_frame {
                        return;
                    }
                }

                announce(&label);
            }
        }
    );

    impl SignupHandler {
        fn new(label: String) -> Id<Self> {
            let this = Self::alloc().set_ivars(HandlerIvars { label });
            unsafe { msg_send_id![super(this), init] }
        }
    }

    pub fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>, label: String) {
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

            let handler = SignupHandler::new(label.clone());
            let c = CString::new("wisSignup").unwrap_or_default();
            let name: *mut AnyObject =
                msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()];
            let _: () = msg_send![controller, addScriptMessageHandler: &*handler, name: name];
        });
    }
}
