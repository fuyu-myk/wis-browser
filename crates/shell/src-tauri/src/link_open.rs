//! Cmd/middle-click a link -> open it in a new **background** tab.
//!
//! A builder-level page script intercepts `Cmd`/`Ctrl`+click and middle-click on
//! `<a href>` (Opt+click stays peek's), `preventDefault`s WebKit's open-in-new-
//! window, and posts the href to the `wisOpenInTab` message handler, which
//! re-emits it to the chrome. The focused window creates a background tab in its
//! own workspace. Engine new-window requests (`window.open` / `target=_blank`)
//! flow through `on_new_window` instead, which routes plain navigations here as
//! [`OPEN_TAB_FOREGROUND_EVENT`] and keeps geometry-carrying popups native
//! (see `create_tab_webview`).

use std::sync::OnceLock;

use tauri::AppHandle;

/// Event emitted to the chrome with the link to open in a background tab.
const OPEN_IN_TAB_EVENT: &str = "wis-open-in-tab";

/// Event emitted to the chrome with a link to open as a **foreground** tab.
pub const OPEN_TAB_FOREGROUND_EVENT: &str = "wis-open-in-tab-fg";

/// App handle so the native message handler can reach the event bus.
static APP: OnceLock<AppHandle> = OnceLock::new();

/// Record the app handle once during setup so the `wisOpenInTab` handler can emit.
pub fn set_app_handle(app: AppHandle) {
    let _ = APP.set(app);
}

/// Emit the open-in-tab event for a parsed href (the Windows dispatcher's entry;
/// macOS reaches the same emit through its `wisOpenInTab` script-message handler).
#[cfg(windows)]
pub(crate) fn dispatch_message(href: &str) {
    use tauri::Emitter;

    if href.is_empty() {
        return;
    }

    if let Some(app) = APP.get() {
        let _ = app.emit(OPEN_IN_TAB_EVENT, href);
    }
}

/// Injected page script (tab webviews): `Cmd`/`Ctrl`+click or middle-click a
/// link to open it in a background tab instead of navigating / popping a window.
const OPEN_IN_TAB_JS: &str = r#"(function () {
  if (window.__wisOpenInTab) return;
  window.__wisOpenInTab = true;
  function hrefAt(e) {
    var t = e.target;
    var el = (t && t.nodeType === 1) ? t : (t && t.parentElement);
    var a = el && el.closest ? el.closest('a[href]') : null;
    if (!a) return null;
    var href = a.href || '';
    if (!/^https?:/i.test(href)) return null;
    return href;
  }
  function post(href) {
    try {
      if (window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.wisOpenInTab) {
        window.webkit.messageHandlers.wisOpenInTab.postMessage(href);
      }
    } catch (err) {}
  }
  document.addEventListener('click', function (e) {
    if (e.button !== 0 || e.altKey || !(e.metaKey || e.ctrlKey)) return;
    var href = hrefAt(e);
    if (!href) return;
    e.preventDefault();
    e.stopPropagation();
    post(href);
  }, true);
  document.addEventListener('auxclick', function (e) {
    if (e.button !== 1) return;
    var href = hrefAt(e);
    if (!href) return;
    e.preventDefault();
    e.stopPropagation();
    post(href);
  }, true);
  // Middle-button mousedown autoscroll / pre-empts the default new-window.
  document.addEventListener('mousedown', function (e) {
    if (e.button === 1 && hrefAt(e)) e.preventDefault();
  }, true);
})();"#;

/// The trigger, exposed so tab webviews can bake it in as a builder-level
/// initialization script (runs at document start, before the first document).
pub fn page_script() -> &'static str {
    OPEN_IN_TAB_JS
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
    use tauri::Emitter;

    use super::{APP, OPEN_IN_TAB_EVENT};

    declare_class!(
        struct OpenInTabHandler;

        unsafe impl ClassType for OpenInTabHandler {
            type Super = NSObject;
            type Mutability = mutability::InteriorMutable;
            const NAME: &'static str = "WisOpenInTabHandler";
        }

        impl DeclaredClass for OpenInTabHandler {
            type Ivars = ();
        }

        unsafe impl OpenInTabHandler {
            #[method(userContentController:didReceiveScriptMessage:)]
            fn did_receive(&self, _ucc: *mut AnyObject, message: *mut AnyObject) {
                unsafe { handle_message(message) }
            }
        }
    );

    impl OpenInTabHandler {
        fn new() -> Id<Self> {
            unsafe { msg_send_id![Self::alloc(), init] }
        }
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

    unsafe fn handle_message(message: *mut AnyObject) {
        if message.is_null() {
            return;
        }

        let body: *mut AnyObject = msg_send![message, body];
        let Some(href) = nsstring_to_string(body) else {
            return;
        };

        if href.is_empty() {
            return;
        }

        if let Some(app) = APP.get() {
            let _ = app.emit(OPEN_IN_TAB_EVENT, href);
        }
    }

    /// Install the `wisOpenInTab` message handler on a tab webview. The trigger
    /// script rides the builder (see [`super::page_script`]); message handlers
    /// apply retroactively, so an already-loaded page is covered.
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

            let handler = OpenInTabHandler::new();
            let c = CString::new("wisOpenInTab").unwrap_or_default();
            let name: *mut AnyObject =
                msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()];
            let _: () = msg_send![controller, addScriptMessageHandler: &*handler, name: name];
        });
    }
}

/// Install the Cmd/middle-click open-in-tab channel on a tab webview. No-op off macOS.
pub fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
    #[cfg(target_os = "macos")]
    imp::install(webview);

    #[cfg(not(target_os = "macos"))]
    let _ = webview;
}
