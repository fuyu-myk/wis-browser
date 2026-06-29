//! Per-site camera/microphone consent.
//!
//! Embedded WKWebView grants `getUserMedia` silently. Rather than alter wry's
//! permission delegate, [`install`] injects a **document-start** script (all
//! frames) that wraps `getUserMedia`: a page can reach the camera/microphone
//! only through a consent gate. The gate posts the request to the narrow
//! `wisMediaPermission` handler; a remembered per-origin decision resolves it
//! immediately, otherwise [`handle`] emits `wis-permission-request` and the
//! chrome shows the in-browser prompt overlay, which calls
//! [`resolve_media_permission`] back. The override is installed before any page
//! script runs and the original `getUserMedia` is captured in a closure the
//! page can't reach, so WebKit still rubber-stamps the underlying request but
//! only ever sees requests the user already approved. The
//! `NSCameraUsageDescription` / `NSMicrophoneUsageDescription` Info.plist
//! strings let the OS open the device once consent is given.

use std::sync::OnceLock;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;

static APP: OnceLock<AppHandle> = OnceLock::new();

/// Event raised when a page asks for the camera/microphone and the answer is
/// not already remembered; the chrome shows the consent prompt overlay.
const PERMISSION_REQUEST_EVENT: &str = "wis-permission-request";

/// Record the app handle so the message handler can reach [`AppState`] and emit.
pub fn set_app_handle(app: AppHandle) {
    let _ = APP.set(app);
}

/// The getUserMedia consent gate, exposed so the Windows tab builder can inject
/// it as a document-start init script.
#[cfg(windows)]
pub fn gate_script() -> &'static str {
    GATE_JS
}

/// Decide a camera/mic request from a parsed `{ id, origin, kind }` payload (the
/// Windows dispatcher's entry; macOS reaches `handle` through its
/// `wisMediaPermission` script-message handler). A remembered decision resolves
/// in the page immediately; an unknown one is deferred to the prompt overlay
/// (which `handle` raises), then answered by `resolve_media_permission`.
#[cfg(windows)]
pub(crate) fn dispatch_message(label: &str, page_id: &str, origin: &str, kind: &str) {
    // `id` splices into the resolve-callback string, so restrict it to digits.
    if page_id.is_empty() || !page_id.bytes().all(|b| b.is_ascii_digit()) {
        return;
    }
    let Some(granted) = handle(label, page_id, origin, kind) else {
        return;
    };

    if let Some(app) = APP.get() {
        if let Some(wv) = app.webviews().get(label).cloned() {
            let js = format!(
                "window.__wisMediaResolve && window.__wisMediaResolve(\"{page_id}\", {});",
                if granted { "true" } else { "false" }
            );
            let _ = wv.eval(&js);
        }
    }
}

/// A request awaiting the user's decision: how to reach the page that asked.
pub struct PendingMedia {
    pub webview_label: String,
    /// Page-side request id, looked up in the gate's `pending` map.
    pub page_id: String,
    pub origin: String,
    pub kind: String,
}

#[derive(Serialize, Clone)]
struct PermissionRequest {
    request_id: u64,
    origin: String,
    kind: String,
}

/// Page-world gate injected at document start in every frame. Wraps
/// `MediaDevices.prototype.getUserMedia` so the camera/microphone is reachable
/// only after the `wisMediaPermission` handler resolves the request; the
/// original is captured in `realGUM`, which the page cannot reach.
#[cfg(any(target_os = "macos", windows))]
const GATE_JS: &str = r#"(function () {
  var proto = window.MediaDevices && window.MediaDevices.prototype;
  if (!proto || typeof proto.getUserMedia !== 'function' || window.__wisMediaGate) return;
  window.__wisMediaGate = true;
  var realGUM = proto.getUserMedia;
  var pending = Object.create(null);
  var seq = 0;
  Object.defineProperty(window, '__wisMediaResolve', {
    value: function (id, granted) {
      var r = pending[id];
      if (!r) return;
      delete pending[id];
      r(!!granted);
    },
    writable: false, configurable: false, enumerable: false
  });
  function ask(kind) {
    return new Promise(function (resolve) {
      try {
        var id = String(++seq);
        pending[id] = resolve;
        window.webkit.messageHandlers.wisMediaPermission.postMessage(
          { id: id, origin: location.origin, kind: kind }
        );
      } catch (e) { resolve(false); }
    });
  }
  function kindOf(c) {
    var v = !!(c && c.video), a = !!(c && c.audio);
    return (v && a) ? 'camera+microphone' : v ? 'camera' : a ? 'microphone' : 'camera';
  }
  proto.getUserMedia = function (constraints) {
    var self = this;
    return ask(kindOf(constraints)).then(function (granted) {
      if (!granted) {
        return Promise.reject(new DOMException('Permission denied', 'NotAllowedError'));
      }
      return realGUM.call(self, constraints);
    });
  };
})();"#;

