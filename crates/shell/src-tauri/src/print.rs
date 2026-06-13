//! Print / Save as PDF for the active tab via WKWebView's
//! `printOperationWithPrintInfo:` (macOS 11+).
//!
//! The standard macOS print panel carries the "PDF -> Save as PDF" menu, so
//! a single path covers both printing and exporting a page to PDF. The
//! operation runs as a document-modal sheet on the tab's window when it has
//! one, falling back to an app-modal run otherwise.

use tauri::{AppHandle, Manager};
use wis_core::{TabId, WisError, WisResult};

#[tauri::command]
pub async fn print_active_tab(app: AppHandle, id: TabId) -> WisResult<()> {
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
        use objc2::runtime::{AnyObject, Bool};
        use objc2::{class, msg_send, sel};

        webview
            .with_webview(move |w| unsafe {
                let wk: *mut AnyObject = w.inner() as *mut AnyObject;
                if wk.is_null() {
                    return;
                }

                // Shared print info persists the user's last paper choice and
                // is owned by AppKit.
                let info: *mut AnyObject = msg_send![class!(NSPrintInfo), sharedPrintInfo];
                let op: *mut AnyObject = msg_send![wk, printOperationWithPrintInfo: info];
                if op.is_null() {
                    return;
                }
                let _: () = msg_send![op, setShowsPrintPanel: Bool::YES];

                let window: *mut AnyObject = msg_send![wk, window];
                if window.is_null() {
                    let _: Bool = msg_send![op, runOperation];
                } else {
                    let _: () = msg_send![
                        op,
                        runOperationModalForWindow: window,
                        delegate: std::ptr::null_mut::<AnyObject>(),
                        didRunSelector: sel!(printOperationDidRun:success:contextInfo:),
                        contextInfo: std::ptr::null_mut::<std::ffi::c_void>(),
                    ];
                }
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
