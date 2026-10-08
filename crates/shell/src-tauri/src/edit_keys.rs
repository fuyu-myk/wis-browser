//! Native fallback for editing keys that stay page-first on macOS.
//!
//! The vendored wry patch (vendor/wry/WIS-PATCH.md) forwards Cmd+Z /
//! Cmd+Shift+Z / Cmd+A to the focused page first, so web apps with their own
//! shortcut handling (Google Docs) see the raw key event instead of the
//! menu's `undo:`/`selectAll:` selectors. WebKit's page-declined resend never
//! reaches the menu in this embedding, so a page that does not consume the
//! key would silently lose native undo/redo/select-all (plain text fields).
//! A page script watches those combos and, one macrotask after the dispatch
//! finishes unconsumed, posts the command to `wisEditKey`; the handler
//! replays the standard editing selector down the responder chain, exactly
//! what the menu accelerator would have done.

/// Injected page script (tab webviews): report unconsumed editing combos.
/// The `setTimeout` defers the `defaultPrevented` check until after every
/// page handler (including ones registered later than this one) has run.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const EDIT_KEYS_JS: &str = r#"(function () {
  if (window.__wisEditKeys) return;
  window.__wisEditKeys = true;
  function post(cmd) {
    try {
      if (window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.wisEditKey) {
        window.webkit.messageHandlers.wisEditKey.postMessage(cmd);
      }
    } catch (err) {}
  }
  window.addEventListener('keydown', function (e) {
    if (!e.isTrusted || !e.metaKey || e.ctrlKey || e.altKey) return;
    var k = (e.key || '').toLowerCase();
    var cmd = null;
    if (k === 'z') cmd = e.shiftKey ? 'redo' : 'undo';
    else if (k === 'a' && !e.shiftKey) cmd = 'selectAll';
    if (!cmd) return;
    setTimeout(function () {
      if (!e.defaultPrevented) post(cmd);
    }, 0);
  }, false);
})();"#;

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::CStr;
    use std::os::raw::c_char;

    use objc2::rc::Id;
    use objc2::runtime::{AnyObject, NSObject, Sel};
    use objc2::{
        class, declare_class, msg_send, msg_send_id, mutability, sel, ClassType, DeclaredClass,
    };

    use super::EDIT_KEYS_JS;
    use crate::content_world::macos as content_world;

    declare_class!(
        struct EditKeyHandler;

        unsafe impl ClassType for EditKeyHandler {
            type Super = NSObject;
            type Mutability = mutability::InteriorMutable;
            const NAME: &'static str = "WisEditKeyHandler";
        }

        impl DeclaredClass for EditKeyHandler {
            type Ivars = ();
        }

        unsafe impl EditKeyHandler {
            #[method(userContentController:didReceiveScriptMessage:)]
            fn did_receive(&self, _ucc: *mut AnyObject, message: *mut AnyObject) {
                unsafe { handle_message(message) }
            }
        }
    );

    impl EditKeyHandler {
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
        let Some(cmd) = nsstring_to_string(body) else {
            return;
        };

        let action: Sel = match cmd.as_str() {
            "undo" => sel!(undo:),
            "redo" => sel!(redo:),
            "selectAll" => sel!(selectAll:),
            _ => return,
        };

        // Replay down the responder chain (first responder is the webview the
        // message came from; the user was just typing there). Runs on the main
        // thread: WKScriptMessageHandler callbacks are delivered there.
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        if app.is_null() {
            return;
        }
        let _: bool = msg_send![
            app,
            sendAction: action,
            to: std::ptr::null_mut::<AnyObject>(),
            from: std::ptr::null_mut::<AnyObject>()
        ];
    }

    /// Install the watcher and its `wisEditKey` handler on a webview.
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

            let handler = EditKeyHandler::new();
            content_world::add_handler(controller, &*handler, "wisEditKey");
            content_world::install_script(wk, controller, &label, "wisEditKey", EDIT_KEYS_JS, true);
        });
    }
}

/// Install the unconsumed-editing-key channel on a tab webview. No-op off macOS.
pub fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
    #[cfg(target_os = "macos")]
    imp::install(webview);

    #[cfg(not(target_os = "macos"))]
    let _ = webview;
}
