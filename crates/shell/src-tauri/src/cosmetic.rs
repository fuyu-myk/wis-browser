//! Cosmetic filtering (element hiding) for tab webviews.
//!
//! Network blocking (proxy + `WKContentRuleList`) stops requests; cosmetic
//! filtering hides what is left behind: empty ad slots, placeholders and
//! overlay nags. A document-end user script collects the page's classes and
//! ids (one initial scan, then mutation-batched deltas, capped per message)
//! and posts them with the page URL to a narrow `wisCosmetic` handler. Rust
//! answers from the adblock engine: host-specific hide selectors once per
//! document, plus generic class/id selectors per scan, evaluated back into
//! the page (into the sender's frame, so iframes are covered too) as a CSS
//! append. Installation is skipped for Permissive workspaces.

use std::sync::OnceLock;

use tauri::AppHandle;

static APP: OnceLock<AppHandle> = OnceLock::new();

pub fn set_app_handle(app: AppHandle) {
    let _ = APP.set(app);
}

/// Marker prefix identifying the per-page scriptlet user script, so the
/// navigation swap can drop the previous page's payload while re-adding
/// every other user script (WebKit only offers removeAllUserScripts).
const SCRIPTLET_MARKER: &str = "/*wis-scriptlets*/";

/// Swap the document-start scriptlet payload for an upcoming navigation.
/// Called from the tab webview's `on_navigation` hook, which runs inside
/// WebKit's decide-policy on the main thread, so the new user script is in
/// place before the next document is created. Scriptlet rules (uBO
/// `+js(...)`) must run before the page's own scripts.
pub fn swap_scriptlets(app: &tauri::AppHandle, label: &str, url: &str) {
    use tauri::Manager;

    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return;
    }
    let Some(webview) = app.webviews().get(label).cloned() else {
        return;
    };

    let blocker = {
        let state = app.state::<crate::state::AppState>();
        let guard = state
            .tracker_blocker
            .read()
            .expect("tracker_blocker poisoned");
        guard.clone()
    };
    let payload = blocker.scriptlets_for_page(url);

    // An empty payload still swaps, clearing the previous site's scriptlets.
    #[cfg(target_os = "macos")]
    imp::swap_scriptlet_script(&webview, &payload);

    #[cfg(not(target_os = "macos"))]
    let _ = (webview, payload);
}

