//! Signup detection: tell the chrome when a new-password field takes focus,
//! so it can offer a generated password, and when a sign-in field does, so it
//! can offer the logins saved for the site.
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
//! A sign-in field is a password field that does not qualify, or the username
//! before one. On a username-first page, with no password field yet, only a
//! field declared `autocomplete="username"` counts.
//!
//! Text heuristics are deliberately absent.
//!
//! The tab and host are read natively from the webview, never from the page.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

const SIGNUP_EVENT: &str = "wis-signup-field";
const LOGIN_EVENT: &str = "wis-login-field";
/// The anchored field moved, because the page scrolled or resized.
const SIGNUP_MOVE_EVENT: &str = "wis-signup-move";
/// The tab's page went, by navigation or close, taking its fields with it.
const SIGNUP_GONE_EVENT: &str = "wis-signup-gone";
/// Focus moved on from the field an offer was open for.
const SIGNUP_LEAVE_EVENT: &str = "wis-signup-leave";
/// The page started scrolling under an open offer; a move follows once it settles.
const SIGNUP_SCROLL_EVENT: &str = "wis-signup-scroll";

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
    /// Where the field is, so the offer can sit beside it.
    rect: Option<FieldRect>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct LoginEvent {
    window: String,
    tab_id: String,
    host: String,
    rect: FieldRect,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SignupMoveEvent {
    tab_id: String,
    rect: FieldRect,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SignupGoneEvent {
    tab_id: String,
}

/// A field's box in its page's own CSS pixels, with the viewport width the
/// chrome measures its zoom against.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FieldRect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    view_width: f64,
}

/// Past any real page. These end up as overlay bounds, so the page is not
/// trusted to keep them sane.
const MAX_COORD: f64 = 1_000_000.0;

fn field_rect(
    x: Option<f64>,
    y: Option<f64>,
    width: Option<f64>,
    height: Option<f64>,
    view_width: Option<f64>,
) -> Option<FieldRect> {
    let (x, y, width, height, view_width) = (x?, y?, width?, height?, view_width?);
    let parts = [x, y, width, height, view_width];
    if parts
        .iter()
        .any(|part| !part.is_finite() || part.abs() > MAX_COORD)
    {
        return None;
    }
    if width < 0.0 || height < 0.0 || view_width <= 0.0 {
        return None;
    }

    Some(FieldRect {
        x,
        y,
        width,
        height,
        view_width,
    })
}

#[derive(Debug)]
enum Report {
    Leave,
    Scroll,
    Focus {
        rules: Option<String>,
        max_length: Option<u32>,
        qualifies: bool,
        rect: Option<FieldRect>,
    },
    Move(FieldRect),
    Login(FieldRect),
}

/// What the page sent. Read through accessors, so one parser serves both the
/// macOS dictionary and the JSON the other platforms deliver.
fn read_report(
    text: impl Fn(&str) -> Option<String>,
    number: impl Fn(&str) -> Option<f64>,
    flag: impl Fn(&str) -> bool,
) -> Option<Report> {
    let rect = field_rect(
        number("x"),
        number("y"),
        number("width"),
        number("height"),
        number("viewWidth"),
    );
    match text("kind").as_deref() {
        Some("move") => return rect.map(Report::Move),
        Some("login") => return rect.map(Report::Login),
        Some("leave") => return Some(Report::Leave),
        Some("scroll") => return Some(Report::Scroll),
        _ => {}
    }

    let (rules, max_length) = field_constraints(
        text("rules").as_deref(),
        number("maxLength")
            .filter(|n| n.is_finite())
            .map(|n| n as i64),
    );

    Some(Report::Focus {
        rules,
        max_length,
        qualifies: flag("qualifies"),
        rect,
    })
}

fn dispatch(label: &str, report: Report) {
    match report {
        Report::Focus {
            rules,
            max_length,
            qualifies,
            rect,
        } => {
            announce(label, rules, max_length, qualifies, rect);
            if let (false, Some(rect)) = (qualifies, rect) {
                announce_login(label, rect);
            }
        }
        Report::Move(rect) => announce_move(label, rect),
        Report::Login(rect) => announce_login(label, rect),
        Report::Leave => announce_tab(label, SIGNUP_LEAVE_EVENT),
        Report::Scroll => announce_tab(label, SIGNUP_SCROLL_EVENT),
    }
}

fn announce_tab(label: &str, event: &str) {
    let Some(app) = APP.get() else {
        return;
    };
    let Some(tab_id) = label.strip_prefix("tab-") else {
        return;
    };
    let payload = SignupGoneEvent {
        tab_id: tab_id.to_string(),
    };
    if let Err(err) = app.emit(event, payload) {
        log::warn!("emit {event} failed: {err}");
    }
}

fn announce_move(label: &str, rect: FieldRect) {
    let Some(app) = APP.get() else {
        return;
    };
    let Some(tab_id) = label.strip_prefix("tab-") else {
        return;
    };
    let payload = SignupMoveEvent {
        tab_id: tab_id.to_string(),
        rect,
    };
    if let Err(err) = app.emit(SIGNUP_MOVE_EVENT, payload) {
        log::warn!("emit {SIGNUP_MOVE_EVENT} failed: {err}");
    }
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

/// Drop what a tab's page reported, when it navigates or closes, and tell the
/// chrome so a card anchored to that page goes with it.
pub(crate) fn forget_field(tab_id: &str) {
    if let Some(fields) = FIELDS.lock().expect("signup fields poisoned").as_mut() {
        fields.remove(tab_id);
    }

    if let Some(app) = APP.get() {
        let payload = SignupGoneEvent {
            tab_id: tab_id.to_string(),
        };
        if let Err(err) = app.emit(SIGNUP_GONE_EVENT, payload) {
            log::warn!("emit {SIGNUP_GONE_EVENT} failed: {err}");
        }
    }
}

/// Stop the page reporting where its offer's field is, once the offer closes.
#[tauri::command]
pub fn release_signup_field(app: AppHandle, tab_id: wis_core::TabId) {
    let label = format!("tab-{}", tab_id.as_uuid());
    if let Some(webview) = app.webviews().get(&label) {
        let _ = webview.eval("window.__wisSignupRelease && window.__wisSignupRelease();");
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

  var anchored = null;
  var settle = null;

  function send(body) {
    try {
      var h = window.webkit && window.webkit.messageHandlers;
      if (h && h.wisSignup) {
        h.wisSignup.postMessage(body);
      } else if (window.wisSignup && window.wisSignup.postMessage) {
        window.wisSignup.postMessage(JSON.stringify(body));
      }
    } catch (e) {}
  }

  function measured(el, kind) {
    var r = el.getBoundingClientRect();
    return {
      kind: kind,
      x: r.left,
      y: r.top,
      width: r.width,
      height: r.height,
      viewWidth: window.innerWidth
    };
  }

  function post(el, signup) {
    var body = measured(el, 'focus');
    body.qualifies = signup;
    body.rules = (el.getAttribute && el.getAttribute('passwordrules')) || '';
    body.maxLength = el.maxLength > 0 ? el.maxLength : 0;
    anchored = el;
    send(body);
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

  function textual(el) {
    if (!el || el.tagName !== 'INPUT' || el.disabled || el.readOnly) return false;
    var t = (el.type || 'text').toLowerCase();
    return t === 'text' || t === 'email' || t === 'tel';
  }

  // A password field that is not for a signup.
  // With no password field around, as on a username-first page, only a field
  // declared with 'username' counts.
  function signIn(el) {
    if (el && el.type === 'password') return !el.disabled && !el.readOnly && !qualifies(el);
    if (!textual(el)) return false;
    var pw = null;
    try {
      pw = (el.form || document).querySelector('input[type=password]');
    } catch (e) {}
    if (pw) return !!(el.compareDocumentPosition(pw) & 4) && !qualifies(pw);
    return (el.getAttribute('autocomplete') || '').toLowerCase().indexOf('username') !== -1;
  }

  function leave() {
    anchored = null;
    clearTimeout(settle);
    settle = null;
    send({ kind: 'leave' });
  }

  document.addEventListener('focusin', function (e) {
    if (window.top !== window.self) return;
    // Moving to anything else in the page closes the offer.
    if (anchored && e.target !== anchored) leave();
    report(e.target);
  }, true);

  function report(el) {
    if (eligible(el)) {
      post(el, qualifies(el));
    } else if (signIn(el)) {
      anchored = el;
      send(measured(el, 'login'));
    }
  }

  document.addEventListener('pointerdown', function (e) {
    if (anchored && e.target !== anchored) leave();
    if (!anchored && window.top === window.self && e.target === document.activeElement) {
      report(e.target);
    }
  }, true);
  document.addEventListener('keydown', function (e) {
    if (anchored && e.target === anchored && e.key === 'Escape') leave();
  }, true);
  document.addEventListener('input', function (e) {
    if (anchored && e.target === anchored && e.isTrusted) leave();
  }, true);

  function moved(e) {
    if (!anchored) return;
    var scroller = e && e.target;
    if (scroller && scroller !== document && scroller.contains && !scroller.contains(anchored)) return;
    if (settle === null) send({ kind: 'scroll' });
    else clearTimeout(settle);
    settle = setTimeout(function () {
      settle = null;
      if (!anchored) return;
      if (!anchored.isConnected) {
        leave();
        return;
      }
      send(measured(anchored, 'move'));
    }, 150);
  }

  window.addEventListener('scroll', moved, { capture: true, passive: true });
  window.addEventListener('resize', moved, { passive: true });
  window.__wisSignupRelease = function () {
    anchored = null;
    clearTimeout(settle);
    settle = null;
  };
})();"#;

/// Where a report came from: the app, the tab id, its window and the page's
/// host, all read from the webview.
struct Source<'a> {
    app: &'static AppHandle,
    tab_id: &'a str,
    window: String,
    host: String,
}

fn source(label: &str) -> Option<Source<'_>> {
    let app = APP.get()?;
    let tab_id = label.strip_prefix("tab-")?;
    let webview = app.webviews().get(label).cloned()?;
    let host = webview
        .url()
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))?;
    Some(Source {
        app,
        tab_id,
        window: webview.window().label().to_string(),
        host,
    })
}

