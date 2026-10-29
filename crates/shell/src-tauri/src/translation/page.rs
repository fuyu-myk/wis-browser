//! The page side of translation: `page.js`, run in Wis's content world, and
//! the `wisTranslate` handler its messages arrive on. Only Wis's world can
//! post to the handler, and only the main frame's messages count.

use super::message::{parse_message, Run};

const PAGE_JS: &str = include_str!("page.js");

const HANDLER: &str = "wisTranslate";

pub const REVERT_SCRIPT: &str = "window.__wisTranslate && window.__wisTranslate.revert();";

/// Defines the script if this document lacks it, then starts generation `gen`.
pub fn start_script(gen: u64) -> String {
    format!("{PAGE_JS}\nwindow.__wisTranslate.start({gen});")
}

pub fn go_script(gen: u64) -> String {
    format!("window.__wisTranslate && window.__wisTranslate.go({gen});")
}

/// Puts one segment's translation in place. The runs go in as JSON, a JS
/// literal, and the page script sets them as text nodes.
pub fn apply_script(gen: u64, id: u32, runs: &[Run]) -> String {
    let runs = serde_json::to_string(runs).expect("runs serialise");
    format!("window.__wisTranslate && window.__wisTranslate.apply({gen}, {id}, {runs});")
}

fn dispatch(label: &str, body: &str) {
    match parse_message(body) {
        Some(message) => super::session::on_message(label, message),
        None => log::debug!("dropped a malformed translation message from {label}"),
    }
}

pub fn install(webview: &tauri::Webview) {
    imp::install(webview, webview.label().to_string());
}

mod imp {
    use std::ffi::CStr;
    use std::os::raw::c_char;

    use objc2::rc::Id;
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{
        class, declare_class, msg_send, msg_send_id, mutability, ClassType, DeclaredClass,
    };

    use crate::platform::content_world::macos as content_world;

    struct HandlerIvars {
        label: String,
    }

    declare_class!(
        struct TranslateHandler;

        unsafe impl ClassType for TranslateHandler {
            type Super = NSObject;
            type Mutability = mutability::InteriorMutable;
            const NAME: &'static str = "WisTranslateHandler";
        }

        impl DeclaredClass for TranslateHandler {
            type Ivars = HandlerIvars;
        }

        unsafe impl TranslateHandler {
            #[method(userContentController:didReceiveScriptMessage:)]
            fn did_receive(&self, _ucc: *mut AnyObject, message: *mut AnyObject) {
                let label = self.ivars().label.clone();
                // SAFETY: WebKit passes a live WKScriptMessage for this call.
                unsafe { handle_message(&label, message) }
            }
        }
    );

    impl TranslateHandler {
        fn new(label: String) -> Id<Self> {
            let this = Self::alloc().set_ivars(HandlerIvars { label });
            // SAFETY: NSObject's `init` on a freshly allocated instance.
            unsafe { msg_send_id![super(this), init] }
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

    unsafe fn handle_message(label: &str, message: *mut AnyObject) {
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
        let body: *mut AnyObject = msg_send![message, body];
        if let Some(body) = nsstring_to_string(body) {
            super::dispatch(label, &body);
        }
    }

    pub fn install(webview: &tauri::Webview, label: String) {
        // SAFETY: wry hands over the live WKWebView on the main thread, and
        // each send matches its method (`every_send_matches_its_method`).
        let installed = webview.with_webview(move |w| unsafe {
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
            let handler = TranslateHandler::new(label);
            content_world::add_handler(controller, &*handler, super::HANDLER);
        });
        if let Err(err) = installed {
            log::warn!("translation handler not installed: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use objc2::{class, sel};

    type Obj = *mut AnyObject;

    /// A wrong message type aborts at runtime only.
    #[test]
    fn every_send_matches_its_method() {
        let message = class!(WKScriptMessage);
        message.verify_sel::<(), Obj>(sel!(frameInfo)).unwrap();
        message.verify_sel::<(), Obj>(sel!(body)).unwrap();
        class!(WKFrameInfo)
            .verify_sel::<(), Bool>(sel!(isMainFrame))
            .unwrap();
        class!(NSString)
            .verify_sel::<(*const AnyClass,), Bool>(sel!(isKindOfClass:))
            .unwrap();
        class!(NSString)
            .verify_sel::<(), *const std::os::raw::c_char>(sel!(UTF8String))
            .unwrap();
        class!(WKWebView)
            .verify_sel::<(), Obj>(sel!(configuration))
            .unwrap();
        class!(WKWebViewConfiguration)
            .verify_sel::<(), Obj>(sel!(userContentController))
            .unwrap();
    }

    #[test]
    fn a_translation_goes_into_the_page_as_a_json_literal() {
        let runs = [
            Run {
                text: "Say \"hi\" </script>".into(),
                tag: None,
            },
            Run {
                text: "here".into(),
                tag: Some(0),
            },
        ];
        assert_eq!(
            apply_script(2, 7, &runs),
            r#"window.__wisTranslate && window.__wisTranslate.apply(2, 7, [{"text":"Say \"hi\" </script>"},{"text":"here","tag":0}]);"#
        );
    }

    #[test]
    fn starting_defines_the_script_before_calling_it() {
        let script = start_script(4);
        assert!(script.starts_with(PAGE_JS));
        assert!(script.ends_with("window.__wisTranslate.start(4);"));
    }
}