/// Injected page script: collects classes/ids and applies returned CSS.
const COSMETIC_JS: &str = r#"(function () {
  if (window.__wisCosmeticInstalled) return;
  window.__wisCosmeticInstalled = true;
  var MAX_PER_SCAN = 1500;
  var seen = {};
  var sentInitial = false;
  var style = null;
  window.__wisCosmeticApply = function (css) {
    try {
      if (!css) return;
      if (!style || !style.isConnected) {
        style = document.createElement('style');
        style.id = 'wis-cosmetic';
        (document.head || document.documentElement).appendChild(style);
      }
      style.appendChild(document.createTextNode(css + "\n"));
    } catch (e) {}
  };
  function collect() {
    var classes = [];
    var ids = [];
    try {
      var els = document.querySelectorAll('[class],[id]');
      for (var i = 0; i < els.length; i++) {
        if (classes.length >= MAX_PER_SCAN && ids.length >= MAX_PER_SCAN) break;
        var el = els[i];
        if (el.id && !seen['#' + el.id] && ids.length < MAX_PER_SCAN) {
          seen['#' + el.id] = 1;
          ids.push(el.id);
        }
        var cl = el.classList;
        for (var j = 0; j < cl.length; j++) {
          if (!seen['.' + cl[j]] && classes.length < MAX_PER_SCAN) {
            seen['.' + cl[j]] = 1;
            classes.push(cl[j]);
          }
        }
      }
    } catch (e) {}
    if (sentInitial && classes.length === 0 && ids.length === 0) return;
    try {
      if (window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.wisCosmetic) {
        window.webkit.messageHandlers.wisCosmetic.postMessage({
          href: location.href,
          initial: !sentInitial,
          classes: classes,
          ids: ids
        });
        sentInitial = true;
      }
    } catch (e) {}
  }
  collect();
  var pending = null;
  try {
    new MutationObserver(function () {
      if (pending) return;
      pending = setTimeout(function () { pending = null; collect(); }, 700);
    }).observe(document.documentElement, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ['class', 'id']
    });
  } catch (e) {}
})();"#;

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;

    use objc2::rc::Id;
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{
        class, declare_class, msg_send, msg_send_id, mutability, sel, ClassType, DeclaredClass,
    };
    use tauri::Manager;

    use super::{APP, COSMETIC_JS, SCRIPTLET_MARKER};

    declare_class!(
        struct CosmeticHandler;

        unsafe impl ClassType for CosmeticHandler {
            type Super = NSObject;
            type Mutability = mutability::InteriorMutable;
            const NAME: &'static str = "WisCosmeticHandler";
        }

        impl DeclaredClass for CosmeticHandler {
            type Ivars = ();
        }

        unsafe impl CosmeticHandler {
            #[method(userContentController:didReceiveScriptMessage:)]
            fn did_receive(&self, _ucc: *mut AnyObject, message: *mut AnyObject) {
                unsafe { handle_message(message) }
            }
        }
    );

    impl CosmeticHandler {
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

    unsafe fn dict_bool(dict: *mut AnyObject, key: &str) -> bool {
        let k = ns_string(key);
        let v: *mut AnyObject = msg_send![dict, objectForKey: k];
        if v.is_null() {
            return false;
        }

        let is_num: bool = msg_send![v, isKindOfClass: class!(NSNumber)];
        if !is_num {
            return false;
        }

        let b: bool = msg_send![v, boolValue];
        b
    }

    unsafe fn dict_string_array(dict: *mut AnyObject, key: &str, cap: usize) -> Vec<String> {
        let k = ns_string(key);
        let arr: *mut AnyObject = msg_send![dict, objectForKey: k];
        if arr.is_null() {
            return Vec::new();
        }

        let is_arr: bool = msg_send![arr, isKindOfClass: class!(NSArray)];
        if !is_arr {
            return Vec::new();
        }

        let count: usize = msg_send![arr, count];
        let mut out = Vec::with_capacity(count.min(cap));
        for i in 0..count.min(cap) {
            let item: *mut AnyObject = msg_send![arr, objectAtIndex: i];
            if let Some(s) = nsstring_to_string(item) {
                out.push(s);
            }
        }

        out
    }

    unsafe fn handle_message(message: *mut AnyObject) {
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

        let Some(href) = dict_string(body, "href") else {
            return;
        };
        if !(href.starts_with("http://") || href.starts_with("https://")) {
            return;
        }

        let initial = dict_bool(body, "initial");
        let classes = dict_string_array(body, "classes", 2000);
        let ids = dict_string_array(body, "ids", 2000);

        let Some(app) = APP.get() else {
            return;
        };

        let blocker = {
            let state = app.state::<crate::state::AppState>();
            let guard = state
                .tracker_blocker
                .read()
                .expect("tracker_blocker poisoned");
            guard.clone()
        };

        let page = blocker.cosmetic_for_page(&href);
        let mut css = String::new();
        if initial {
            css.push_str(&page.hide_css);
        }
        if !page.generichide && (!classes.is_empty() || !ids.is_empty()) {
            css.push_str(&blocker.cosmetic_generic_css(&classes, &ids, &page.exceptions));
        }
        if css.is_empty() {
            return;
        }
        let Ok(arg) = serde_json::to_string(&css) else {
            return;
        };
        let js = ns_string(&format!("window.__wisCosmeticApply({arg});"));

        // Reply into the sender's frame so iframes receive their own CSS;
        // fall back to the main frame when the frame-targeted API is absent.
        let webview: *mut AnyObject = msg_send![message, webView];
        if webview.is_null() {
            return;
        }

        let nil_block: *mut AnyObject = std::ptr::null_mut();
        let frame: *mut AnyObject = msg_send![message, frameInfo];
        let in_frame_sel = sel!(evaluateJavaScript:inFrame:inContentWorld:completionHandler:);
        let supports_frames: bool = msg_send![webview, respondsToSelector: in_frame_sel];
        if supports_frames && !frame.is_null() {
            let world: *mut AnyObject = msg_send![class!(WKContentWorld), pageWorld];
            let _: () = msg_send![
                webview,
                evaluateJavaScript: js,
                inFrame: frame,
                inContentWorld: world,
                completionHandler: nil_block
            ];
        } else {
            let _: () = msg_send![webview, evaluateJavaScript: js, completionHandler: nil_block];
        }
    }

    /// Replace the marked scriptlet user script: re-add every other script,
    /// then append the new payload (document start, main frame only; the
    /// payload was computed for the main document's URL).
    pub fn swap_scriptlet_script<R: tauri::Runtime>(
        webview: &tauri::webview::Webview<R>,
        payload: &str,
    ) {
        let source = if payload.is_empty() {
            String::new()
        } else {
            format!("{SCRIPTLET_MARKER}\ntry{{{payload}}}catch(e){{}}")
        };
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

            let existing: *mut AnyObject = msg_send![controller, userScripts];
            let count: usize = if existing.is_null() {
                0
            } else {
                msg_send![existing, count]
            };
            let marker = ns_string(SCRIPTLET_MARKER);
            let mut keep: Vec<*mut AnyObject> = Vec::with_capacity(count);
            for i in 0..count {
                let script: *mut AnyObject = msg_send![existing, objectAtIndex: i];
                let src: *mut AnyObject = msg_send![script, source];
                let marked: bool = if src.is_null() {
                    false
                } else {
                    msg_send![src, hasPrefix: marker]
                };
                if !marked {
                    let _: *mut AnyObject = msg_send![script, retain];
                    keep.push(script);
                }
            }
            let _: () = msg_send![controller, removeAllUserScripts];
            for script in keep {
                let _: () = msg_send![controller, addUserScript: script];
                let _: () = msg_send![script, release];
            }

            if !source.is_empty() {
                let src = ns_string(&source);
                let script: *mut AnyObject = msg_send![class!(WKUserScript), alloc];
                let injection_time: isize = 0; // WKUserScriptInjectionTimeAtDocumentStart
                let script: *mut AnyObject = msg_send![
                    script,
                    initWithSource: src,
                    injectionTime: injection_time,
                    forMainFrameOnly: true,
                ];
                let _: () = msg_send![controller, addUserScript: script];
                let _: () = msg_send![script, release];
            }
        });
    }

    /// Install the cosmetic-filtering user script + message handler.
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

            let src = ns_string(COSMETIC_JS);
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

            let handler = CosmeticHandler::new();
            let name = ns_string("wisCosmetic");
            let _: () = msg_send![controller, addScriptMessageHandler: &*handler, name: name];
        });
    }
}

/// Install the cosmetic-filtering channel on a tab webview. No-op off macOS.
pub fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
    #[cfg(target_os = "macos")]
    imp::install(webview);

    #[cfg(not(target_os = "macos"))]
    let _ = webview;
}
