//! Open the web inspector on a tab, on the node that was right-clicked.
//!
//! Tauri compiles `Webview::open_devtools` out past `debug_assertions` unless
//! `tauri/devtools` is on, so the workspace enables that feature. Every gate
//! runs a debug build.
//!
//! Open, never toggle. `close_devtools` has an empty body on Windows and
//! `is_devtools_open` always answers false there, so a toggle would be wrong.
//! `is_devtools_open` also blocks on an event-loop round trip, which would
//! deadlock if it were ever called from the main thread.

use tauri::{AppHandle, Manager};
use wis_core::{TabId, WisError, WisResult};

/// Open the inspector on `id`'s webview and reveal the right-clicked node.
///
/// A private workspace's webviews are built with devtools off, so this is a
/// no-op there. The menu offers no row in that case.
#[tauri::command]
pub async fn inspect_tab(app: AppHandle, id: TabId) -> WisResult<()> {
    let label = format!("tab-{}", id.as_uuid());
    let webview = app
        .webviews()
        .get(&label)
        .cloned()
        .ok_or_else(|| WisError::InvalidInput {
            message: format!("no webview labelled {label}"),
        })?;

    webview.open_devtools();

    #[cfg(target_os = "macos")]
    macos::reveal_menu_target(&webview);

    Ok(())
}

/// Reveal the node `page_menu.rs` parked, which it marks in the page's DOM.
///
/// No selector inspects a node. Safari's own Inspect Element reaches
/// `InspectorDOMAgent::inspect` through `InjectedScript::inspectObject`, which
/// is where the console's `inspect()` also lands, and an inspector extension's
/// `evaluateScript:` runs in the page with the command line API bound. Failing
/// anywhere leaves the inspector open on the document, as before.
#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::CString;
    use std::ptr;

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Bool};
    use objc2::{class, msg_send, msg_send_id, sel};

    const REVEAL_JS: &str = r#"(function () {
  function find(root) {
    var node = root.querySelector('[data-wis-inspect="__WIS_NONCE__"]');
    if (node) return node;
    var all = root.querySelectorAll('*');
    for (var i = 0; i < all.length; i++) {
      if (all[i].shadowRoot) {
        node = find(all[i].shadowRoot);
        if (node) return node;
      }
    }
    return null;
  }
  var t = find(document);
  if (t) {
    t.removeAttribute('data-wis-inspect');
    inspect(t);
  }
})()"#;

    pub(super) fn reveal_menu_target<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
        let nonce = uuid::Uuid::new_v4().simple().to_string();
        let _ = webview.with_webview(move |w| unsafe {
            let wk: *mut AnyObject = w.inner() as *mut AnyObject;
            if wk.is_null() {
                return;
            }

            let inspector: Option<Retained<AnyObject>> = msg_send_id![wk, _inspector];
            let Some(inspector) = inspector else {
                return;
            };

            let host_sel = sel!(registerExtensionWithID:extensionBundleIdentifier:displayName:completionHandler:);
            let known: bool = msg_send![&*inspector, respondsToSelector: host_sel];
            if !known {
                return;
            }

            crate::platform::content_world::macos::run(
                wk,
                &format!("window.__wisInspectMark && window.__wisInspectMark('{nonce}');"),
            );
            register(inspector, nonce);
        });
    }

    /// A fresh id on each registration. The inspector drops its registrations
    /// when it closes.
    unsafe fn register(inspector: Retained<AnyObject>, nonce: String) {
        let uuid: *mut AnyObject = msg_send![class!(NSUUID), UUID];
        let extension_id: *mut AnyObject = msg_send![uuid, UUIDString];

        let host = inspector.clone();
        let registered = RcBlock::new(move |_error: *mut AnyObject, extension: *mut AnyObject| {
            let Some(extension) = (unsafe { Retained::retain(extension) }) else {
                return;
            };
            let eval_sel = sel!(evaluateScript:frameURL:contextSecurityOrigin:useContentScriptContext:completionHandler:);
            let known: bool = unsafe { msg_send![&*extension, respondsToSelector: eval_sel] };
            if !known {
                return;
            }

            unsafe { evaluate(host.clone(), extension, &nonce) };
        });

        let _: () = msg_send![
            &*inspector,
            registerExtensionWithID: extension_id,
            extensionBundleIdentifier: ns_string("com.wis.browser.inspect"),
            displayName: ns_string("Wis"),
            completionHandler: &*registered,
        ];
    }

    unsafe fn evaluate(
        inspector: Retained<AnyObject>,
        extension: Retained<AnyObject>,
        nonce: &str,
    ) {
        let unregister_from = extension.clone();
        let evaluated = RcBlock::new(move |_error: *mut AnyObject, _result: *mut AnyObject| {
            let done = RcBlock::new(|_error: *mut AnyObject| {});
            let _: () = unsafe {
                msg_send![
                    &*inspector,
                    unregisterExtension: &*unregister_from,
                    completionHandler: &*done,
                ]
            };
        });

        let null: *mut AnyObject = ptr::null_mut();
        let _: () = msg_send![
            &*extension,
            evaluateScript: ns_string(&REVEAL_JS.replace("__WIS_NONCE__", nonce)),
            frameURL: null,
            contextSecurityOrigin: null,
            useContentScriptContext: Bool::NO,
            completionHandler: &*evaluated,
        ];
    }

    unsafe fn ns_string(value: &str) -> *mut AnyObject {
        let c = CString::new(value).unwrap_or_default();
        msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()]
    }
}
