//! Shared native save/open panels for commands running off the main thread.
//!
//! `runModal` must run on the main thread; commands dispatch there via
//! `run_on_main_thread` and block on a channel (panels are short-lived,
//! user-driven modals). Non-macOS builds resolve to "cancelled".

use std::path::PathBuf;

use tauri::AppHandle;
use wis_core::{WisError, WisResult};

const PANEL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

fn dispatch<F>(app: &AppHandle, panel: F) -> WisResult<Option<PathBuf>>
where
    F: FnOnce() -> Option<PathBuf> + Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel::<Option<PathBuf>>();
    app.run_on_main_thread(move || {
        let _ = tx.send(panel());
    })
    .map_err(|err| WisError::InvalidInput {
        message: format!("panel dispatch: {err}"),
    })?;

    Ok(rx.recv_timeout(PANEL_TIMEOUT).ok().flatten())
}

/// Native save panel prefilled with `suggested` filename.
pub fn run_save_panel(app: &AppHandle, suggested: &str) -> WisResult<Option<PathBuf>> {
    let suggested = suggested.to_string();
    dispatch(app, move || save_panel(&suggested))
}

/// Native open panel filtered to `extensions`.
pub fn run_open_panel(app: &AppHandle, extensions: &[&str]) -> WisResult<Option<PathBuf>> {
    let exts: Vec<String> = extensions.iter().map(|e| e.to_string()).collect();
    dispatch(app, move || open_panel(&exts))
}

#[cfg(target_os = "macos")]
fn nsstring(s: &str) -> Option<*mut objc2::runtime::AnyObject> {
    use objc2::{class, msg_send};
    let c = std::ffi::CString::new(s).ok()?;
    let ns: *mut objc2::runtime::AnyObject =
        unsafe { msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()] };

    (!ns.is_null()).then_some(ns)
}

#[cfg(target_os = "macos")]
fn url_to_path(panel: *mut objc2::runtime::AnyObject) -> Option<PathBuf> {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use std::ffi::CStr;
    use std::os::raw::c_char;

    unsafe {
        let url: *mut AnyObject = msg_send![panel, URL];
        if url.is_null() {
            return None;
        }
        let path_string: *mut AnyObject = msg_send![url, path];
        if path_string.is_null() {
            return None;
        }
        let utf8: *const c_char = msg_send![path_string, UTF8String];
        if utf8.is_null() {
            return None;
        }

        Some(PathBuf::from(
            CStr::from_ptr(utf8).to_string_lossy().into_owned(),
        ))
    }
}

#[cfg(target_os = "macos")]
fn save_panel(suggested: &str) -> Option<PathBuf> {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};

    unsafe {
        let panel: *mut AnyObject = msg_send![class!(NSSavePanel), savePanel];
        if panel.is_null() {
            return None;
        }

        if let Some(name) = nsstring(suggested) {
            let _: () = msg_send![panel, setNameFieldStringValue: name];
        }
        let _: () = msg_send![panel, setCanCreateDirectories: true];

        let response: isize = msg_send![panel, runModal];
        if response != 1 {
            return None;
        }

        url_to_path(panel)
    }
}

#[cfg(target_os = "macos")]
fn open_panel(extensions: &[String]) -> Option<PathBuf> {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};

    unsafe {
        let panel: *mut AnyObject = msg_send![class!(NSOpenPanel), openPanel];
        if panel.is_null() {
            return None;
        }

        let _: () = msg_send![panel, setCanChooseFiles: true];
        let _: () = msg_send![panel, setCanChooseDirectories: false];
        let _: () = msg_send![panel, setAllowsMultipleSelection: false];
        let items: Vec<*mut AnyObject> =
            extensions.iter().filter_map(|ext| nsstring(ext)).collect();
        if !items.is_empty() {
            let arr: *mut AnyObject = msg_send![
                class!(NSArray),
                arrayWithObjects: items.as_ptr(),
                count: items.len()
            ];
            let _: () = msg_send![panel, setAllowedFileTypes: arr];
        }

        let response: isize = msg_send![panel, runModal];
        if response != 1 {
            return None;
        }

        url_to_path(panel)
    }
}

// `rfd` shows the native IFileDialog; it runs on the main thread here (via
// `dispatch`), the same as macOS's `runModal`.
#[cfg(windows)]
fn save_panel(suggested: &str) -> Option<PathBuf> {
    rfd::FileDialog::new().set_file_name(suggested).save_file()
}

#[cfg(windows)]
fn open_panel(extensions: &[String]) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new();
    if !extensions.is_empty() {
        let exts: Vec<&str> = extensions.iter().map(String::as_str).collect();
        dialog = dialog.add_filter("Supported", &exts);
    }

    dialog.pick_file()
}

#[cfg(not(any(target_os = "macos", windows)))]
fn save_panel(_suggested: &str) -> Option<PathBuf> {
    None
}

#[cfg(not(any(target_os = "macos", windows)))]
fn open_panel(_extensions: &[String]) -> Option<PathBuf> {
    None
}
