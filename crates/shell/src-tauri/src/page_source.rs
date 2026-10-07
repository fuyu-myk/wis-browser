//! View Source: the document a tab is showing, handed to a Wis source tab.
//!
//! The capture is the serialized live DOM, not the bytes the server sent. Wis
//! makes no second request. What is given up is the server's own formatting,
//! comments a script removed, and pre-hydration markup.
//!
//! Trust model mirrors `page_text.rs`: the tab id and URL are read natively
//! from the webview, never from the message, and the markup is untrusted text
//! that is capped here and rendered by React as a text node.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tokio::sync::oneshot;
use wis_core::{TabId, WisError, WisResult};

const CAPTURE_TIMEOUT: Duration = Duration::from_secs(6);
/// Untrusted-boundary re-cap, past any real page. The script caps first.
const MAX_SOURCE_BYTES: usize = 4 * 1024 * 1024;
/// One `<pre>` per chunk, so the engine lays out only what is on screen.
const CHUNK_BYTES: usize = 64 * 1024;
/// Captures held at once, bounding the worst case.
const MAX_BUFFERS: usize = 6;

static APP: OnceLock<AppHandle> = OnceLock::new();
static SOURCES: OnceLock<Mutex<Vec<(TabId, PageSource)>>> = OnceLock::new();
static PENDING: OnceLock<Mutex<HashMap<String, oneshot::Sender<Captured>>>> = OnceLock::new();

pub fn set_app_handle(app: AppHandle) {
    let _ = APP.set(app);
}

/// A captured document, ready for the page that will display it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageSource {
    url: String,
    /// Concatenated, these are the capture.
    chunks: Vec<String>,
    byte_len: usize,
    truncated: bool,
}

struct Captured {
    url: String,
    html: String,
    truncated: bool,
}

fn sources() -> &'static Mutex<Vec<(TabId, PageSource)>> {
    SOURCES.get_or_init(|| Mutex::new(Vec::new()))
}

fn pending() -> &'static Mutex<HashMap<String, oneshot::Sender<Captured>>> {
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

fn capped(html: &str) -> (String, bool) {
    if html.len() <= MAX_SOURCE_BYTES {
        return (html.to_string(), false);
    }

    let mut end = MAX_SOURCE_BYTES;
    while end > 0 && !html.is_char_boundary(end) {
        end -= 1;
    }

    (html[..end].to_string(), true)
}

/// Split on char boundaries so every chunk is renderable on its own.
fn split_source(html: &str) -> Vec<String> {
    if html.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let mut start = 0;
    while start < html.len() {
        let mut end = (start + CHUNK_BYTES).min(html.len());
        while end > start && !html.is_char_boundary(end) {
            end -= 1;
        }

        chunks.push(html[start..end].to_string());
        start = end;
    }

    chunks
}

/// The script the chrome evaluates on demand.
const CAPTURE_JS: &str = r#"(function () {
  if (window.top !== window.self) return;
  var MAX = 4000000;
  function post(body) {
    try {
      var h = window.webkit && window.webkit.messageHandlers;
      if (h && h.wisPageSource) {
        h.wisPageSource.postMessage(body);
      } else if (window.wisPageSource && window.wisPageSource.postMessage) {
        window.wisPageSource.postMessage(JSON.stringify(body));
      }
    } catch (e) {}
  }
  try {
    var root = document.documentElement;
    if (!root) { post({ html: '', truncated: false }); return; }

    // Cloned so Wis's own injected nodes go without touching the live page.
    var clone = root.cloneNode(true);
    var junk = clone.querySelectorAll(
      '#wis-cosmetic,#wis-cosmetic-host,#wis-find-style,#wis-menu-selection-style,#__wisHoverGuard,#__wisSwipeNav,' +
      '#__wis-popout-hover,#__wis-popout-style,#__wis-popout-overlay');
    for (var i = 0; i < junk.length; i++) {
      if (junk[i].parentNode) junk[i].parentNode.removeChild(junk[i]);
    }

    var out = '';
    var dt = document.doctype;
    if (dt) {
      out += '<!DOCTYPE ' + dt.name
        + (dt.publicId ? ' PUBLIC "' + dt.publicId + '"' : '')
        + (!dt.publicId && dt.systemId ? ' SYSTEM' : '')
        + (dt.systemId ? ' "' + dt.systemId + '"' : '')
        + '>\n';
    }
    out += clone.outerHTML;

    var truncated = out.length > MAX;
    if (truncated) {
      var end = MAX;
      var last = out.charCodeAt(end - 1);
      if (last >= 0xd800 && last <= 0xdbff) end -= 1;
      out = out.slice(0, end);
    }
    post({ html: out, truncated: truncated });
  } catch (e) {
    post({ html: '', truncated: false });
  }
})();"#;

/// Hand a capture over to the caller.
fn deliver(label: &str, url: String, html: String, truncated: bool) {
    let sender = pending()
        .lock()
        .expect("page_source pending poisoned")
        .remove(label);
    if let Some(sender) = sender {
        let _ = sender.send(Captured {
            url,
            html,
            truncated,
        });
    }
}

#[cfg(any(windows, target_os = "linux"))]
pub(crate) fn dispatch_message(label: &str, body: &serde_json::Value) {
    let Some(app) = APP.get() else {
        return;
    };
    let Some(webview) = app.webviews().get(label).cloned() else {
        return;
    };
    let Some(url) = webview.url().ok().map(|u| u.to_string()) else {
        return;
    };
    let html = body
        .get("html")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let truncated = body
        .get("truncated")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

    deliver(label, url, html, truncated);
}