/// Decide a freshly received request. `Some(granted)` when the origin + kind is
/// already remembered (the caller resolves the page immediately); `None` when
/// the chrome has been asked to prompt (resolution arrives via
/// [`resolve_media_permission`]). Runs on the main thread.
fn handle(label: &str, page_id: &str, origin: &str, kind: &str) -> Option<bool> {
    let app = APP.get()?;
    let state = app.state::<AppState>();
    let key = format!("{kind}|{origin}");

    // Session ("just once") decisions take precedence over the persisted set.
    if let Some(&decided) = state
        .media_permissions_session
        .read()
        .expect("media_permissions_session poisoned")
        .get(&key)
    {
        return Some(decided);
    }
    if let Some(&decided) = state
        .media_permissions
        .read()
        .expect("media_permissions poisoned")
        .get(&key)
    {
        return Some(decided);
    }

    let request_id = state
        .media_seq
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    state
        .pending_media
        .lock()
        .expect("pending_media poisoned")
        .insert(
            request_id,
            PendingMedia {
                webview_label: label.to_string(),
                page_id: page_id.to_string(),
                origin: origin.to_string(),
                kind: kind.to_string(),
            },
        );

    let _ = app.emit(
        PERMISSION_REQUEST_EVENT,
        PermissionRequest {
            request_id,
            origin: origin.to_string(),
            kind: kind.to_string(),
        },
    );

    None
}

/// Resolve a prompted camera/microphone request: remember the decision for the
/// session and unblock the page (its `getUserMedia` proceeds or rejects). Stale
/// ids (already resolved, or webview gone) are a no-op.
#[tauri::command]
pub async fn resolve_media_permission(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    request_id: u64,
    granted: bool,
    remember: bool,
) -> Result<(), wis_core::WisError> {
    let Some(pending) = state
        .pending_media
        .lock()
        .expect("pending_media poisoned")
        .remove(&request_id)
    else {
        return Ok(());
    };

    let key = format!("{}|{}", pending.kind, pending.origin);
    if remember {
        // Persisted decision: seed the persisted cache now.
        state
            .media_permissions
            .write()
            .expect("media_permissions poisoned")
            .insert(key, granted);
    } else {
        state
            .media_permissions_session
            .write()
            .expect("media_permissions_session poisoned")
            .insert(key, granted);
    }

    if let Some(wv) = app.webviews().get(&pending.webview_label).cloned() {
        let js = format!(
            "window.__wisMediaResolve && window.__wisMediaResolve(\"{}\", {});",
            pending.page_id,
            if granted { "true" } else { "false" }
        );
        let _ = wv.eval(&js);
    }

    Ok(())
}

/// Install the consent gate + `wisMediaPermission` handler on a remote webview
/// (tabs, widget panels, Peek). No-op off macOS.
pub fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
    #[cfg(target_os = "macos")]
    imp::install(webview, webview.label().to_string());
    #[cfg(not(target_os = "macos"))]
    let _ = webview;
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

    use super::{handle, GATE_JS};

    struct HandlerIvars {
        label: String,
    }

    declare_class!(
        struct MediaPermissionHandler;

        unsafe impl ClassType for MediaPermissionHandler {
            type Super = NSObject;
            type Mutability = mutability::InteriorMutable;
            const NAME: &'static str = "WisMediaPermissionHandler";
        }

        impl DeclaredClass for MediaPermissionHandler {
            type Ivars = HandlerIvars;
        }

        unsafe impl MediaPermissionHandler {
            #[method(userContentController:didReceiveScriptMessage:)]
            fn did_receive(&self, _ucc: *mut AnyObject, message: *mut AnyObject) {
                let label = self.ivars().label.clone();
                unsafe { handle_message(&label, message) }
            }
        }
    );

    impl MediaPermissionHandler {
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

        let body: *mut AnyObject = msg_send![message, body];
        if body.is_null() {
            return;
        }

        let is_dict: bool = msg_send![body, isKindOfClass: class!(NSDictionary)];
        if !is_dict {
            return;
        }

        // `id` is a page-generated counter; restrict to digits so it is safe to
        // splice into the resolve-callback string.
        let page_id = dict_string(body, "id").unwrap_or_default();
        if page_id.is_empty() || !page_id.bytes().all(|b| b.is_ascii_digit()) {
            return;
        }

        let origin = dict_string(body, "origin").unwrap_or_default();
        let kind = dict_string(body, "kind").unwrap_or_default();

        // A remembered decision resolves immediately in the asking frame; an
        // unknown one is deferred to the prompt overlay (handle emits).
        let Some(granted) = handle(label, &page_id, &origin, &kind) else {
            return;
        };

        let webview: *mut AnyObject = msg_send![message, webView];
        if webview.is_null() {
            return;
        }

        let frame_info: *mut AnyObject = msg_send![message, frameInfo];
        let page_world: *mut AnyObject = msg_send![class!(WKContentWorld), pageWorld];
        let js = ns_string(&format!(
            "window.__wisMediaResolve && window.__wisMediaResolve(\"{page_id}\", {});",
            if granted { "true" } else { "false" }
        ));

        let nil: *mut AnyObject = std::ptr::null_mut();
        let _: () = msg_send![
            webview,
            evaluateJavaScript: js,
            inFrame: frame_info,
            inContentWorld: page_world,
            completionHandler: nil,
        ];
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

            // Gate script: document start, all frames, so the override is in
            // place before any page script can capture getUserMedia.
            let src = ns_string(GATE_JS);
            let script: *mut AnyObject = msg_send![class!(WKUserScript), alloc];
            let injection_time: isize = 0; // WKUserScriptInjectionTimeAtDocumentStart
            let script: *mut AnyObject = msg_send![
                script,
                initWithSource: src,
                injectionTime: injection_time,
                forMainFrameOnly: false,
            ];

            let _: () = msg_send![controller, addUserScript: script];
            let _: () = msg_send![script, release];

            let handler = MediaPermissionHandler::new(label);
            let name = ns_string("wisMediaPermission");
            let _: () = msg_send![controller, addScriptMessageHandler: &*handler, name: name];
        });
    }
}
