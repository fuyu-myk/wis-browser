//! Print / Save as PDF for the active tab via WKWebView's
//! `printOperationWithPrintInfo:` (macOS 11+).
//!
//! The standard macOS print panel carries the "PDF -> Save as PDF" menu, so
//! a single path covers both printing and exporting a page to PDF. The
//! operation runs as a document-modal sheet on the tab's window when it has
//! one, falling back to an app-modal run otherwise.
//!
//! On Windows the same single-path intent holds: `ICoreWebView2_16::ShowPrintUI`
//! opens the system print dialog, whose "Microsoft Print to PDF" destination
//! covers PDF export.

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

    // Covers both printing and PDF export, mirroring the macOS print
    // panel's "PDF -> Save as PDF" menu.
    #[cfg(windows)]
    crate::platform::win_webview2::with_core_webview2(&webview, |core| {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            ICoreWebView2_16, COREWEBVIEW2_PRINT_DIALOG_KIND_SYSTEM,
        };
        use windows::core::Interface;
        if let Ok(c16) = core.cast::<ICoreWebView2_16>() {
            // SAFETY: runs on the UI thread via `with_core_webview2`.
            let _ = unsafe { c16.ShowPrintUI(COREWEBVIEW2_PRINT_DIALOG_KIND_SYSTEM) };
        }
    });

    // WebKitGTK's print dialog; its "Print to File" destination covers PDF
    // export, mirroring the other engines.
    #[cfg(target_os = "linux")]
    {
        use webkit2gtk::{PrintOperation, PrintOperationExt};
        webview
            .with_webview(move |w| {
                let wv = w.inner();
                let _ = PrintOperation::new(&wv).run_dialog(gtk::Window::NONE);
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
