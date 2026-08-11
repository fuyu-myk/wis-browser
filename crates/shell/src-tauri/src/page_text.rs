//! AI page-text channel: extract the readable text of a tab on demand and
//! hand it to the chrome.
//!
//! Security spine (mirroring `capture.rs`): a narrow, dedicated
//! `WKScriptMessageHandler` (`wisPageText`) forwards only `{ text, title }` to
//! Rust, which re-emits it to the chrome as a Tauri event.
//!
//! Trust model:
//! - The **URL is read from the webview natively**. The chrome correlates a
//!   response to its request by that URL.
//! - Page text is untrusted input everywhere it flows: the handler re-caps it
//!   in bytes even though the page script already capped it, and the chrome
//!   renders it with React escaping only. It is never eval'd or trusted as
//!   markup.

use std::sync::OnceLock;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

/// Tauri event carrying a tab's extracted readable text to the chrome.
const PAGE_TEXT_EVENT: &str = "wis-page-text";

/// Hard cap on the delivered text. The page script also caps (in chars); this
/// is the untrusted-boundary re-cap in bytes.
const MAX_PAGE_TEXT_BYTES: usize = 150 * 1024;

#[derive(Debug, Serialize, Clone)]
struct PageTextEvent {
    /// The origin URL, read natively from the webview.
    url: String,
    /// The page title (display only; page-provided).
    title: String,
    /// The extracted readable text (untrusted; capped).
    text: String,
}

/// App handle for the native message handler to reach the async runtime.
static APP: OnceLock<AppHandle> = OnceLock::new();

/// Record the app handle once, during setup, so the `wisPageText` handler can
/// emit into the chrome.
pub fn set_app_handle(app: AppHandle) {
    let _ = APP.set(app);
}

/// The on-demand extraction script: reuse the shared main-content scoring,
/// take its plain text, cap it, and post `{ text, title }` to `wisPageText`.
/// Evaluated in the target tab by [`request_page_text`].
fn extract_script() -> &'static str {
    static SCRIPT: OnceLock<String> = OnceLock::new();
    SCRIPT.get_or_init(|| {
        format!(
            "{}\n{}",
            crate::reader::MAIN_CONTENT_FINDER,
            r#"(function () {
  try {
    var best = __wisFindMainContent();
    var text = ((best && (best.innerText || best.textContent)) || '').replace(/\s+/g, ' ').trim();
    if (text.length > 150000) text = text.slice(0, 150000);
    var title = (document.title || '').trim();
    if (window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.wisPageText) {
      window.webkit.messageHandlers.wisPageText.postMessage({ text: text, title: title });
    } else if (window.wisPageText && window.wisPageText.postMessage) {
      window.wisPageText.postMessage({ text: text, title: title });
    }
  } catch (e) {}
})();"#
        )
    })
}

/// Truncate `text` to at most [`MAX_PAGE_TEXT_BYTES`], on a char boundary.
fn cap_text(text: &str) -> String {
    if text.len() <= MAX_PAGE_TEXT_BYTES {
        return text.to_string();
    }

    let mut end = MAX_PAGE_TEXT_BYTES;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }

    text[..end].to_string()
}

/// Emit the extracted text to the chrome. `url` is the sending webview's own
/// URL, so text is always attributed to its true origin; `title`/`text` are
/// page-provided and untrusted.
fn emit_page_text(url: String, title: String, text: String) {
    let Some(app) = APP.get() else {
        return;
    };
    let event = PageTextEvent {
        url,
        title,
        text: cap_text(&text),
    };
    if let Err(err) = app.emit(PAGE_TEXT_EVENT, event) {
        log::warn!("emit {PAGE_TEXT_EVENT} failed: {err}");
    }
}

/// Windows/Linux dispatcher entry: their message shims parse the `wisPageText`
/// payload and route it here. `label` identifies the sending tab webview,
/// whose URL is read natively here.
#[cfg(any(windows, target_os = "linux"))]
pub(crate) fn dispatch_message(label: &str, text: String, title: String) {
    let Some(app) = APP.get() else {
        return;
    };
    let Some(webview) = app.webviews().get(label).cloned() else {
        return;
    };
    let Some(url) = webview.url().ok().map(|u| u.to_string()) else {
        return;
    };

    emit_page_text(url, title, text);
}

/// Ask a tab to extract and deliver its readable text. The result arrives
/// asynchronously as a `wis-page-text` event once the page posts to the
/// `wisPageText` handler.
#[tauri::command]
pub async fn request_page_text(app: AppHandle, id: wis_core::TabId) -> wis_core::WisResult<()> {
    let label = format!("tab-{}", id.as_uuid());
    let webview = app
        .webviews()
        .get(&label)
        .cloned()
        .ok_or(wis_core::WisError::TabNotFound { id })?;
    webview
        .eval(extract_script())
        .map_err(|err| wis_core::WisError::InvalidInput {
            message: format!("page-text eval failed: {err}"),
        })
}

/// Install the `wisPageText` message handler on a tab webview (macOS). The
/// extraction script itself is evaluated on demand, so nothing runs until the
/// chrome asks. No-op off macOS (Windows/Linux route through their dispatchers).
pub fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
    #[cfg(target_os = "macos")]
    macos::install(webview);

    #[cfg(not(target_os = "macos"))]
    let _ = webview;
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;

    use objc2::rc::Id;
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{
        class, declare_class, msg_send, msg_send_id, mutability, ClassType, DeclaredClass,
    };

    use super::emit_page_text;

    declare_class!(
        struct PageTextHandler;

        unsafe impl ClassType for PageTextHandler {
            type Super = NSObject;
            type Mutability = mutability::InteriorMutable;
            const NAME: &'static str = "WisPageTextHandler";
        }

        impl DeclaredClass for PageTextHandler {
            type Ivars = ();
        }

        unsafe impl PageTextHandler {
            #[method(userContentController:didReceiveScriptMessage:)]
            fn did_receive(&self, _ucc: *mut AnyObject, message: *mut AnyObject) {
                unsafe { handle_message(message) }
            }
        }
    );

    impl PageTextHandler {
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

    unsafe fn handle_message(message: *mut AnyObject) {
        if message.is_null() {
            return;
        }

        // URL natively from the sending webview: text is attributed to its own
        // origin, never one the page's JS could claim.
        let webview: *mut AnyObject = msg_send![message, webView];
        if webview.is_null() {
            return;
        }

        let url_obj: *mut AnyObject = msg_send![webview, URL];
        if url_obj.is_null() {
            return;
        }

        let url_ns: *mut AnyObject = msg_send![url_obj, absoluteString];
        let Some(url) = nsstring_to_string(url_ns) else {
            return;
        };

        // Body must be a dictionary carrying the page-provided text/title.
        let body: *mut AnyObject = msg_send![message, body];
        if body.is_null() {
            return;
        }

        let is_dict: bool = msg_send![body, isKindOfClass: class!(NSDictionary)];
        if !is_dict {
            return;
        }

        let Some(text) = dict_string(body, "text") else {
            return;
        };

        let title = dict_string(body, "title").unwrap_or_default();
        emit_page_text(url, title, text);
    }

    /// Register the narrow `wisPageText` message handler on a tab webview.
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

            let handler = PageTextHandler::new();
            let name = ns_string("wisPageText");
            let _: () = msg_send![controller, addScriptMessageHandler: &*handler, name: name];
        });
    }
}