/// Tell the chrome that a signup field in `label`'s tab took focus.
fn announce(
    label: &str,
    rules: Option<String>,
    max_length: Option<u32>,
    qualifies: bool,
    rect: Option<FieldRect>,
) {
    let Some(Source {
        app,
        tab_id,
        window,
        host,
    }) = source(label)
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
        window,
        tab_id: tab_id.to_string(),
        host,
        rules,
        max_length,
        qualifies,
        rect,
    };
    if let Err(err) = app.emit(SIGNUP_EVENT, payload) {
        log::warn!("emit {SIGNUP_EVENT} failed: {err}");
    }
}

/// Tell the chrome that a sign-in field in `label`'s tab took focus.
fn announce_login(label: &str, rect: FieldRect) {
    let Some(Source {
        app,
        tab_id,
        window,
        host,
    }) = source(label)
    else {
        return;
    };

    let payload = LoginEvent {
        window,
        tab_id: tab_id.to_string(),
        host,
        rect,
    };
    if let Err(err) = app.emit(LOGIN_EVENT, payload) {
        log::warn!("emit {LOGIN_EVENT} failed: {err}");
    }
}

#[cfg(any(windows, target_os = "linux"))]
pub(crate) fn dispatch_message(label: &str, body: &serde_json::Value) {
    let report = read_report(
        |key| {
            body.get(key)
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        },
        |key| body.get(key).and_then(serde_json::Value::as_f64),
        |key| {
            body.get(key)
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        },
    );
    if let Some(report) = report {
        dispatch(label, report);
    }
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

    use super::{dispatch, read_report, Report};

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

    /// What the page posted, or none when the body is not the dictionary the
    /// script sends.
    pub(super) unsafe fn read_body(message: *mut AnyObject) -> Option<Report> {
        let body: *mut AnyObject = msg_send![message, body];
        if body.is_null() {
            return None;
        }
        let is_dict: bool = msg_send![body, isKindOfClass: class!(NSDictionary)];
        if !is_dict {
            return None;
        }

        read_report(
            |key| unsafe { string_field(body, key) },
            |key| unsafe { number_field(body, key) },
            |key| unsafe { bool_field(body, key) },
        )
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

                if let Some(report) = unsafe { read_body(message) } {
                    dispatch(&label, report);
                }
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

    fn report_from(fields: &[(&str, &str)]) -> Option<Report> {
        let map: std::collections::HashMap<&str, &str> = fields.iter().copied().collect();
        read_report(
            |key| map.get(key).map(|value| value.to_string()),
            |key| map.get(key).and_then(|value| value.parse().ok()),
            |key| map.get(key) == Some(&"true"),
        )
    }

    const RECT: [(&str, &str); 5] = [
        ("x", "120"),
        ("y", "340.5"),
        ("width", "300"),
        ("height", "40"),
        ("viewWidth", "1280"),
    ];

    #[test]
    fn a_focus_carries_the_fields_constraints_and_where_it_is() {
        let mut fields = RECT.to_vec();
        fields.extend([
            ("kind", "focus"),
            ("qualifies", "true"),
            ("rules", "maxlength: 16;"),
            ("maxLength", "64"),
        ]);
        let Some(Report::Focus {
            rules,
            max_length,
            qualifies,
            rect,
        }) = report_from(&fields)
        else {
            panic!("expected a focus report");
        };
        assert_eq!(rules.as_deref(), Some("maxlength: 16;"));
        assert_eq!(max_length, Some(64));
        assert!(qualifies);
        assert_eq!(rect.map(|r| (r.x, r.y)), Some((120.0, 340.5)));
    }

    #[test]
    fn a_focus_without_a_usable_rect_still_reports_the_field() {
        let Some(Report::Focus { rect, .. }) = report_from(&[("kind", "focus")]) else {
            panic!("expected a focus report");
        };
        assert!(
            rect.is_none(),
            "the offer falls back to the top of the pane"
        );
    }

    #[test]
    fn a_move_with_a_rect_is_a_move() {
        let mut fields = RECT.to_vec();
        fields.push(("kind", "move"));
        assert!(matches!(report_from(&fields), Some(Report::Move(_))));
    }

    #[test]
    fn a_leave_needs_nothing_else() {
        assert!(matches!(
            report_from(&[("kind", "leave")]),
            Some(Report::Leave)
        ));
    }

    #[test]
    fn a_login_needs_a_rect() {
        let mut fields = RECT.to_vec();
        fields.push(("kind", "login"));
        assert!(matches!(report_from(&fields), Some(Report::Login(_))));
        assert!(report_from(&[("kind", "login")]).is_none());
    }

    #[test]
    fn a_scroll_needs_nothing_else() {
        assert!(matches!(
            report_from(&[("kind", "scroll")]),
            Some(Report::Scroll)
        ));
    }

    #[test]
    fn a_move_without_a_rect_is_dropped() {
        assert!(report_from(&[("kind", "move"), ("x", "1")]).is_none());
    }

    #[test]
    fn non_finite_coordinates_are_refused() {
        assert!(field_rect(Some(f64::NAN), Some(0.0), Some(1.0), Some(1.0), Some(800.0)).is_none());
        assert!(field_rect(
            Some(0.0),
            Some(f64::INFINITY),
            Some(1.0),
            Some(1.0),
            Some(800.0)
        )
        .is_none());
    }

    #[test]
    fn a_negative_size_or_no_viewport_is_refused() {
        assert!(field_rect(Some(0.0), Some(0.0), Some(-1.0), Some(1.0), Some(800.0)).is_none());
        assert!(field_rect(Some(0.0), Some(0.0), Some(1.0), Some(1.0), Some(0.0)).is_none());
    }

    /// The page is untrusted, and these end up as overlay bounds.
    #[test]
    fn an_absurd_magnitude_is_refused() {
        assert!(field_rect(Some(1e12), Some(0.0), Some(1.0), Some(1.0), Some(800.0)).is_none());
    }

    #[test]
    fn a_field_scrolled_above_the_viewport_is_still_a_rect() {
        assert!(field_rect(Some(10.0), Some(-500.0), Some(1.0), Some(1.0), Some(800.0)).is_some());
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
