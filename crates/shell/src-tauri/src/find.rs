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

#[tauri::command]
pub async fn tab_find_string(
    app: AppHandle,
    id: TabId,
    query: String,
    backwards: bool,
    case_sensitive: bool,
) -> WisResult<()> {
    let label = format!("tab-{}", id.as_uuid());
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

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (webview, query, backwards, case_sensitive, label);
    }

    Ok(())
}

#[tauri::command]
pub async fn tab_find_clear(app: AppHandle, id: TabId) -> WisResult<()> {
    let label = format!("tab-{}", id.as_uuid());
    let webview = app
        .webviews()
        .get(&label)
        .cloned()
        .ok_or_else(|| WisError::InvalidInput {
            message: format!("no webview labelled {label}"),
        })?;

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

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (webview, label);
    }

    Ok(())
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
