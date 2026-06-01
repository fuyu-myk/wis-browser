//! Per-tab Find-in-Page via WKWebView's public
//! `findString:withConfiguration:completionHandler:` API (macOS 11+).
//!
//! Two commands:
//!   - [`tab_find_string`] issues a find. `backwards` controls direction so
//!     the same call serves the initial search, Cmd+G (forward), and
//!     Cmd+Shift+G (backward).
//!   - [`tab_find_clear`] calls `_hideFindUI` to drop highlights.
//!
//! Match success is published asynchronously via the `wis-find-result`
//! Tauri event so the FindBar can render a "match / no match" indicator
//! without blocking the IPC.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use wis_core::{TabId, WisError, WisResult};

pub const FIND_RESULT_EVENT: &str = "wis-find-result";

#[derive(Debug, Clone, Serialize)]
struct FindResultPayload<'a> {
    tab_id: &'a str,
    query: &'a str,
    backwards: bool,
    found: bool,
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
        use block2::RcBlock;
        use objc2::msg_send;
        use objc2::runtime::{AnyObject, Bool};

        let app_clone = app.clone();
        let tab_id_str = id.as_uuid().to_string();
        let query_for_event = query.clone();

        webview
            .with_webview(move |w| unsafe {
                let wk: *mut AnyObject = w.inner() as *mut AnyObject;
                if wk.is_null() {
                    return;
                }

                let q_c = match std::ffi::CString::new(query.as_str()) {
                    Ok(s) => s,
                    Err(_) => return,
                };
                let ns_q: *mut AnyObject =
                    msg_send![objc2::class!(NSString), stringWithUTF8String: q_c.as_ptr()];

                // Build WKFindConfiguration with backwards + case sensitivity.
                let config: *mut AnyObject =
                    msg_send![objc2::class!(WKFindConfiguration), new];
                let _: () = msg_send![config, setBackwards: Bool::new(backwards)];
                let _: () = msg_send![config, setCaseSensitive: Bool::new(case_sensitive)];

                let app_for_block = app_clone.clone();
                let tab_id_block = tab_id_str.clone();
                let query_block = query_for_event.clone();
                let handler = RcBlock::new(move |result: *mut AnyObject| {
                    let found = if result.is_null() {
                        false
                    } else {
                        let m: Bool = msg_send![result, matchFound];
                        m.as_bool()
                    };
                    let payload = FindResultPayload {
                        tab_id: &tab_id_block,
                        query: &query_block,
                        backwards,
                        found,
                    };
                    if let Err(err) = app_for_block.emit(FIND_RESULT_EVENT, payload) {
                        log::warn!("emit {FIND_RESULT_EVENT} failed: {err}");
                    }
                });

                let _: () = msg_send![
                    wk,
                    findString: ns_q,
                    withConfiguration: config,
                    completionHandler: &*handler,
                ];
            })
            .map_err(|err| WisError::InvalidInput {
                message: format!("with_webview failed: {err}"),
            })?;
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (webview, query, backwards, case_sensitive);
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
        use block2::RcBlock;
        use objc2::msg_send;
        use objc2::runtime::{AnyObject, Bool};

        webview
            .with_webview(move |w| unsafe {
                let wk: *mut AnyObject = w.inner() as *mut AnyObject;
                if wk.is_null() {
                    return;
                }
                let empty_c = std::ffi::CString::new("").unwrap();
                let ns_empty: *mut AnyObject =
                    msg_send![objc2::class!(NSString), stringWithUTF8String: empty_c.as_ptr()];
                let config: *mut AnyObject =
                    msg_send![objc2::class!(WKFindConfiguration), new];
                let _: () = msg_send![config, setBackwards: Bool::NO];
                let _: () = msg_send![config, setCaseSensitive: Bool::NO];
                let noop = RcBlock::new(|_result: *mut AnyObject| {});
                let _: () = msg_send![
                    wk,
                    findString: ns_empty,
                    withConfiguration: config,
                    completionHandler: &*noop,
                ];
            })
            .map_err(|err| WisError::InvalidInput {
                message: format!("with_webview failed: {err}"),
            })?;
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = webview;
    }

    Ok(())
}