/// Capture `id`'s document and hold it under `id`, for a source tab to claim.
///
/// Keyed by the page's own tab rather than by the source tab, so it can be
/// taken before that tab exists.
#[tauri::command]
pub async fn capture_page_source(app: AppHandle, id: TabId) -> WisResult<()> {
    let label = format!("tab-{}", id.as_uuid());
    let webview = app
        .webviews()
        .get(&label)
        .cloned()
        .ok_or(WisError::TabNotFound { id })?;

    let (tx, rx) = oneshot::channel();
    pending()
        .lock()
        .expect("page_source pending poisoned")
        .insert(label.clone(), tx);

    webview.eval(CAPTURE_JS).map_err(|err| {
        pending()
            .lock()
            .expect("page_source pending poisoned")
            .remove(&label);
        WisError::InvalidInput {
            message: format!("view-source eval failed: {err}"),
        }
    })?;

    let captured = tokio::time::timeout(CAPTURE_TIMEOUT, rx).await;
    pending()
        .lock()
        .expect("page_source pending poisoned")
        .remove(&label);
    let Ok(Ok(captured)) = captured else {
        return Err(WisError::InvalidInput {
            message: "the page did not answer with its source".to_string(),
        });
    };

    let (html, capped_here) = capped(&captured.html);
    let source = PageSource {
        url: captured.url,
        byte_len: html.len(),
        chunks: split_source(&html),
        truncated: captured.truncated || capped_here,
    };

    let mut held = sources().lock().expect("page_source poisoned");
    held.retain(|(tab, _)| *tab != id);
    held.push((id, source));
    while held.len() > MAX_BUFFERS {
        held.remove(0);
    }

    Ok(())
}

/// The capture taken from `tab`, if it is still held.
#[tauri::command]
pub async fn page_source(tab: TabId) -> WisResult<Option<PageSource>> {
    let held = sources().lock().expect("page_source poisoned");

    Ok(held
        .iter()
        .find(|(from, _)| *from == tab)
        .map(|(_, source)| source.clone()))
}

/// Drop the capture taken from `tab`, when that tab closes.
pub fn forget(tab: TabId) {
    sources()
        .lock()
        .expect("page_source poisoned")
        .retain(|(from, _)| *from != tab);
}

/// Install the `wisPageSource` handler. The capture script runs on demand.
pub fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
    #[cfg(target_os = "macos")]
    macos::install(webview, webview.label().to_string());

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

    use super::deliver;

    struct HandlerIvars {
        label: String,
    }

    declare_class!(
        struct PageSourceHandler;

        unsafe impl ClassType for PageSourceHandler {
            type Super = NSObject;
            type Mutability = mutability::InteriorMutable;
            const NAME: &'static str = "WisPageSourceHandler";
        }

        impl DeclaredClass for PageSourceHandler {
            type Ivars = HandlerIvars;
        }

        unsafe impl PageSourceHandler {
            #[method(userContentController:didReceiveScriptMessage:)]
            fn did_receive(&self, _ucc: *mut AnyObject, message: *mut AnyObject) {
                let label = self.ivars().label.clone();
                unsafe { handle_message(&label, message) }
            }
        }
    );

    impl PageSourceHandler {
        fn new(label: String) -> Id<Self> {
            let this = Self::alloc().set_ivars(HandlerIvars { label });
            unsafe { msg_send_id![super(this), init] }
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

        let body: *mut AnyObject = msg_send![message, body];
        if body.is_null() {
            return;
        }
        let is_dict: bool = msg_send![body, isKindOfClass: class!(NSDictionary)];
        if !is_dict {
            return;
        }

        let html_key = ns_string("html");
        let html_obj: *mut AnyObject = msg_send![body, objectForKey: html_key];
        let Some(html) = nsstring_to_string(html_obj) else {
            return;
        };

        let truncated_key = ns_string("truncated");
        let truncated_obj: *mut AnyObject = msg_send![body, objectForKey: truncated_key];
        let truncated = if truncated_obj.is_null() {
            false
        } else {
            let value: bool = msg_send![truncated_obj, boolValue];
            value
        };

        deliver(label, url, html, truncated);
    }

    pub fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>, label: String) {
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

            let handler = PageSourceHandler::new(label.clone());
            let name = ns_string("wisPageSource");
            let _: () = msg_send![controller, addScriptMessageHandler: &*handler, name: name];
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_capture_has_no_chunks() {
        assert!(split_source("").is_empty());
    }

    #[test]
    fn chunks_rejoin_into_the_original() {
        let html = "a".repeat(CHUNK_BYTES * 2 + 17);
        assert_eq!(split_source(&html).concat(), html);
    }

    #[test]
    fn chunks_split_on_character_boundaries() {
        // A multi-byte character straddling the boundary must not be halved.
        let html = "é".repeat(CHUNK_BYTES);
        let chunks = split_source(&html);
        assert_eq!(chunks.concat(), html);
        assert!(chunks.len() > 1);
    }

    #[test]
    fn a_capture_within_the_cap_is_not_marked_truncated() {
        let (out, truncated) = capped("<html></html>");
        assert_eq!(out, "<html></html>");
        assert!(!truncated);
    }

    #[test]
    fn an_oversized_capture_is_capped_and_marked() {
        let html = "x".repeat(MAX_SOURCE_BYTES + 10);
        let (out, truncated) = capped(&html);
        assert_eq!(out.len(), MAX_SOURCE_BYTES);
        assert!(truncated);
    }
}
