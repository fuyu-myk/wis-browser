//! How each OS asks where a download goes, and opens, reveals and marks
//! its file.

use std::path::{Path, PathBuf};

/// Open a file with its default application, per OS.
#[cfg(target_os = "macos")]
pub(super) fn open_path_command(path: &Path) -> std::process::Command {
    let mut cmd = std::process::Command::new("open");
    cmd.arg(path);
    cmd
}

#[cfg(target_os = "linux")]
pub(super) fn open_path_command(path: &Path) -> std::process::Command {
    let mut cmd = std::process::Command::new("xdg-open");
    cmd.arg(path);
    cmd
}

#[cfg(windows)]
pub(super) fn open_path_command(path: &Path) -> std::process::Command {
    let path = dunce::simplified(path);
    let mut cmd = std::process::Command::new("cmd");
    cmd.args(["/C", "start", ""]).arg(path);
    cmd
}

/// Reveal a file in the OS file manager, selecting it where the platform can.
#[cfg(target_os = "macos")]
pub(super) fn reveal_path_command(path: &Path) -> std::process::Command {
    let mut cmd = std::process::Command::new("open");
    cmd.arg("-R").arg(path);
    cmd
}

#[cfg(target_os = "linux")]
pub(super) fn reveal_path_command(path: &Path) -> std::process::Command {
    let mut cmd = std::process::Command::new("xdg-open");
    cmd.arg(path.parent().unwrap_or(path));
    cmd
}

#[cfg(windows)]
pub(super) fn reveal_path_command(path: &Path) -> std::process::Command {
    let path = dunce::simplified(path);
    let mut cmd = std::process::Command::new("explorer");
    let mut arg = std::ffi::OsString::from("/select,");
    arg.push(path);
    cmd.arg(arg);
    cmd
}

/// Native save panel on the main thread (wry's download callback runs there;
/// `runModal` spins a nested event loop until the user decides). Returns the
/// chosen full path, or `None` on cancel. Downloads are macOS-only territory
/// for this panel; other platforms behave as a cancel.
#[cfg(target_os = "macos")]
pub(super) fn ask_destination(suggested: &str, start_dir: &Path) -> Option<PathBuf> {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;

    let c_dir = CString::new(start_dir.to_str()?).ok()?;
    let c_name = CString::new(suggested).ok()?;

    unsafe {
        let panel: *mut AnyObject = msg_send![class!(NSSavePanel), savePanel];
        if panel.is_null() {
            return None;
        }

        let dir_string: *mut AnyObject =
            msg_send![class!(NSString), stringWithUTF8String: c_dir.as_ptr()];
        let dir_url: *mut AnyObject =
            msg_send![class!(NSURL), fileURLWithPath: dir_string, isDirectory: true];
        if !dir_url.is_null() {
            let _: () = msg_send![panel, setDirectoryURL: dir_url];
        }

        let name_string: *mut AnyObject =
            msg_send![class!(NSString), stringWithUTF8String: c_name.as_ptr()];
        let _: () = msg_send![panel, setNameFieldStringValue: name_string];
        let _: () = msg_send![panel, setCanCreateDirectories: true];

        // NSModalResponseOK == 1.
        let response: isize = msg_send![panel, runModal];
        if response != 1 {
            return None;
        }
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

#[cfg(not(target_os = "macos"))]
pub(super) fn ask_destination(suggested: &str, start_dir: &Path) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().set_file_name(suggested);
    if start_dir.is_dir() {
        dialog = dialog.set_directory(start_dir);
    }

    dialog.save_file()
}

/// Tag a completed download with `com.apple.quarantine` so Gatekeeper treats
/// it like any browser download. Field order: flags;hex-timestamp;agent;.
#[cfg(target_os = "macos")]
pub(super) fn set_quarantine(path: &Path) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    use std::time::{SystemTime, UNIX_EPOCH};

    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let value = format!("0081;{secs:08x};Wis;");
    let cpath = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::other("path contains NUL"))?;
    let cname = CString::new("com.apple.quarantine").expect("static name has no NUL");
    let rc = unsafe {
        libc::setxattr(
            cpath.as_ptr(),
            cname.as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
            0,
        )
    };

    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }

    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub(super) fn set_quarantine(_path: &Path) -> std::io::Result<()> {
    Ok(())
}
