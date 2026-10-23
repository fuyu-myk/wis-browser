//! Global Privacy Control's page-side half: `navigator.globalPrivacyControl`,
//! defined in every frame before page scripts run. The header half lives in
//! the proxy (`wis_network` `proxy/upstream.rs`), which can only stamp the
//! requests it forwards itself, so the property is the signal every site sees.
//!
//! The property is baked into each remote webview at creation with the
//! setting of that moment, and a document-start script re-runs on every
//! later document with that same value.
//!
//! A toggle does two things per live remote webview:
//!
//! 1. Flips the top frame of the current document through
//!    `window.__wisGpcSet`.
//! 2. Installs one more document-start script that flips every later
//!    document in every frame. The flip scripts run after the baked one
//!    and accumulate one per toggle, the last one winning.

/// The property definition; `%ON%` is replaced by the setting at creation.
const GPC_JS: &str = r#"(function () {
  var on = %ON%;
  var define = function (target) {
    Object.defineProperty(target, 'globalPrivacyControl', {
      get: function () { return on; },
      configurable: true,
      enumerable: true
    });
  };
  try { define(Navigator.prototype); } catch (e) {
    try { define(navigator); } catch (e2) {}
  }
  Object.defineProperty(window, '__wisGpcSet', {
    value: function (v) { on = !!v; },
    configurable: true
  });
})();"#;

/// The document-start script for a remote webview, baked with the setting.
pub fn page_script(enabled: bool) -> String {
    GPC_JS.replace("%ON%", if enabled { "true" } else { "false" })
}

/// The one-liner that moves an already-defined property to `enabled`.
fn flip_script(enabled: bool) -> String {
    format!("window.__wisGpcSet && window.__wisGpcSet({enabled});")
}

/// Move the signal to `enabled` on every live remote webview: the current
/// top frame now, and every later document through a document-start script.
pub fn apply_to_all_remote(app: &tauri::AppHandle, enabled: bool) {
    use tauri::Manager;

    let js = flip_script(enabled);
    for (label, webview) in app.webviews() {
        if crate::platform::webview_prefs::is_remote(&label) {
            if let Err(err) = webview.eval(&js) {
                log::debug!("gpc: flip on {label} failed: {err}");
            }
            install_flip(&webview, js.clone());
        }
    }
}

/// Add `js` as a document-start, all-frames user script on a live webview.
#[cfg(target_os = "macos")]
fn install_flip<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>, js: String) {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};

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

        let Ok(c) = std::ffi::CString::new(js) else {
            return;
        };
        let src: *mut AnyObject = msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()];
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
    });
}

#[cfg(windows)]
fn install_flip<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>, js: String) {
    use webview2_com::AddScriptToExecuteOnDocumentCreatedCompletedHandler;
    use windows::core::HSTRING;

    // WebView2 injects document-created scripts into every frame.
    crate::platform::win_webview2::with_core_webview2(webview, move |core| {
        let source = HSTRING::from(js.as_str());
        let handler =
            AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(|_, _| Ok(())));
        // SAFETY: on the UI thread via `with_core_webview2`; `source` outlives
        // the synchronous call, which copies the string.
        if let Err(err) = unsafe { core.AddScriptToExecuteOnDocumentCreated(&source, &handler) } {
            log::debug!("gpc: AddScriptToExecuteOnDocumentCreated failed: {err}");
        }
    });
}

#[cfg(target_os = "linux")]
fn install_flip<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>, js: String) {
    use webkit2gtk::{
        UserContentInjectedFrames, UserContentManagerExt, UserScript, UserScriptInjectionTime,
        WebViewExt,
    };

    let _ = webview.with_webview(move |w| {
        let Some(ucm) = w.inner().user_content_manager() else {
            log::debug!("gpc: webview has no user content manager");
            return;
        };
        ucm.add_script(&UserScript::new(
            &js,
            UserContentInjectedFrames::AllFrames,
            UserScriptInjectionTime::Start,
            &[],
            &[],
        ));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_script_bakes_the_setting() {
        assert!(page_script(true).contains("var on = true;"));
        assert!(page_script(false).contains("var on = false;"));
        assert!(!page_script(true).contains("%ON%"));
    }

    #[test]
    fn the_flip_guards_against_a_document_without_the_property() {
        assert_eq!(
            flip_script(false),
            "window.__wisGpcSet && window.__wisGpcSet(false);"
        );
    }
}
