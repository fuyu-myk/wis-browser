//! Per-tab Find-in-Page with a match count.
//!
//! WKWebView's public `findString:withConfiguration:` reports only whether a
//! match exists, not how many or which one is current. Safari's count comes
//! from WebKit SPI: a `_WKFindDelegate` plus `_findString:options:maxCount:`,
//! whose `didFindMatches:forString:withMatchIndex:` callback carries the total
//! and the current index. This module drives that SPI (guarded behind
//! `respondsToSelector:`, the same way `audio.rs` uses `_isPlayingAudio`) and
//! falls back to the public API (match / no-match, no count) if it is ever
//! unavailable.
//!
//! Results are published asynchronously via the `wis-find-result` Tauri event
//! so the FindBar renders the count without blocking the IPC.

use std::sync::OnceLock;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use wis_core::{TabId, WisError, WisResult};

pub const FIND_RESULT_EVENT: &str = "wis-find-result";

static APP: OnceLock<AppHandle> = OnceLock::new();

/// Record the app handle so the find delegate can emit results.
pub fn set_app_handle(app: AppHandle) {
    let _ = APP.set(app);
}

#[derive(Debug, Clone, Serialize)]
struct FindResultPayload {
    tab_id: String,
    query: String,
    found: bool,
    /// Total matches, or -1 when the count is unavailable (public fallback).
    total: i64,
    /// 1-based index of the current match, or -1 when not determined.
    index: i64,
}

/// Emit a result for the tab whose webview is `label` (`tab-<uuid>`).
fn emit_result(label: &str, query: &str, found: bool, total: i64, index: i64) {
    let Some(app) = APP.get() else {
        return;
    };

    let tab_id = label.strip_prefix("tab-").unwrap_or(label).to_string();
    let payload = FindResultPayload {
        tab_id,
        query: query.to_string(),
        found,
        total,
        index,
    };

    if let Err(err) = app.emit(FIND_RESULT_EVENT, payload) {
        log::warn!("emit {FIND_RESULT_EVENT} failed: {err}");
    }
}

/// Connect the WebKitGTK `FindController` result signals for a tab webview so
/// match counts reach the FindBar. Connected once per webview. WebKitGTK has
/// no current-match index, so the index is always emitted as -1.
#[cfg(target_os = "linux")]
pub(crate) fn install_linux(webview: &webkit2gtk::WebView, label: &str) {
    use webkit2gtk::{FindControllerExt, WebViewExt};

    let Some(controller) = webview.find_controller() else {
        return;
    };

    let found_label = label.to_string();
    controller.connect_found_text(move |fc, count| {
        let query = fc.search_text().map(|s| s.to_string()).unwrap_or_default();
        emit_result(&found_label, &query, true, count as i64, -1);
    });

    let failed_label = label.to_string();
    controller.connect_failed_to_find_text(move |fc| {
        let query = fc.search_text().map(|s| s.to_string()).unwrap_or_default();
        emit_result(&failed_label, &query, false, 0, -1);
    });
}

/// While a peek preview is up in the calling window, find acts on it instead
/// of the tab hidden behind it. Results then carry the peek label as their
/// `tab_id`; the FindBar accepts both.
fn find_target_label(window: &tauri::Window, id: TabId) -> String {
    crate::peek::live_peek_label(window).unwrap_or_else(|| format!("tab-{}", id.as_uuid()))
}

