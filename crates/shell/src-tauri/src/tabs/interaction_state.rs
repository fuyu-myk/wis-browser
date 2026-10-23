//! Save/restore WKWebView `interactionState` across tab sleep (macOS 12+).
//!
//! Waking a slept tab used to be a cold reload. `interactionState` is
//! WebKit's opaque session blob: back/forward list, scroll position, and
//! form data. The chrome reads it (base64) just before discarding a
//! background webview and hands it back to `create_tab_webview` on wake,
//! which starts the new webview on `about:blank` and applies the blob; the
//! restore performs its own navigation with everything intact.
//!
//! Blobs only live in chrome memory for the session, so they are always
//! consumed by the same WebKit version that produced them.

use tauri::{AppHandle, Manager};
use wis_core::{TabId, WisResult};

/// Read a webview's serialized interaction state as base64, or `None` when
/// unavailable (no webview, non-macOS, empty state).
#[tauri::command]
pub async fn tab_interaction_state(app: AppHandle, id: TabId) -> WisResult<Option<String>> {
    let label = format!("tab-{}", id.as_uuid());
    let Some(webview) = app.webviews().get(&label).cloned() else {
        return Ok(None);
    };

    Ok(read(&webview))
}

/// Read `interactionState` from a live webview. Dispatches to the main
/// thread and waits briefly for the result.
#[cfg(target_os = "macos")]
pub fn read<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) -> Option<String> {
    use std::ffi::CStr;
    use std::os::raw::c_char;

    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};

    let (tx, rx) = std::sync::mpsc::channel::<Option<String>>();
    let dispatched = webview.with_webview(move |w| {
        let value = unsafe {
            let wk: *mut AnyObject = w.inner() as *mut AnyObject;
            if wk.is_null() {
                None
            } else {
                let state: *mut AnyObject = msg_send![wk, interactionState];
                let is_data: bool = if state.is_null() {
                    false
                } else {
                    msg_send![state, isKindOfClass: class!(NSData)]
                };
                if !is_data {
                    None
                } else {
                    let b64: *mut AnyObject =
                        msg_send![state, base64EncodedStringWithOptions: 0usize];
                    if b64.is_null() {
                        None
                    } else {
                        let utf8: *const c_char = msg_send![b64, UTF8String];
                        if utf8.is_null() {
                            None
                        } else {
                            Some(CStr::from_ptr(utf8).to_string_lossy().into_owned())
                        }
                    }
                }
            }
        };
        let _ = tx.send(value);
    });

    if dispatched.is_err() {
        return None;
    }

    rx.recv_timeout(std::time::Duration::from_millis(500))
        .ok()
        .flatten()
}

#[cfg(not(target_os = "macos"))]
pub fn read<R: tauri::Runtime>(_webview: &tauri::webview::Webview<R>) -> Option<String> {
    None
}

/// [`read`] for async callers: it waits up to 500 ms on the main thread, which
/// would otherwise hold a runtime worker the proxies share.
pub async fn read_async(webview: Option<tauri::Webview>) -> Option<String> {
    let webview = webview?;
    tauri::async_runtime::spawn_blocking(move || read(&webview))
        .await
        .ok()
        .flatten()
}

/// Apply a previously read interaction state to a fresh webview. The restore
/// triggers its own navigation, so callers create the webview on
/// `about:blank` first. Best-effort: failures just leave the blank page and
/// the caller's fallback navigation (if any) takes over.
#[cfg(target_os = "macos")]
pub fn apply<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>, state_b64: &str) {
    use std::ffi::CString;

    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};

    let Ok(b64_c) = CString::new(state_b64) else {
        return;
    };

    let _ = webview.with_webview(move |w| unsafe {
        let wk: *mut AnyObject = w.inner() as *mut AnyObject;
        if wk.is_null() {
            return;
        }
        let ns_b64: *mut AnyObject =
            msg_send![class!(NSString), stringWithUTF8String: b64_c.as_ptr()];
        if ns_b64.is_null() {
            return;
        }
        let alloc: *mut AnyObject = msg_send![class!(NSData), alloc];
        // Option 1 = ignore unknown characters.
        let data: *mut AnyObject =
            msg_send![alloc, initWithBase64EncodedString: ns_b64, options: 1usize];
        if data.is_null() {
            return;
        }
        let _: () = msg_send![wk, setInteractionState: data];
    });
}

#[cfg(not(target_os = "macos"))]
pub fn apply<R: tauri::Runtime>(_webview: &tauri::webview::Webview<R>, _state_b64: &str) {}
