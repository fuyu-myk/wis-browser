//! Page context-menu channel: the page reports what was right-clicked, the
//! chrome draws the menu.
//!
//! Since the engines offer no public way to customise their own page menu, Wis
//! suppresses it from the page instead: a document-start listener calls
//! `preventDefault()` and posts what it found to `wisPageMenu`, and the chrome
//! opens an overlay menu at the pointer.
//!
//! Trust model, mirroring `page_text.rs`:
//! - The **tab id and page URL are read natively**, from the sending webview's
//!   own label and `url()`, never from the message.
//! - Everything else is page-provided and untrusted.
//!
//! Two deliberate limits:
//! - **Main frame only.** The script guards on `window.top === window.self`,
//!   and Windows re-checks the frame in its dispatcher, so a right-click
//!   inside an iframe keeps the engine's menu. This is a **correctness**
//!   guard, and not a trust boundary.
//! - **Editable targets keep the native menu**, which carries spelling
//!   suggestions and paste that Wis cannot offer.

use std::sync::OnceLock;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use wis_core::TabId;

/// Tauri event carrying a right-click to the chrome.
const PAGE_MENU_EVENT: &str = "wis-page-menu";

/// Boundary cap on page-provided display text, which is safe to truncate.
const MAX_TEXT_BYTES: usize = 2 * 1024;
/// A selection is carried to the assistant, so it is budgeted separately and
/// far higher.
const MAX_SELECTION_BYTES: usize = 192 * 1024;
/// Ceiling on a page-provided URL. Longer ones are dropped.
const MAX_URL_BYTES: usize = 4 * 1024;

/// Metadata of the point that was right-clicked.
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PageMenuEvent {
    /// The tab that was clicked, from the webview's own label.
    tab_id: String,
    /// The page's URL, read natively from the webview.
    page_url: String,
    /// Pointer's x position in the page's own CSS pixels.
    x: f64,
    /// Pointer's y position in the page's own CSS pixels.
    y: f64,
    /// The page view width in CSS pixels.
    view_width: f64,
    /// The page view height in CSS pixels.
    view_height: f64,
    link_href: Option<String>,
    link_text: Option<String>,
    image_src: Option<String>,
    /// `"video"` or `"audio"` when the click landed on a media element.
    media_kind: Option<String>,
    /// The media element's resolved source.
    media_src: Option<String>,
    selection: Option<String>,
}

/// App handle for the native message handlers to reach the chrome.
static APP: OnceLock<AppHandle> = OnceLock::new();

/// Record the app handle once, during setup.
pub fn set_app_handle(app: AppHandle) {
    let _ = APP.set(app);
}

/// The document-start listener. It yields to anything that already handled
/// the event: a site with its own menu, the element picker (`cosmetic.rs`),
/// and the pop-out overlay all call `preventDefault()` first.
///
/// The selection colours are baked in at injection.
pub fn page_script() -> String {
    let css = crate::selection_color::selection_css();

    PAGE_MENU_JS
        .replace("__WIS_SEL_FALLBACK__", &css.fallback)
        .replace("__WIS_SEL_PREFERRED__", &css.preferred)
}

/// Drop the stand-in highlight in `webview`. The chrome calls this when the
/// menu closes.
pub fn clear_selection_paint<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
    let _ = crate::content_world::eval(
        webview,
        "window.__wisMenuClearPaint && window.__wisMenuClearPaint();",
    );
}

