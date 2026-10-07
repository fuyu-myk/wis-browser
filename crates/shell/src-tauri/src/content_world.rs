//! The JS world Wis's own page scripts run in.
//!
//! On macOS that is a `WKContentWorld` of its own: it shares the page's DOM
//! but not its globals, so the page can neither call the scripts nor post to
//! their handlers. Elsewhere it is still the page's world.

use tauri::webview::Webview;
use tauri::Runtime;

/// Run `js` in the main frame, in the world the scripts live in.
pub fn eval<R: Runtime>(webview: &Webview<R>, js: impl Into<String>) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    {
        let js = js.into();
        webview.with_webview(move |w| unsafe {
            let wk = w.inner() as *mut objc2::runtime::AnyObject;
            if !wk.is_null() {
                macos::run(wk, &js);
            }
        })
    }

    #[cfg(not(target_os = "macos"))]
    webview.eval(js)
}

/// `source`, run at most once per document under `key`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn once(key: &str, source: &str) -> String {
    format!("if (!window.__wisOnce_{key}) {{\nwindow.__wisOnce_{key} = true;\n{source}\n}}")
}

#[cfg(target_os = "macos")]
pub mod macos {
    use std::ffi::CString;

    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send, Message};

    const AT_DOCUMENT_START: isize = 0;

    unsafe fn ns_string(s: &str) -> *mut AnyObject {
        let c = CString::new(s).unwrap_or_default();
        msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()]
    }

    pub unsafe fn world() -> *mut AnyObject {
        msg_send![class!(WKContentWorld), worldWithName: ns_string("wis")]
    }

    /// Add `source` to every document from here on, and run it in the current
    /// one, which a document already under way when the tab was built missed.
    pub unsafe fn install_script(
        wk: *mut AnyObject,
        controller: *mut AnyObject,
        key: &str,
        source: &str,
        main_frame_only: bool,
    ) {
        let source = super::once(key, source);
        let script: *mut AnyObject = msg_send![class!(WKUserScript), alloc];
        let script: *mut AnyObject = msg_send![
            script,
            initWithSource: ns_string(&source),
            injectionTime: AT_DOCUMENT_START,
            forMainFrameOnly: main_frame_only,
            inContentWorld: world(),
        ];
        let _: () = msg_send![controller, addUserScript: script];
        let _: () = msg_send![script, release];
        run(wk, &source);
    }

    pub unsafe fn add_handler<T: Message>(controller: *mut AnyObject, handler: &T, name: &str) {
        let _: () = msg_send![
            controller,
            addScriptMessageHandler: handler,
            contentWorld: world(),
            name: ns_string(name),
        ];
    }

    pub unsafe fn run(wk: *mut AnyObject, source: &str) {
        run_in_frame(wk, std::ptr::null_mut(), source);
    }

    /// Run `source` in `frame`, or the main frame when it is null.
    pub unsafe fn run_in_frame(wk: *mut AnyObject, frame: *mut AnyObject, source: &str) {
        let nil: *mut AnyObject = std::ptr::null_mut();
        let _: () = msg_send![
            wk,
            evaluateJavaScript: ns_string(source),
            inFrame: frame,
            inContentWorld: world(),
            completionHandler: nil,
        ];
    }

    #[cfg(test)]
    mod tests {
        use objc2::runtime::{AnyObject, Bool};
        use objc2::{class, sel};

        type Obj = *mut AnyObject;

        /// A wrong message type aborts at runtime only. WebKit will not start
        /// off the main thread, so check each send against its method instead.
        #[test]
        fn every_send_matches_its_method() {
            class!(WKContentWorld)
                .metaclass()
                .verify_sel::<(Obj,), Obj>(sel!(worldWithName:))
                .unwrap();
            class!(WKUserScript)
                .verify_sel::<(Obj, isize, Bool, Obj), Obj>(sel!(
                    initWithSource:injectionTime:forMainFrameOnly:inContentWorld:
                ))
                .unwrap();
            let controller = class!(WKUserContentController);
            controller
                .verify_sel::<(Obj,), ()>(sel!(addUserScript:))
                .unwrap();
            controller
                .verify_sel::<(Obj, Obj, Obj), ()>(sel!(addScriptMessageHandler:contentWorld:name:))
                .unwrap();
            class!(WKWebView)
                .verify_sel::<(Obj, Obj, Obj, Obj), ()>(sel!(
                    evaluateJavaScript:inFrame:inContentWorld:completionHandler:
                ))
                .unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_guarded_script_runs_its_source_once_per_key() {
        let guarded = super::once("wisPeek", "(function () {})();");
        assert!(guarded.starts_with("if (!window.__wisOnce_wisPeek) {"));
        assert!(guarded.contains("(function () {})();"));
    }
}
