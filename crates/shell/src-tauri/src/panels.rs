//! Shared native save/open panels for commands running off the main thread.
//!
//! `runModal` must run on the main thread; commands dispatch there via
//! `run_on_main_thread` and block on a channel (panels are short-lived,
//! user-driven modals). Non-macOS builds resolve to "cancelled".

use std::path::PathBuf;

use tauri::AppHandle;
use wis_core::{WisError, WisResult};

const PANEL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

fn dispatch<T, F>(app: &AppHandle, panel: F) -> WisResult<Option<T>>
where
    T: Send + 'static,
    F: FnOnce() -> Option<T> + Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel::<Option<T>>();
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

/// A system warning dialog that pages and Wis's own webviews cannot click,
/// resolving whether the user chose `action`.
pub fn confirm_natively(
    app: &AppHandle,
    title: &str,
    message: &str,
    action: &str,
) -> WisResult<bool> {
    let (title, message, action) = (title.to_string(), message.to_string(), action.to_string());
    Ok(dispatch(app, move || Some(confirm_dialog(&title, &message, &action)))?.unwrap_or(false))
}

#[cfg(target_os = "macos")]
fn confirm_dialog(title: &str, message: &str, action: &str) -> bool {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};

    const CRITICAL: usize = 2;
    const SECOND_BUTTON: isize = 1001;
    unsafe {
        let alert: *mut AnyObject = msg_send![class!(NSAlert), new];
        if alert.is_null() {
            return false;
        }

        if let Some(text) = nsstring(title) {
            let _: () = msg_send![alert, setMessageText: text];
        }
        if let Some(text) = nsstring(message) {
            let _: () = msg_send![alert, setInformativeText: text];
        }
        let _: () = msg_send![alert, setAlertStyle: CRITICAL];
        let mut added = 0;
        for label in ["Cancel", action] {
            if let Some(text) = nsstring(label) {
                let button: *mut AnyObject = msg_send![alert, addButtonWithTitle: text];
                added += usize::from(!button.is_null());
            }
        }

        let response: isize = msg_send![alert, runModal];
        let _: () = msg_send![alert, release];
        added == 2 && response == SECOND_BUTTON
    }
}

#[cfg(windows)]
fn confirm_dialog(title: &str, message: &str, action: &str) -> bool {
    match rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Warning)
        .set_title(title)
        .set_description(message)
        .set_buttons(rfd::MessageButtons::OkCancelCustom(
            action.to_string(),
            "Cancel".to_string(),
        ))
        .show()
    {
        rfd::MessageDialogResult::Ok => true,
        rfd::MessageDialogResult::Custom(label) => label == action,
        _ => false,
    }
}

#[cfg(target_os = "linux")]
fn confirm_dialog(title: &str, message: &str, action: &str) -> bool {
    use gtk::prelude::*;

    let dialog = gtk::MessageDialog::new(
        None::<&gtk::Window>,
        gtk::DialogFlags::MODAL,
        gtk::MessageType::Warning,
        gtk::ButtonsType::None,
        title,
    );
    dialog.set_secondary_text(Some(message));
    dialog.add_button("Cancel", gtk::ResponseType::Cancel);
    dialog.add_button(action, gtk::ResponseType::Accept);
    dialog.set_default_response(gtk::ResponseType::Cancel);

    let response = dialog.run();
    // SAFETY: the dialog is ours alone and nothing reads it after this.
    unsafe { dialog.destroy() };
    response == gtk::ResponseType::Accept
}

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
fn confirm_dialog(_title: &str, _message: &str, _action: &str) -> bool {
    false
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

// Linux: `rfd` uses the XDG desktop portal (or GTK) for a native dialog, run
// on the main thread here (via `dispatch`) like the other platforms.
#[cfg(not(any(target_os = "macos", windows)))]
fn save_panel(suggested: &str) -> Option<PathBuf> {
    rfd::FileDialog::new().set_file_name(suggested).save_file()
}

#[cfg(not(any(target_os = "macos", windows)))]
fn open_panel(extensions: &[String]) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new();
    if !extensions.is_empty() {
        let exts: Vec<&str> = extensions.iter().map(String::as_str).collect();
        dialog = dialog.add_filter("Supported", &exts);
    }

    dialog.pick_file()
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use objc2::runtime::AnyObject;
    use objc2::{class, sel};

    type Obj = *mut AnyObject;

    /// A wrong message type aborts at runtime only.
    #[test]
    fn every_alert_send_matches_its_method() {
        let alert = class!(NSAlert);
        alert
            .verify_sel::<(Obj,), ()>(sel!(setMessageText:))
            .unwrap();
        alert
            .verify_sel::<(Obj,), ()>(sel!(setInformativeText:))
            .unwrap();
        alert
            .verify_sel::<(usize,), ()>(sel!(setAlertStyle:))
            .unwrap();
        alert
            .verify_sel::<(Obj,), Obj>(sel!(addButtonWithTitle:))
            .unwrap();
        alert.verify_sel::<(), isize>(sel!(runModal)).unwrap();
    }
}
