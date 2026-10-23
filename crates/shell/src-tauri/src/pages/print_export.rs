//! Print outside Chrome.
//!
//! Without Chrome's PDF viewer, certain sites print by exporting the document
//! as a print-flagged PDF into a hidden iframe, which the server sends as an
//! attachment and a browser saves. On macOS Wis opens it in the print sheet
//! instead: the download goes to a temporary file that is read and removed
//! once finished, and never reaches the downloads list.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use url::Url;

/// A print export in flight: the download URL and where it is written.
struct Pending {
    url: String,
    path: PathBuf,
    title: String,
}

static PENDING: Mutex<Vec<Pending>> = Mutex::new(Vec::new());

/// Whether `download` is a print export.
fn is_print_export(download: &Url) -> bool {
    download
        .host_str()
        .is_some_and(|host| host.ends_with(".googleusercontent.com"))
        && download.path().starts_with("/export/")
        && has_print_flag(download)
}

/// A PDF export whose options, sent as numbered pairs (`2=print&3=1`),
/// include `print=1`.
fn has_print_flag(download: &Url) -> bool {
    let pairs: HashMap<String, String> = download.query_pairs().into_owned().collect();
    pairs.get("format").is_some_and(|format| format == "pdf")
        && pairs.iter().any(|(key, value)| {
            value == "print"
                && key
                    .parse::<usize>()
                    .ok()
                    .and_then(|index| pairs.get(&(index + 1).to_string()))
                    .is_some_and(|flag| flag == "1")
        })
}

/// Take over a requested download when it is a print export, pointing
/// it at a temporary file. False leaves it to the downloads pipeline.
pub fn intercept(url: &Url, destination: &mut PathBuf) -> bool {
    if !cfg!(target_os = "macos") || !is_print_export(url) {
        return false;
    }

    let dir = std::env::temp_dir().join("wis-print");
    if let Err(err) = std::fs::create_dir_all(&dir) {
        log::warn!("print export left as a download, no temp dir: {err}");
        return false;
    }

    let title = destination
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let path = dir.join(format!("{}.pdf", uuid::Uuid::new_v4()));
    *destination = path.clone();
    PENDING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(Pending {
            url: url.to_string(),
            path,
            title,
        });
    log::info!("print export started");

    true
}

/// Finish an intercepted print export: print it when it arrived, then
/// remove the file. False when `url` is not one.
pub fn finish<R: tauri::Runtime>(webview: &tauri::Webview<R>, url: &Url, success: bool) -> bool {
    let pending = {
        let mut pending = PENDING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(index) = pending.iter().position(|p| p.url == url.as_str()) else {
            return false;
        };
        pending.remove(index)
    };

    log::info!(
        "print export {}",
        if success { "finished" } else { "failed" }
    );
    let bytes = success.then(|| std::fs::read(&pending.path));
    if let Err(err) = std::fs::remove_file(&pending.path) {
        log::warn!("print export temp file not removed: {err}");
    }
    match bytes {
        Some(Ok(bytes)) => print_pdf(webview, bytes, pending.title),
        Some(Err(err)) => log::warn!("print export unreadable: {err}"),
        None => {}
    }

    true
}

#[cfg(target_os = "macos")]
fn print_pdf<R: tauri::Runtime>(webview: &tauri::Webview<R>, bytes: Vec<u8>, title: String) {
    if let Err(err) = webview.with_webview(move |w| unsafe {
        imp::print_pdf(w.inner() as *mut objc2::runtime::AnyObject, &bytes, &title)
    }) {
        log::warn!("print export not shown: {err}");
    }
}

