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
//! 2. Replaces the webview's document-start flip script, which runs after the
//!    baked one and flips every later document in every frame. Each webview
//!    keeps one: macOS removes the one before by its marker, Linux keeps its
//!    handle on the web view and Windows the id WebView2 gave it.

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

/// Starts every flip script, so a toggle can find the one before it.
const FLIP_MARKER: &str = "/*wis-gpc-flip*/";

/// The one-liner that moves an already-defined property to `enabled`.
fn flip_script(enabled: bool) -> String {
    format!("{FLIP_MARKER}window.__wisGpcSet && window.__wisGpcSet({enabled});")
}

/// A webview's latest flip script: which install it was, and the id WebView2
/// gave it once added.
#[cfg(windows)]
struct Flip {
    install: u64,
    id: Option<String>,
}

/// Each webview's latest flip script, by label.
#[cfg(windows)]
static FLIPS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, Flip>>> =
    std::sync::LazyLock::new(Default::default);

#[cfg(windows)]
static NEXT_FLIP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Drop a closing webview's flip id.
#[cfg(windows)]
pub fn forget(label: &str) {
    FLIPS.lock().expect("FLIPS poisoned").remove(label);
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

/// Make `js` the live webview's one document-start, all-frames flip script.
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
        crate::platform::user_scripts::remove_marked(controller, FLIP_MARKER);

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

    let label = webview.label().to_string();
    // WebView2 injects document-created scripts into every frame.
    crate::platform::win_webview2::with_core_webview2(webview, move |core| {
        let install = NEXT_FLIP.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let previous = FLIPS
            .lock()
            .expect("FLIPS poisoned")
            .insert(label.clone(), Flip { install, id: None })
            .and_then(|flip| flip.id);
        if let Some(id) = previous {
            remove_flip(core, &id);
        }

        let source = HSTRING::from(js.as_str());
        let added_to = core.clone();
        let handler = AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(
            move |result, id| {
                if result.is_err() {
                    return Ok(());
                }
                let stale = match FLIPS.lock().expect("FLIPS poisoned").get_mut(&label) {
                    Some(flip) if flip.install == install => {
                        flip.id = Some(id);
                        None
                    }
                    _ => Some(id),
                };
                if let Some(id) = stale {
                    remove_flip(&added_to, &id);
                }
                Ok(())
            },
        ));
        // SAFETY: on the UI thread via `with_core_webview2`; `source` outlives
        // the synchronous call, which copies the string.
        if let Err(err) = unsafe { core.AddScriptToExecuteOnDocumentCreated(&source, &handler) } {
            log::debug!("gpc: AddScriptToExecuteOnDocumentCreated failed: {err}");
        }
    });
}

#[cfg(windows)]
fn remove_flip(core: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2, id: &str) {
    use windows::core::HSTRING;

    // SAFETY: on the UI thread, inside `with_core_webview2` or a WebView2
    // completion handler; WebView2 copies the id before returning.
    if let Err(err) = unsafe { core.RemoveScriptToExecuteOnDocumentCreated(&HSTRING::from(id)) } {
        log::debug!("gpc: RemoveScriptToExecuteOnDocumentCreated failed: {err}");
    }
}

/// The web view's data key for its flip script.
#[cfg(target_os = "linux")]
const FLIP_KEY: &str = "wis-gpc-flip";

#[cfg(target_os = "linux")]
fn install_flip<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>, js: String) {
    use gtk::glib::object::ObjectExt;
    use webkit2gtk::{
        UserContentInjectedFrames, UserContentManagerExt, UserScript, UserScriptInjectionTime,
        WebViewExt,
    };

    let _ = webview.with_webview(move |w| {
        let view = w.inner();
        let Some(ucm) = view.user_content_manager() else {
            log::debug!("gpc: webview has no user content manager");
            return;
        };
        // SAFETY: `FLIP_KEY` is only ever set to a `UserScript`, here.
        if let Some(previous) = unsafe { view.steal_data::<UserScript>(FLIP_KEY) } {
            ucm.remove_script(&previous);
        }

        let script = UserScript::new(
            &js,
            UserContentInjectedFrames::AllFrames,
            UserScriptInjectionTime::Start,
            &[],
            &[],
        );
        ucm.add_script(&script);
        // SAFETY: as above.
        unsafe { view.set_data(FLIP_KEY, script) };
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
        assert!(flip_script(false).ends_with("window.__wisGpcSet && window.__wisGpcSet(false);"));
    }

    #[test]
    fn the_flip_is_marked_so_the_next_toggle_can_find_it() {
        assert!(flip_script(true).starts_with(FLIP_MARKER));
        assert!(!FLIP_MARKER.starts_with(crate::protection::cosmetic::INJECT_MARKER_PREFIX));
    }
}