#[tauri::command]
pub async fn tab_find_string(
    app: AppHandle,
    window: tauri::Window,
    id: TabId,
    query: String,
    backwards: bool,
    case_sensitive: bool,
) -> WisResult<()> {
    let label = find_target_label(&window, id);
    let webview = app
        .webviews()
        .get(&label)
        .cloned()
        .ok_or_else(|| WisError::InvalidInput {
            message: format!("no webview labelled {label}"),
        })?;

    #[cfg(target_os = "macos")]
    {
        let app_for_fallback = app.clone();
        webview
            .with_webview(move |w| unsafe {
                let wk = w.inner() as *mut objc2::runtime::AnyObject;
                if wk.is_null() {
                    return;
                }

                imp::find(
                    wk,
                    &label,
                    &app_for_fallback,
                    &query,
                    backwards,
                    case_sensitive,
                );
            })
            .map_err(|err| WisError::InvalidInput {
                message: format!("with_webview failed: {err}"),
            })?;
    }

    // Windows: drive the JS find engine (CSS Custom Highlight API). The engine
    // rides the tab builder as an init script. The accent is read per search,
    // so a change in Settings shows on the next one.
    #[cfg(windows)]
    {
        let _ = label;
        let q = serde_json::to_string(&query).unwrap_or_else(|_| "\"\"".to_string());
        let colors = serde_json::to_string(&find_colors(accent_rgb()))
            .unwrap_or_else(|_| "null".to_string());
        let js = format!(
            "window.__wisFindSearch && window.__wisFindSearch({q}, {backwards}, {case_sensitive}, {colors});"
        );
        let _ = webview.eval(&js);
    }

    // Linux: drive WebKitGTK's native FindController. Result counts arrive
    // over the signals wired in `install_linux`.
    #[cfg(target_os = "linux")]
    {
        webview
            .with_webview(move |w| {
                use webkit2gtk::{FindControllerExt, FindOptions, WebViewExt};
                let Some(fc) = w.inner().find_controller() else {
                    return;
                };

                let mut options = FindOptions::WRAP_AROUND;
                if !case_sensitive {
                    options |= FindOptions::CASE_INSENSITIVE;
                }
                if backwards {
                    options |= FindOptions::BACKWARDS;
                }

                let same = fc
                    .search_text()
                    .map(|s| s.as_str() == query.as_str())
                    .unwrap_or(false);
                if same && backwards {
                    fc.search_previous();
                } else if same {
                    fc.search_next();
                } else {
                    fc.search(&query, options.bits(), u32::MAX);
                }
            })
            .map_err(|err| WisError::InvalidInput {
                message: format!("with_webview failed: {err}"),
            })?;
    }

    #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
    {
        let _ = (webview, query, backwards, case_sensitive, label);
    }

    Ok(())
}

#[tauri::command]
pub async fn tab_find_clear(app: AppHandle, window: tauri::Window, id: TabId) -> WisResult<()> {
    let mut labels = vec![format!("tab-{}", id.as_uuid())];
    if let Some(peek) = crate::peek::live_peek_label(&window) {
        labels.push(peek);
    }

    for label in labels {
        let Some(webview) = app.webviews().get(&label).cloned() else {
            continue;
        };
        clear_webview(&webview)?;
    }

    Ok(())
}

/// Drop a webview's find highlights. Also run as a page starts navigating,
/// while the old document is still current, so its matches cannot come back
/// with it from the back-forward cache.
pub(crate) fn clear_webview(webview: &tauri::webview::Webview) -> WisResult<()> {
    #[cfg(target_os = "macos")]
    {
        webview
            .with_webview(move |w| unsafe {
                let wk = w.inner() as *mut objc2::runtime::AnyObject;
                if wk.is_null() {
                    return;
                }

                imp::clear(wk);
            })
            .map_err(|err| WisError::InvalidInput {
                message: format!("with_webview failed: {err}"),
            })?;
    }

    #[cfg(windows)]
    {
        let _ = webview.eval("window.__wisFindClear && window.__wisFindClear();");
    }

    #[cfg(target_os = "linux")]
    {
        webview
            .with_webview(|w| {
                use webkit2gtk::{FindControllerExt, WebViewExt};
                if let Some(fc) = w.inner().find_controller() {
                    fc.search_finish();
                }
            })
            .map_err(|err| WisError::InvalidInput {
                message: format!("with_webview failed: {err}"),
            })?;
    }

    #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
    {
        let _ = webview;
    }

    Ok(())
}

/// What the Windows engine paints matches with: the accent, tinted for every
/// match and solid for the current one.
#[cfg(any(windows, test))]
#[derive(Debug, PartialEq, Eq, Serialize)]
struct FindColors {
    #[serde(rename = "match")]
    matches: String,
    current: String,
    /// The current match's text, whichever of black and white reads on it.
    text: String,
}