const PAGE_MENU_JS: &str = r#"(function () {
  if (window.top !== window.self) return;
  if (window.__wisPageMenu) return;
  window.__wisPageMenu = true;

  var MAX_LABEL = 400;
  var MAX_SELECTION = 32000;
  var HIGHLIGHT = 'wis-menu-selection';
  var STYLE_ID = 'wis-menu-selection-style';
  // Rises per paint, so a deferred drop cannot take a newer one with it.
  var paintGen = 0;

  function post(body) {
    try {
      var handlers = window.webkit && window.webkit.messageHandlers;
      if (handlers && handlers.wisPageMenu) {
        handlers.wisPageMenu.postMessage(JSON.stringify(body));
      }
    } catch (e) {}
  }

  function isEditable(el) {
    if (!el || !el.tagName) return false;
    var tag = el.tagName.toLowerCase();
    if (tag === 'textarea') return true;
    if (tag === 'input') {
      var t = (el.getAttribute('type') || 'text').toLowerCase();
      return t === '' || t === 'text' || t === 'search' || t === 'url' ||
             t === 'email' || t === 'tel' || t === 'password' || t === 'number';
    }
    return el.isContentEditable === true;
  }

  function clip(value, limit) {
    if (typeof value !== 'string') return null;
    var text = value.trim();
    if (!text) return null;
    if (text.length <= limit) return text;
    var cut = limit;
    var last = text.charCodeAt(cut - 1);
    if (last >= 0xd800 && last <= 0xdbff) cut -= 1;
    return text.slice(0, cut);
  }

  function rule(decls) {
    return '::highlight(' + HIGHLIGHT + '){' + decls + '}';
  }
  var SUPPRESS = '::selection{background-color:transparent}';
  var PAINT =
    SUPPRESS + rule('__WIS_SEL_FALLBACK__') + rule('__WIS_SEL_PREFERRED__');

  function styleNode() {
    var style = document.getElementById(STYLE_ID);
    if (!style) {
      style = document.createElement('style');
      style.id = STYLE_ID;
      (document.head || document.documentElement).appendChild(style);
    }
    return style;
  }

  function paintSelection(sel) {
    if (!window.CSS || !CSS.highlights || !window.Highlight) return;
    try {
      var painted = new Highlight();
      for (var i = 0; i < sel.rangeCount; i++) {
        var range = sel.getRangeAt(i);
        if (!range.collapsed) painted.add(range.cloneRange());
      }
      if (!painted.size) return;

      paintGen++;
      styleNode().textContent = PAINT;
      CSS.highlights.set(HIGHLIGHT, painted);
    } catch (err) {}
  }

  function clearSelectionPaint() {
    try {
      if (!window.CSS || !CSS.highlights) return;
      if (document.getElementById(STYLE_ID)) {
        styleNode().textContent = rule('background-color:transparent');
      }
      var gen = paintGen;
      var drop = function () {
        if (gen !== paintGen) return;
        try { CSS.highlights.delete(HIGHLIGHT); } catch (err) {}
      };
      if (window.requestAnimationFrame) requestAnimationFrame(drop);
      else drop();
    } catch (err) {}
  }

  window.__wisMenuClearPaint = clearSelectionPaint;

  // Armed from `pane_focus.rs` for the press that moves focus to this pane.
  var keepSelectionOnce = false;
  window.__wisKeepSelectionOnce = function () {
    keepSelectionOnce = true;
  };

  // Inspect Element reads the node from the page's world, through the DOM.
  var inspectMarked = null;
  window.__wisInspectMark = function (nonce) {
    if (inspectMarked) {
      try { inspectMarked.removeAttribute('data-wis-inspect'); } catch (err) {}
      inspectMarked = null;
    }
    var t = window.__wisInspectTarget;
    if (t && t.deref) t = t.deref();
    if (t && !t.setAttribute) t = t.parentElement;
    if (t && t.setAttribute) {
      t.setAttribute('data-wis-inspect', String(nonce));
      inspectMarked = t;
    }
  };
  window.addEventListener('mousedown', function (e) {
    if (!keepSelectionOnce) return;
    keepSelectionOnce = false;
    if (e.button !== 0) return;
    try {
      var sel = window.getSelection();
      if (sel && sel.rangeCount && !sel.getRangeAt(0).collapsed) e.preventDefault();
    } catch (err) {}
  }, true);

  // The highlight outlives the menu.
  window.addEventListener('pointerdown', function (e) {
    if (e.button === 0) clearSelectionPaint();
  }, true);

  window.addEventListener('contextmenu', function (e) {
    if (e.defaultPrevented || !e.isTrusted) return;

    var path = [];
    try { path = e.composedPath() || []; } catch (err) { path = []; }
    if (!path.length && e.target) path = [e.target];

    var link = null, linkText = null, image = null, editable = false;
    var media = null, mediaSrc = null;
    for (var i = 0; i < path.length; i++) {
      var el = path[i];
      if (!el || !el.tagName) continue;
      var tag = el.tagName.toLowerCase();
      if (!editable && isEditable(el)) editable = true;
      if (!image && tag === 'img' && el.currentSrc) image = el.currentSrc;
      if (!media && (tag === 'video' || tag === 'audio')) {
        media = tag;
        mediaSrc = el.currentSrc || el.src || null;
      }
      if (!link && tag === 'a' && el.href) {
        link = el.href;
        linkText = clip(el.textContent, MAX_LABEL);
      }
    }

    // Leave the engine's menu for editable targets.
    if (editable) return;

    e.preventDefault();

    clearSelectionPaint();
    var selection = null;
    try {
      var sel = window.getSelection();
      selection = clip(String(sel), MAX_SELECTION);
      if (selection) paintSelection(sel);
    } catch (err) {}

    // Parked for devtools.rs.
    try {
      var node = path[0] || e.target;
      window.__wisInspectTarget =
        (node && typeof WeakRef === 'function') ? new WeakRef(node) : node;
    } catch (err) {}

    post({
      x: e.clientX,
      y: e.clientY,
      viewWidth: window.innerWidth,
      viewHeight: window.innerHeight,
      linkHref: link,
      linkText: linkText,
      imageSrc: image,
      mediaKind: media,
      mediaSrc: mediaSrc,
      selection: selection
    });
  }, false);
})();"#;

