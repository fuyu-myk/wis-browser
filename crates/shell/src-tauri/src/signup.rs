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
    /// The field's `passwordrules`, so the password fits what the site takes.
    rules: Option<String>,
    max_length: Option<u32>,
    /// It looks like a signup, so the offer is shown unprompted.
    qualifies: bool,
}

/// The password field last focused in a tab, for "Use a strong password".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldInfo {
    rules: Option<String>,
    max_length: Option<u32>,
}

static FIELDS: std::sync::Mutex<Option<std::collections::HashMap<String, FieldInfo>>> =
    std::sync::Mutex::new(None);

fn remember_field(tab_id: &str, field: FieldInfo) {
    FIELDS
        .lock()
        .expect("signup fields poisoned")
        .get_or_insert_with(Default::default)
        .insert(tab_id.to_string(), field);
}

/// Drop what a tab's page reported, when it navigates or closes.
pub(crate) fn forget_field(tab_id: &str) {
    if let Some(fields) = FIELDS.lock().expect("signup fields poisoned").as_mut() {
        fields.remove(tab_id);
    }
}

/// The password field last focused in `tab_id`'s page, if it is still there.
#[tauri::command]
pub fn signup_field(tab_id: wis_core::TabId) -> Option<FieldInfo> {
    FIELDS
        .lock()
        .expect("signup fields poisoned")
        .as_ref()
        .and_then(|fields| fields.get(&tab_id.as_uuid().to_string()).cloned())
}

/// Far past any real rules string; the page is untrusted.
const MAX_RULES_BYTES: usize = 1024;

/// The field's constraints as the page reported them, bounded and cleaned.
fn field_constraints(
    rules: Option<&str>,
    max_length: Option<i64>,
) -> (Option<String>, Option<u32>) {
    let rules = rules
        .map(str::trim)
        .filter(|rules| !rules.is_empty() && rules.len() <= MAX_RULES_BYTES)
        .map(str::to_string);
    let max_length = max_length
        .filter(|max| *max > 0)
        .and_then(|max| u32::try_from(max).ok());

    (rules, max_length)
}

pub fn page_script() -> &'static str {
    SIGNUP_JS
}

const SIGNUP_JS: &str = r#"(function () {
  if (window.__wisSignupInstalled) return;
  window.__wisSignupInstalled = true;

  function post(el, signup) {
    var body = {
      qualifies: signup,
      rules: (el.getAttribute && el.getAttribute('passwordrules')) || '',
      maxLength: el.maxLength > 0 ? el.maxLength : 0
    };
    try {
      var h = window.webkit && window.webkit.messageHandlers;
      if (h && h.wisSignup) {
        h.wisSignup.postMessage(body);
      } else if (window.wisSignup && window.wisSignup.postMessage) {
        window.wisSignup.postMessage(JSON.stringify(body));
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

  function eligible(el) {
    if (!el || el.type !== 'password' || el.disabled || el.readOnly) return false;
    var declared = (el.autocomplete || el.getAttribute('autocomplete') || '').toLowerCase();
    return declared.indexOf('current-password') === -1;
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
    if (eligible(e.target)) post(e.target, qualifies(e.target));
  }, true);
})();"#;

/// Tell the chrome that a signup field in `label`'s tab took focus.
fn announce(label: &str, rules: Option<String>, max_length: Option<u32>, qualifies: bool) {
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

    remember_field(
        tab_id,
        FieldInfo {
            rules: rules.clone(),
            max_length,
        },
    );
    let payload = SignupEvent {
        window: webview.window().label().to_string(),
        tab_id: tab_id.to_string(),
        host,
        rules,
        max_length,
        qualifies,
    };
    if let Err(err) = app.emit(SIGNUP_EVENT, payload) {
        log::warn!("emit {SIGNUP_EVENT} failed: {err}");
    }
}

#[cfg(any(windows, target_os = "linux"))]
pub(crate) fn dispatch_message(label: &str, body: &serde_json::Value) {
    let (rules, max_length) = field_constraints(
        body.get("rules").and_then(serde_json::Value::as_str),
        body.get("maxLength").and_then(serde_json::Value::as_i64),
    );
    let qualifies = body
        .get("qualifies")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    announce(label, rules, max_length, qualifies);
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

    if let Some(host) = webview
        .url()
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
    {
        GENERATED
            .lock()
            .expect("signup generated poisoned")
            .record(&host, &password);
    }

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

    use std::ffi::CStr;
    use std::os::raw::c_char;

    use super::{announce, field_constraints};

    unsafe fn ns_string(s: &str) -> *mut AnyObject {
        let c = CString::new(s).unwrap_or_default();
        msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()]
    }

    unsafe fn field(dict: *mut AnyObject, key: &str) -> *mut AnyObject {
        msg_send![dict, objectForKey: ns_string(key)]
    }

    unsafe fn string_field(dict: *mut AnyObject, key: &str) -> Option<String> {
        let value = field(dict, key);
        if value.is_null() {
            return None;
        }
        let is_str: bool = msg_send![value, isKindOfClass: class!(NSString)];
        if !is_str {
            return None;
        }
        let utf8: *const c_char = msg_send![value, UTF8String];
        if utf8.is_null() {
            return None;
        }

        Some(CStr::from_ptr(utf8).to_string_lossy().into_owned())
    }

    unsafe fn bool_field(dict: *mut AnyObject, key: &str) -> bool {
        let value = field(dict, key);
        if value.is_null() {
            return false;
        }
        let is_num: bool = msg_send![value, isKindOfClass: class!(NSNumber)];
        if !is_num {
            return false;
        }
        let flag: bool = msg_send![value, boolValue];

        flag
    }

    unsafe fn number_field(dict: *mut AnyObject, key: &str) -> Option<f64> {
        let value = field(dict, key);
        if value.is_null() {
            return None;
        }
        let is_num: bool = msg_send![value, isKindOfClass: class!(NSNumber)];
        if !is_num {
            return None;
        }
        let number: f64 = msg_send![value, doubleValue];

        Some(number)
    }

    /// The field constraints the page posted, or none when the body is not
    /// the dictionary the script sends.
    pub(super) unsafe fn read_body(message: *mut AnyObject) -> (Option<String>, Option<u32>, bool) {
        let body: *mut AnyObject = msg_send![message, body];
        if body.is_null() {
            return (None, None, false);
        }
        let is_dict: bool = msg_send![body, isKindOfClass: class!(NSDictionary)];
        if !is_dict {
            return (None, None, false);
        }

        let rules = string_field(body, "rules");
        let max_length = number_field(body, "maxLength")
            .filter(|n| n.is_finite())
            .map(|n| n as i64);

        let (rules, max_length) = field_constraints(rules.as_deref(), max_length);
        (rules, max_length, bool_field(body, "qualifies"))
    }

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

                let (rules, max_length, qualifies) = unsafe { read_body(message) };
                announce(&label, rules, max_length, qualifies);
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