#[cfg(any(windows, test))]
fn find_colors((red, green, blue): (u8, u8, u8)) -> FindColors {
    // WCAG relative luminance. Black and white contrast equally at 0.179.
    let linear = |channel: u8| {
        let c = f64::from(channel) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let luminance = 0.2126 * linear(red) + 0.7152 * linear(green) + 0.0722 * linear(blue);

    FindColors {
        matches: format!("rgba({red}, {green}, {blue}, 0.35)"),
        current: format!("rgb({red}, {green}, {blue})"),
        text: if luminance > 0.25 { "#000" } else { "#fff" }.to_string(),
    }
}

/// The desktop accent, as Explorer uses it. Blink paints its own blue for the
/// CSS `Highlight` keyword, so the page cannot be asked.
#[cfg(windows)]
fn accent_rgb() -> (u8, u8, u8) {
    use windows::core::w;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};

    /// Windows' own default accent.
    const DEFAULT: (u8, u8, u8) = (0, 120, 212);

    let mut value: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: `value` outlives the call and `size` is its exact size;
    // RRF_RT_REG_DWORD refuses any value that is not a 4-byte DWORD.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\DWM"),
            w!("AccentColor"),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast()),
            Some(&mut size),
        )
    };
    if status != ERROR_SUCCESS {
        return DEFAULT;
    }

    // Stored as 0xAABBGGRR.
    (value as u8, (value >> 8) as u8, (value >> 16) as u8)
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::{CStr, CString};
    use std::os::raw::{c_char, c_void};

    use block2::RcBlock;
    use objc2::rc::Id;
    use objc2::runtime::{AnyObject, Bool, NSObject};
    use objc2::{
        class, declare_class, msg_send, msg_send_id, mutability, sel, ClassType, DeclaredClass,
    };
    use tauri::AppHandle;

    use super::emit_result;

    // _WKFindOptions bit flags (WebKit/UIProcess/API/Cocoa/_WKFindOptions.h).
    const CASE_INSENSITIVE: usize = 1 << 0;
    const BACKWARDS: usize = 1 << 3;
    const WRAP_AROUND: usize = 1 << 4;
    const SHOW_FIND_INDICATOR: usize = 1 << 6;
    const SHOW_HIGHLIGHT: usize = 1 << 7;
    const DETERMINE_MATCH_INDEX: usize = 1 << 9;

    // Cap the search work; pages with more matches report the cap.
    const MAX_COUNT: usize = 1000;

    // OBJC_ASSOCIATION_RETAIN_NONATOMIC.
    const ASSOCIATION_RETAIN: usize = 1;
    static ASSOC_KEY: u8 = 0;

    extern "C" {
        fn objc_setAssociatedObject(
            object: *mut AnyObject,
            key: *const c_void,
            value: *mut AnyObject,
            policy: usize,
        );
        fn objc_getAssociatedObject(object: *mut AnyObject, key: *const c_void) -> *mut AnyObject;
    }

    struct FindIvars {
        label: String,
    }

    declare_class!(
        struct WisFindDelegate;

        unsafe impl ClassType for WisFindDelegate {
            type Super = NSObject;
            type Mutability = mutability::InteriorMutable;
            const NAME: &'static str = "WisFindDelegate";
        }

        impl DeclaredClass for WisFindDelegate {
            type Ivars = FindIvars;
        }

        unsafe impl WisFindDelegate {
            #[method(_webView:didFindMatches:forString:withMatchIndex:)]
            fn did_find(
                &self,
                _webview: *mut AnyObject,
                matches: usize,
                string: *mut AnyObject,
                match_index: isize,
            ) {
                let label = self.ivars().label.clone();
                let query = unsafe { nsstring_to_string(string) }.unwrap_or_default();
                let total = if matches > 100_000 {
                    -1
                } else {
                    matches as i64
                };
                // matchIndex is 0-based, or NSNotFound (isize::MAX) when unset.
                let index: i64 = if match_index < 0 || match_index == isize::MAX {
                    -1
                } else {
                    match_index as i64 + 1
                };

                emit_result(&label, &query, true, total, index);
            }

            #[method(_webView:didFailToFindString:)]
            fn did_fail(&self, _webview: *mut AnyObject, string: *mut AnyObject) {
                let label = self.ivars().label.clone();
                let query = unsafe { nsstring_to_string(string) }.unwrap_or_default();
                emit_result(&label, &query, false, 0, -1);
            }
        }
    );

    impl WisFindDelegate {
        fn new(label: String) -> Id<Self> {
            let this = Self::alloc().set_ivars(FindIvars { label });
            unsafe { msg_send_id![super(this), init] }
        }
    }

    /// Set the find delegate once per webview, retained as an associated object
    /// so it lives exactly as long as the webview.
    unsafe fn ensure_delegate(wk: *mut AnyObject, label: &str) {
        let key = &ASSOC_KEY as *const u8 as *const c_void;
        if !objc_getAssociatedObject(wk, key).is_null() {
            return;
        }

        let delegate = WisFindDelegate::new(label.to_string());
        let _: () = msg_send![wk, _setFindDelegate: &*delegate];
        objc_setAssociatedObject(
            wk,
            key,
            Id::as_ptr(&delegate) as *mut AnyObject,
            ASSOCIATION_RETAIN,
        );
    }

    pub unsafe fn find(
        wk: *mut AnyObject,
        label: &str,
        app: &AppHandle,
        query: &str,
        backwards: bool,
        case_sensitive: bool,
    ) {
        let spi: Bool = msg_send![wk, respondsToSelector: sel!(_findString:options:maxCount:)];
        let can_delegate: Bool = msg_send![wk, respondsToSelector: sel!(_setFindDelegate:)];
        if spi.as_bool() && can_delegate.as_bool() {
            ensure_delegate(wk, label);
            // Indicate the current match and highlight the rest.
            let mut options =
                WRAP_AROUND | SHOW_FIND_INDICATOR | SHOW_HIGHLIGHT | DETERMINE_MATCH_INDEX;
            if !case_sensitive {
                options |= CASE_INSENSITIVE;
            }
            if backwards {
                options |= BACKWARDS;
            }
            let q = ns_string(query);
            let _: () = msg_send![wk, _findString: q, options: options, maxCount: MAX_COUNT];
        } else {
            public_find(wk, label, app, query, backwards, case_sensitive);
        }
    }

    /// Public-API fallback: highlights and reports match / no-match, no count.
    unsafe fn public_find(
        wk: *mut AnyObject,
        label: &str,
        app: &AppHandle,
        query: &str,
        backwards: bool,
        case_sensitive: bool,
    ) {
        let q = ns_string(query);
        let config: *mut AnyObject = msg_send![class!(WKFindConfiguration), new];
        let _: () = msg_send![config, setBackwards: Bool::new(backwards)];
        let _: () = msg_send![config, setCaseSensitive: Bool::new(case_sensitive)];

        let label = label.to_string();
        let query = query.to_string();
        let _app = app.clone();
        let handler = RcBlock::new(move |result: *mut AnyObject| {
            let found = if result.is_null() {
                false
            } else {
                let m: Bool = msg_send![result, matchFound];
                m.as_bool()
            };

            emit_result(&label, &query, found, -1, -1);
        });
        let _: () = msg_send![
            wk,
            findString: q,
            withConfiguration: config,
            completionHandler: &*handler,
        ];
    }

    pub unsafe fn clear(wk: *mut AnyObject) {
        let hides: Bool = msg_send![wk, respondsToSelector: sel!(_hideFindUI)];
        if hides.as_bool() {
            let _: () = msg_send![wk, _hideFindUI];
            return;
        }

        // Fallback: an empty search clears the public-API highlight.
        let empty = ns_string("");
        let config: *mut AnyObject = msg_send![class!(WKFindConfiguration), new];
        let noop = RcBlock::new(|_r: *mut AnyObject| {});
        let _: () = msg_send![
            wk,
            findString: empty,
            withConfiguration: config,
            completionHandler: &*noop,
        ];
    }

    unsafe fn ns_string(s: &str) -> *mut AnyObject {
        let c = CString::new(s).unwrap_or_default();
        msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()]
    }

    unsafe fn nsstring_to_string(ns: *mut AnyObject) -> Option<String> {
        if ns.is_null() {
            return None;
        }

        let utf8: *const c_char = msg_send![ns, UTF8String];
        if utf8.is_null() {
            return None;
        }

        Some(CStr::from_ptr(utf8).to_string_lossy().into_owned())
    }
}