/// Drop the highlight the menu left behind in `tab_id`'s webview.
#[tauri::command]
pub fn clear_menu_selection(app: AppHandle, tab_id: TabId) {
    let label = format!("tab-{}", tab_id.as_uuid());
    if let Some(webview) = app.webviews().get(&label) {
        clear_selection_paint(webview);
    }
}

/// Forward one right-click to the chrome. Shared by all three platforms.
pub(crate) fn dispatch_message(label: &str, body: &serde_json::Value) {
    let Some(app) = APP.get() else {
        return;
    };
    let Some(tab_id) = label.strip_prefix("tab-") else {
        return;
    };

    let Some(webview) = app.webviews().get(label).cloned() else {
        return;
    };
    let page_url = webview.url().map(|url| url.to_string()).unwrap_or_default();

    let number = |key: &str| body.get(key).and_then(serde_json::Value::as_f64);
    let text = |key: &str| capped_text(body, key, MAX_TEXT_BYTES);
    let url = |key: &str| {
        body.get(key)
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty() && value.len() <= MAX_URL_BYTES)
            .map(str::to_string)
    };

    let (Some(x), Some(y)) = (number("x"), number("y")) else {
        return;
    };

    let payload = PageMenuEvent {
        tab_id: tab_id.to_string(),
        page_url,
        x,
        y,
        view_width: number("viewWidth").unwrap_or(0.0),
        view_height: number("viewHeight").unwrap_or(0.0),
        link_href: url("linkHref"),
        link_text: text("linkText"),
        image_src: url("imageSrc"),
        media_kind: text("mediaKind"),
        media_src: url("mediaSrc"),
        selection: capped_text(body, "selection", MAX_SELECTION_BYTES),
    };

    let _ = app.emit(PAGE_MENU_EVENT, payload);
}

/// A string field from the message body, bounded by `limit` bytes. `None`
/// when it is absent or empty.
fn capped_text(body: &serde_json::Value, key: &str, limit: usize) -> Option<String> {
    body.get(key)
        .and_then(serde_json::Value::as_str)
        .map(|value| capped(value, limit))
        .filter(|value| !value.is_empty())
}

/// Truncate on a character boundary, so a capped string is still valid UTF-8.
fn capped(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        return value.to_string();
    }

    let mut end = limit;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }

    value[..end].to_string()
}

/// Install the right-click reporter and its `wisPageMenu` handler on a tab
/// webview. macOS only; Windows and Linux route through their own dispatchers.
pub fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
    #[cfg(target_os = "macos")]
    macos::install(webview, webview.label().to_string());

    #[cfg(not(target_os = "macos"))]
    let _ = webview;
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::CStr;
    use std::os::raw::c_char;

    use objc2::rc::Id;
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{
        class, declare_class, msg_send, msg_send_id, mutability, ClassType, DeclaredClass,
    };

    use crate::content_world::macos as content_world;

    use super::dispatch_message;

    struct HandlerIvars {
        label: String,
    }

    declare_class!(
        struct PageMenuHandler;

        unsafe impl ClassType for PageMenuHandler {
            type Super = NSObject;
            type Mutability = mutability::InteriorMutable;
            const NAME: &'static str = "WisPageMenuHandler";
        }

        impl DeclaredClass for PageMenuHandler {
            type Ivars = HandlerIvars;
        }

        unsafe impl PageMenuHandler {
            #[method(userContentController:didReceiveScriptMessage:)]
            fn did_receive(&self, _ucc: *mut AnyObject, message: *mut AnyObject) {
                let label = self.ivars().label.clone();
                unsafe { handle_message(&label, message) }
            }
        }
    );

    impl PageMenuHandler {
        fn new(label: String) -> Id<Self> {
            let this = Self::alloc().set_ivars(HandlerIvars { label });
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

        // Only the main frame may open the menu.
        let frame: *mut AnyObject = msg_send![message, frameInfo];
        if frame.is_null() {
            return;
        }
        let is_main_frame: bool = msg_send![frame, isMainFrame];
        if !is_main_frame {
            return;
        }

        let body: *mut AnyObject = msg_send![message, body];
        let Some(raw) = nsstring_to_string(body) else {
            return;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
            return;
        };

        dispatch_message(label, &value);
    }

    pub(super) fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>, label: String) {
        let script = super::page_script();
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

            content_world::install_script(wk, controller, &label, "wisPageMenu", &script, true);
            let handler = PageMenuHandler::new(label);
            content_world::add_handler(controller, &*handler, "wisPageMenu");
        });
    }
}
