//! Hand a URL off to the OS default browser.
//!
//! An exit hatch for pages Wis's embedded WebKit cannot fully handle (most
//! notably Widevine-protected video on Netflix and similar): the page opens in
//! a full, separately installed browser instead of dead-ending here.

use tauri::command;
use url::Url;
use wis_core::{WisError, WisResult};

/// The app's own bundle id. If it is the default web handler, opening "the default
/// browser" would loop straight back into Wis, so the macOS path falls back to
/// Safari in that case.
#[cfg(target_os = "macos")]
pub(crate) const WIS_BUNDLE_ID: &str = "app.myk.wis";

/// Open `url` in the user's default browser. Only `http(s)` is accepted so this
/// can never be used to launch arbitrary apps or `file://` paths.
#[command]
pub async fn open_in_external_browser(url: String) -> WisResult<()> {
    let parsed = Url::parse(&url).map_err(|_| WisError::InvalidUrl { url: url.clone() })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(WisError::InvalidInput {
            message: format!(
                "refusing to open a non-web URL externally: {}",
                parsed.scheme()
            ),
        });
    }

    open_external(parsed.as_str())
}

fn spawn(mut cmd: std::process::Command) -> WisResult<()> {
    cmd.spawn()
        .map(|_| ())
        .map_err(|err| WisError::InvalidInput {
            message: format!("failed to open external browser: {err}"),
        })
}

#[cfg(target_os = "macos")]
fn open_external(url: &str) -> WisResult<()> {
    let mut cmd = std::process::Command::new("open");

    if default_browser_is_wis(url) {
        cmd.arg("-a").arg("Safari");
    }

    cmd.arg(url);
    spawn(cmd)
}

/// True if the macOS default handler for `url` is Wis itself. Best-effort: any
/// failure resolves to `false`, so the normal default-browser path is used.
/// This is also the status probe for the default-browser settings row.
#[cfg(target_os = "macos")]
pub(crate) fn default_browser_is_wis(url: &str) -> bool {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;

    let Ok(c_url) = CString::new(url) else {
        return false;
    };

    objc2::rc::autoreleasepool(|_| unsafe {
        let url_string: *mut AnyObject =
            msg_send![objc2::class!(NSString), stringWithUTF8String: c_url.as_ptr()];
        if url_string.is_null() {
            return false;
        }
        let nsurl: *mut AnyObject = msg_send![objc2::class!(NSURL), URLWithString: url_string];
        if nsurl.is_null() {
            return false;
        }
        let workspace: *mut AnyObject = msg_send![objc2::class!(NSWorkspace), sharedWorkspace];
        if workspace.is_null() {
            return false;
        }
        let app_url: *mut AnyObject = msg_send![workspace, URLForApplicationToOpenURL: nsurl];
        if app_url.is_null() {
            return false;
        }
        let bundle: *mut AnyObject = msg_send![objc2::class!(NSBundle), bundleWithURL: app_url];
        if bundle.is_null() {
            return false;
        }
        let bundle_id: *mut AnyObject = msg_send![bundle, bundleIdentifier];
        if bundle_id.is_null() {
            return false;
        }
        let utf8: *const c_char = msg_send![bundle_id, UTF8String];
        if utf8.is_null() {
            return false;
        }
        CStr::from_ptr(utf8)
            .to_string_lossy()
            .eq_ignore_ascii_case(WIS_BUNDLE_ID)
    })
}

#[cfg(target_os = "linux")]
fn open_external(url: &str) -> WisResult<()> {
    let mut cmd = std::process::Command::new("xdg-open");
    cmd.arg(url);
    spawn(cmd)
}

#[cfg(target_os = "windows")]
fn open_external(url: &str) -> WisResult<()> {
    // `cmd /C start "" <url>` launches the default handler. The empty quoted
    // string is `start`'s window-title argument, not the URL.
    let mut cmd = std::process::Command::new("cmd");
    cmd.args(["/C", "start", ""]).arg(url);
    spawn(cmd)
}