/// Windows find engine: WebView2 has no native find SPI, so the match-count find
/// is reimplemented in JS with the CSS Custom Highlight API. Injected once per
/// tab as an init script; `tab_find_string`/`tab_find_clear` call
/// `window.__wisFindSearch` / `__wisFindClear`, and results come back over the
/// `wisFindResult` channel to [`dispatch_message`]. Top-document only
/// (cross-origin iframes aren't searched), and capped at `MAX` matches like the
/// macOS path.
#[cfg(windows)]
const FIND_ENGINE_JS: &str = r#"(function () {
  if (window.__wisFindInstalled) return;
  window.__wisFindInstalled = true;
  var MAX = 1000;
  var state = { query: '', ci: true, ranges: [], index: -1 };
  // ASCII-only lowercasing keeps the string length 1:1 so match offsets stay
  // valid against the original node text.
  function ciLower(s) {
    return s.replace(/[A-Z]/g, function (c) { return c.toLowerCase(); });
  }
  var DEFAULT_COLORS = { match: 'rgba(0, 120, 212, 0.35)', current: 'rgb(0, 120, 212)', text: '#fff' };
  function ensureStyle(c) {
    var st = document.getElementById('wis-find-style');
    if (!st) {
      st = document.createElement('style');
      st.id = 'wis-find-style';
      (document.head || document.documentElement).appendChild(st);
    }
    var css =
      '::highlight(wis-find){background:' + c.match + '}' +
      '::highlight(wis-find-current){background:' + c.current + ';color:' + c.text + '}';
    if (st.textContent !== css) st.textContent = css;
  }
  function post(found, total, index) {
    try {
      window.webkit.messageHandlers.wisFindResult.postMessage(
        { query: state.query, found: found, total: total, index: index }
      );
    } catch (e) {}
  }
  function clearHl() {
    try {
      if (window.CSS && CSS.highlights) {
        CSS.highlights.delete('wis-find');
        CSS.highlights.delete('wis-find-current');
      }
    } catch (e) {}
  }
  function apply() {
    if (!window.CSS || !CSS.highlights || typeof Highlight === 'undefined') return;
    clearHl();
    if (state.ranges.length === 0) return;
    try {
      var hl = new Highlight();
      for (var i = 0; i < state.ranges.length; i++) hl.add(state.ranges[i]);
      CSS.highlights.set('wis-find', hl);
      if (state.index >= 0 && state.index < state.ranges.length) {
        CSS.highlights.set('wis-find-current', new Highlight(state.ranges[state.index]));
      }
    } catch (e) {}
  }
  function scrollCurrent() {
    if (state.index < 0 || state.index >= state.ranges.length) return;
    try {
      var r = state.ranges[state.index];
      var rect = r.getBoundingClientRect();
      var vh = window.innerHeight || document.documentElement.clientHeight;
      if (rect.top < 0 || rect.bottom > vh) {
        var el = r.startContainer.parentElement;
        if (el && el.scrollIntoView) el.scrollIntoView({ block: 'center', inline: 'nearest' });
      }
    } catch (e) {}
  }
  function collect(query, ci) {
    var ranges = [];
    if (!query) return ranges;
    var needle = ci ? ciLower(query) : query;
    var root = document.body || document.documentElement;
    if (!root) return ranges;
    var walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
      acceptNode: function (node) {
        if (!node.nodeValue || !node.nodeValue.trim()) return NodeFilter.FILTER_REJECT;
        var p = node.parentElement;
        if (!p) return NodeFilter.FILTER_REJECT;
        var tag = p.tagName;
        if (tag === 'SCRIPT' || tag === 'STYLE' || tag === 'NOSCRIPT' || tag === 'TEXTAREA') {
          return NodeFilter.FILTER_REJECT;
        }
        return NodeFilter.FILTER_ACCEPT;
      }
    });
    var node;
    while ((node = walker.nextNode())) {
      var hay = ci ? ciLower(node.nodeValue) : node.nodeValue;
      var from = 0, idx;
      while ((idx = hay.indexOf(needle, from)) !== -1) {
        // `ciLower` preserves length, so `idx` is a valid offset into the
        // original node text.
        try {
          var range = document.createRange();
          range.setStart(node, idx);
          range.setEnd(node, idx + needle.length);
          ranges.push(range);
        } catch (e) {}
        from = idx + needle.length;
        if (ranges.length >= MAX) return ranges;
      }
    }
    return ranges;
  }
  window.__wisFindSearch = function (query, backwards, caseSensitive, colors) {
    ensureStyle(colors || DEFAULT_COLORS);
    var ci = !caseSensitive;
    if (query && query === state.query && ci === state.ci && state.ranges.length > 0) {
      state.index += backwards ? -1 : 1;
      if (state.index < 0) state.index = state.ranges.length - 1;
      if (state.index >= state.ranges.length) state.index = 0;
      apply(); scrollCurrent();
      post(true, state.ranges.length, state.index + 1);
      return;
    }
    state.query = query || '';
    state.ci = ci;
    state.ranges = collect(state.query, ci);
    if (state.ranges.length === 0) {
      state.index = -1;
      clearHl();
      post(false, 0, -1);
      return;
    }
    state.index = backwards ? state.ranges.length - 1 : 0;
    apply(); scrollCurrent();
    post(true, state.ranges.length, state.index + 1);
  };
  window.__wisFindClear = function () {
    state = { query: '', ci: true, ranges: [], index: -1 };
    clearHl();
  };
})();"#;

