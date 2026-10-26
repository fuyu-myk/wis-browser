//! The JS world Wis's own page scripts run in.
//!
//! On macOS and Linux that is a script world of its own, named `wis`: it shares
//! the page's DOM but not its globals, so the page can neither call the scripts
//! nor post to their handlers. On Windows it is still the page's world.

#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::collections::HashMap;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::sync::Mutex;

use tauri::webview::Webview;
use tauri::Runtime;

#[cfg(any(target_os = "macos", target_os = "linux"))]
const WORLD: &str = "wis";

/// The scripts each webview installed, by label, until its first page loads.
#[cfg(any(target_os = "macos", target_os = "linux"))]
static PENDING: Mutex<Option<HashMap<String, Vec<String>>>> = Mutex::new(None);

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

    #[cfg(target_os = "linux")]
    {
        let js = js.into();
        webview.with_webview(move |w| linux::run(&w.inner(), &js))
    }

    #[cfg(windows)]
    webview.eval(js)
}

/// Run `js`, which holds a secret, as [`eval`] does, wiping Wis's copy once it
/// has run. The engine's own copy of the source can't be wiped.
pub fn eval_secret<R: Runtime>(
    webview: &Webview<R>,
    js: zeroize::Zeroizing<String>,
) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    {
        webview.with_webview(move |w| unsafe {
            let wk = w.inner() as *mut objc2::runtime::AnyObject;
            if !wk.is_null() {
                macos::run(wk, &js);
            }
        })
    }

    #[cfg(target_os = "linux")]
    {
        webview.with_webview(move |w| linux::run(&w.inner(), &js))
    }

    #[cfg(windows)]
    webview.eval(js.as_str())
}

/// Run the scripts `webview` installed again, once, when its first page has
/// loaded. That page can start in a fresh web process before the scripts reach
/// it, and the evaluate at install went to the document it replaced.
pub fn cover_first_document<R: Runtime>(webview: &Webview<R>) {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let sources = PENDING
            .lock()
            .expect("content world pending poisoned")
            .as_mut()
            .and_then(|pending| pending.remove(webview.label()));
        for source in sources.into_iter().flatten() {
            let _ = eval(webview, source);
        }
    }

    #[cfg(windows)]
    let _ = webview;
}

/// Drop what a webview closed before its first page loaded left behind.
pub fn forget(label: &str) {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    if let Some(pending) = PENDING
        .lock()
        .expect("content world pending poisoned")
        .as_mut()
    {
        pending.remove(label);
    }

    #[cfg(windows)]
    let _ = label;
}

/// `source`, run at most once per document under `key`.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
fn once(key: &str, source: &str) -> String {
    format!("if (!window.__wisOnce_{key}) {{\nwindow.__wisOnce_{key} = true;\n{source}\n}}")
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn remember(label: &str, source: &str) {
    PENDING
        .lock()
        .expect("content world pending poisoned")
        .get_or_insert_with(HashMap::new)
        .entry(label.to_string())
        .or_default()
        .push(source.to_string());
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

    unsafe fn world() -> *mut AnyObject {
        msg_send![class!(WKContentWorld), worldWithName: ns_string(super::WORLD)]
    }

    /// Add `source` to every document of webview `label` from here on, and
    /// run it in the current one, which a document already under way when the
    /// tab was built missed.
    pub unsafe fn install_script(
        wk: *mut AnyObject,
        controller: *mut AnyObject,
        label: &str,
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
        if script.is_null() {
            return;
        }

        let _: () = msg_send![controller, addUserScript: script];
        let _: () = msg_send![script, release];
        run(wk, &source);
        super::remember(label, &source);
    }

    pub unsafe fn add_handler<T: Message>(controller: *mut AnyObject, handler: &T, name: &str) {
        let _: () = msg_send![
            controller,
            addScriptMessageHandler: handler,
            contentWorld: world(),
            name: ns_string(name),
        ];
    }

    /// Run `source` in the main frame.
    pub unsafe fn run(wk: *mut AnyObject, source: &str) {
        let nil: *mut AnyObject = std::ptr::null_mut();
        let _: () = msg_send![
            wk,
            evaluateJavaScript: ns_string(source),
            inFrame: nil,
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

#[cfg(target_os = "linux")]
pub mod linux {
    use webkit2gtk::{
        UserContentInjectedFrames, UserContentManager, UserContentManagerExt, UserScript,
        UserScriptInjectionTime, WebView, WebViewExt,
    };

    /// Add `source` to every document of webview `label` from here on, and
    /// run it in the current one.
    pub fn install_script(
        wv: &WebView,
        ucm: &UserContentManager,
        label: &str,
        key: &str,
        source: &str,
        main_frame_only: bool,
    ) {
        let source = super::once(key, source);
        let frames = if main_frame_only {
            UserContentInjectedFrames::TopFrame
        } else {
            UserContentInjectedFrames::AllFrames
        };
        let script = UserScript::for_world(
            &source,
            frames,
            UserScriptInjectionTime::Start,
            super::WORLD,
            &[],
            &[],
        );

        ucm.add_script(&script);
        run(wv, &source);
        super::remember(label, &source);
    }

    /// Register `name` as a handler only scripts in Wis's world can post to.
    pub fn add_handler(ucm: &UserContentManager, name: &str) -> bool {
        ucm.register_script_message_handler_in_world(name, super::WORLD)
    }

    /// Run `source` in the main frame.
    #[allow(deprecated)]
    pub fn run(wv: &WebView, source: &str) {
        wv.run_javascript_in_world(source, super::WORLD, None::<&gtk::gio::Cancellable>, |_| {});
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