#[cfg(not(target_os = "macos"))]
fn print_pdf<R: tauri::Runtime>(_webview: &tauri::Webview<R>, _bytes: Vec<u8>, _title: String) {}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::{c_void, CString};

    use objc2::runtime::{AnyObject, Bool};
    use objc2::{class, msg_send, sel};

    #[link(name = "PDFKit", kind = "framework")]
    extern "C" {}

    /// `kPDFPrintPageScaleDownToFit`: shrink pages larger than the paper.
    const SCALE_DOWN_TO_FIT: isize = 2;

    /// Run the print sheet for `bytes` (a PDF) on `wk`'s window, unless the
    /// window already shows a sheet.
    pub unsafe fn print_pdf(wk: *mut AnyObject, bytes: &[u8], title: &str) {
        if wk.is_null() {
            return;
        }

        let window: *mut AnyObject = msg_send![wk, window];
        if window.is_null() {
            log::warn!("print export not shown: the tab has no window");
            return;
        }
        let sheet: *mut AnyObject = msg_send![window, attachedSheet];
        if !sheet.is_null() {
            log::warn!("print export not shown: the window already has a sheet");
            return;
        }

        let data: *mut AnyObject = msg_send![
            class!(NSData),
            dataWithBytes: bytes.as_ptr() as *const c_void,
            length: bytes.len()
        ];
        let doc: *mut AnyObject = msg_send![class!(PDFDocument), alloc];
        let doc: *mut AnyObject = msg_send![doc, initWithData: data];
        if doc.is_null() {
            log::warn!("print export is not a readable PDF");
            return;
        }

        let info: *mut AnyObject = msg_send![class!(NSPrintInfo), sharedPrintInfo];
        let op: *mut AnyObject = msg_send![
            doc,
            printOperationForPrintInfo: info,
            scalingMode: SCALE_DOWN_TO_FIT,
            autoRotate: Bool::YES
        ];
        // The operation keeps the document alive for as long as it runs.
        let _: () = msg_send![doc, release];
        if op.is_null() {
            return;
        }

        let _: () = msg_send![op, setShowsPrintPanel: Bool::YES];
        if let Ok(title) = CString::new(title) {
            let title: *mut AnyObject =
                msg_send![class!(NSString), stringWithUTF8String: title.as_ptr()];
            let _: () = msg_send![op, setJobTitle: title];
        }
        let nil: *mut AnyObject = std::ptr::null_mut();
        let _: () = msg_send![
            op,
            runOperationModalForWindow: window,
            delegate: nil,
            didRunSelector: sel!(printOperationDidRun:success:contextInfo:),
            contextInfo: std::ptr::null_mut::<c_void>()
        ];
    }

    #[cfg(test)]
    mod tests {
        use std::ffi::c_void;

        use objc2::runtime::{AnyObject, Bool, Sel};
        use objc2::{class, sel};

        type Obj = *mut AnyObject;

        /// A wrong message type aborts at runtime only, so check each send
        /// against its method.
        #[test]
        fn every_send_matches_its_method() {
            let doc = class!(PDFDocument);
            doc.verify_sel::<(Obj,), Obj>(sel!(initWithData:)).unwrap();
            doc.verify_sel::<(Obj, isize, Bool), Obj>(sel!(
                printOperationForPrintInfo:scalingMode:autoRotate:
            ))
            .unwrap();
            class!(NSData)
                .metaclass()
                .verify_sel::<(*const c_void, usize), Obj>(sel!(dataWithBytes:length:))
                .unwrap();
            let op = class!(NSPrintOperation);
            op.verify_sel::<(Obj,), ()>(sel!(setJobTitle:)).unwrap();
            op.verify_sel::<(Obj, Obj, Sel, *mut c_void), ()>(sel!(
                runOperationModalForWindow:delegate:didRunSelector:contextInfo:
            ))
            .unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRINT: &str = "https://doc-0k-bg-docstext.googleusercontent.com/export/a/b/1/c/*/abc?0=mp&1=f&2=print&3=1&4=inspectorResult&5=%7B%7D&format=pdf&id=abc&tab=t.0";
    const DOWNLOAD: &str = "https://doc-0k-bg-docstext.googleusercontent.com/export/a/b/1/c/*/abc?format=pdf&id=abc&tab=t.0&inspectorResult=%7B%7D";

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn docs_print_export_is_recognised() {
        assert!(is_print_export(&url(PRINT)));
    }

    #[test]
    fn a_pdf_download_from_the_file_menu_is_left_alone() {
        assert!(!is_print_export(&url(DOWNLOAD)));
    }

    #[test]
    fn the_print_pair_must_be_print_then_one() {
        let other = PRINT.replace("3=1", "3=0");
        assert!(!is_print_export(&url(&other)));
        let named = PRINT.replace("2=print&3=1", "print=1");
        assert!(!is_print_export(&url(&named)));
    }

    #[test]
    fn only_a_pdf_export_from_googleusercontent_counts() {
        let docx = PRINT.replace("format=pdf", "format=docx");
        assert!(!is_print_export(&url(&docx)));
        let path = PRINT.replace("/export/", "/download/");
        assert!(!is_print_export(&url(&path)));
        let host = PRINT.replace("googleusercontent.com", "googleusercontent.com.example.org");
        assert!(!is_print_export(&url(&host)));
    }
}