/// Hashes of the passwords Wis filled, by host. The save prompt after a signup
/// reads it to tell a password only Wis has seen from one the user typed.
#[derive(Default)]
pub(crate) struct Generated {
    entries: Vec<(String, [u8; 32])>,
}

impl Generated {
    /// A handful of signups in flight is plenty; the oldest go first.
    pub(crate) const CAP: usize = 16;

    fn key(host: &str) -> String {
        let host = host.to_ascii_lowercase();
        host.strip_prefix("www.").unwrap_or(&host).to_string()
    }

    fn digest(password: &str) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        Sha256::digest(password.as_bytes()).into()
    }

    pub(crate) fn record(&mut self, host: &str, password: &str) {
        let host = Self::key(host);
        self.entries.retain(|(known, _)| *known != host);
        self.entries.push((host, Self::digest(password)));
        if self.entries.len() > Self::CAP {
            self.entries.remove(0);
        }
    }

    pub(crate) fn matches(&self, host: &str, password: &str) -> bool {
        let host = Self::key(host);
        let digest = Self::digest(password);
        self.entries
            .iter()
            .any(|(known, hash)| *known == host && *hash == digest)
    }

    pub(crate) fn forget(&mut self, host: &str) {
        let host = Self::key(host);
        self.entries.retain(|(known, _)| *known != host);
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

static GENERATED: std::sync::Mutex<Generated> = std::sync::Mutex::new(Generated {
    entries: Vec::new(),
});

/// Whether `password` is one Wis filled on `host`.
pub(crate) fn is_generated(host: &str, password: &str) -> bool {
    GENERATED
        .lock()
        .expect("signup generated poisoned")
        .matches(host, password)
}

/// Stop treating `host`'s filled password as pending, once its save prompt is
/// answered either way.
pub(crate) fn forget_generated(host: &str) {
    GENERATED
        .lock()
        .expect("signup generated poisoned")
        .forget(host);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_filled_password_is_recognised_on_the_same_host() {
        let mut generated = Generated::default();
        generated.record("github.com", "s3cret-generated");
        assert!(generated.matches("github.com", "s3cret-generated"));
    }

    #[test]
    fn a_typed_password_is_not_mistaken_for_a_generated_one() {
        let mut generated = Generated::default();
        generated.record("github.com", "s3cret-generated");
        assert!(!generated.matches("github.com", "something-i-typed"));
    }

    #[test]
    fn a_password_is_only_recognised_on_the_host_it_was_filled_on() {
        let mut generated = Generated::default();
        generated.record("github.com", "s3cret-generated");
        assert!(!generated.matches("evil.example", "s3cret-generated"));
    }

    #[test]
    fn www_and_case_do_not_split_a_host() {
        let mut generated = Generated::default();
        generated.record("www.GitHub.com", "s3cret-generated");
        assert!(generated.matches("github.com", "s3cret-generated"));
    }

    #[test]
    fn forgetting_a_host_drops_what_was_filled_there() {
        let mut generated = Generated::default();
        generated.record("github.com", "s3cret-generated");
        generated.forget("github.com");
        assert!(!generated.matches("github.com", "s3cret-generated"));
    }

    #[test]
    fn real_rules_pass_through() {
        let (rules, max) = field_constraints(Some(" minlength: 15; "), Some(64));
        assert_eq!(rules.as_deref(), Some("minlength: 15;"));
        assert_eq!(max, Some(64));
    }

    #[test]
    fn an_absent_attribute_and_an_unset_maxlength_are_none() {
        assert_eq!(field_constraints(Some(""), Some(0)), (None, None));
        assert_eq!(field_constraints(None, Some(-1)), (None, None));
    }

    #[test]
    fn an_oversized_rules_string_from_the_page_is_dropped() {
        let huge = "required: upper;".repeat(200);
        assert_eq!(field_constraints(Some(&huge), None).0, None);
    }

    #[test]
    fn a_maxlength_past_u32_is_dropped_rather_than_wrapped() {
        assert_eq!(field_constraints(None, Some(i64::MAX)).1, None);
    }

    #[test]
    fn the_store_is_bounded() {
        let mut generated = Generated::default();
        for i in 0..100 {
            generated.record(&format!("site{i}.example"), "pw");
        }
        assert!(generated.len() <= Generated::CAP);
        assert!(
            generated.matches("site99.example", "pw"),
            "the newest survive"
        );
    }
}