/// The find engine, exposed so the Windows tab builder can inject it as an init
/// script.
#[cfg(windows)]
pub fn engine_script() -> &'static str {
    FIND_ENGINE_JS
}

/// Windows dispatcher entry for `wisFindResult`: re-emit the JS engine's result
/// over [`FIND_RESULT_EVENT`] (macOS reaches the same emit through its
/// `_WKFindDelegate`).
#[cfg(windows)]
pub(crate) fn dispatch_message(label: &str, body: &serde_json::Value) {
    let query = body
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let found = body.get("found").and_then(|v| v.as_bool()).unwrap_or(false);
    let total = body.get("total").and_then(|v| v.as_i64()).unwrap_or(-1);
    let index = body.get("index").and_then(|v| v.as_i64()).unwrap_or(-1);
    emit_result(label, query, found, total, index);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_match_is_a_tint_and_the_current_one_solid() {
        let colors = find_colors((0, 120, 212));
        assert_eq!(colors.matches, "rgba(0, 120, 212, 0.35)");
        assert_eq!(colors.current, "rgb(0, 120, 212)");
    }

    #[test]
    fn a_dark_accent_takes_white_text_and_a_light_one_black() {
        assert_eq!(find_colors((0, 120, 212)).text, "#fff");
        assert_eq!(find_colors((16, 16, 16)).text, "#fff");
        assert_eq!(find_colors((255, 185, 0)).text, "#000");
        assert_eq!(find_colors((247, 99, 12)).text, "#000");
        assert_eq!(find_colors((240, 240, 240)).text, "#000");
    }

    #[test]
    fn the_engine_reads_the_colours_by_these_names() {
        let json = serde_json::to_value(find_colors((1, 2, 3))).unwrap();
        for key in ["match", "current", "text"] {
            assert!(json.get(key).is_some(), "missing {key}");
        }
    }
}
