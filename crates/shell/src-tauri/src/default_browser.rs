//! Register Wis as the OS default browser and open links handed in from
//! other apps.
//!
//! Registration is per-platform:
//!
//! - macOS asks Launch Services (the system confirmation dialog)
//! - Windows writes the HKCU browser-registration keys and opens the Default
//!   Apps settings page
//! - Linux asks `xdg-settings`.
//!
//! Incoming URLs arrive as Apple Events (`RunEvent::Opened`, macOS) or process
//! argv (Windows/Linux, second instances forwarded by the single-instance
//! plugin) and queue in [`EXTERNAL_URLS`] until the main chrome has mounted
//! and drained them.

use tauri::{AppHandle, Emitter};
use wis_core::WisResult;

#[cfg(target_os = "linux")]
use wis_core::WisError;

/// Chrome-side event carrying an externally opened URL (bare string payload).
/// Unlike `wis-open-in-tab-fg`, it is handled by the main window regardless of
/// focus.
pub const OPEN_EXTERNAL_EVENT: &str = "wis-open-external";

/// The desktop entry the Linux bundler emits (`{productName}.desktop`), which
/// is the id `xdg-settings` expects for the default-web-browser check/set.
#[cfg(target_os = "linux")]
const LINUX_DESKTOP_FILE: &str = "Wis.desktop";

/// External URLs that arrived before the main chrome mounted. `ready` flips on
/// the chrome's one-shot drain; later arrivals emit straight to it.
struct ExternalUrlQueue {
    ready: bool,
    pending: Vec<String>,
}

/// A module static that is deliberately not `AppState`.
static EXTERNAL_URLS: std::sync::Mutex<ExternalUrlQueue> =
    std::sync::Mutex::new(ExternalUrlQueue {
        ready: false,
        pending: Vec::new(),
    });

/// Accept only an external well-formed web URL; anything else (files,
/// `javascript:`, custom schemes) is dropped at this boundary.
fn accept_external_url(raw: &str) -> Option<String> {
    let parsed = url::Url::parse(raw.trim()).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") || !parsed.has_host() {
        return None;
    }

    Some(parsed.into())
}

/// Validate and dispatch URLs handed to Wis from outside (Apple Events or
/// argv): emitted to the chrome when it is up, queued otherwise.
pub fn handle_external_urls(app: &AppHandle, raw: impl IntoIterator<Item = String>) {
    let accepted: Vec<String> = raw
        .into_iter()
        .filter_map(|u| accept_external_url(&u))
        .collect();
    if accepted.is_empty() {
        return;
    }

    {
        // The queue is plain data, so a poisoned lock still holds a valid
        // queue; panicking here would abort inside native callbacks.
        let mut queue = EXTERNAL_URLS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !queue.ready {
            queue.pending.extend(accepted);
            return;
        }
    }

    for url in accepted {
        if let Err(err) = app.emit(OPEN_EXTERNAL_EVENT, url) {
            log::warn!("external open: emit failed: {err}");
        }
    }
}

/// One-shot handover of externally opened URLs to the chrome: returns anything
/// queued before the main window mounted and marks the pipe live, so later
/// arrivals emit [`OPEN_EXTERNAL_EVENT`] directly.
#[tauri::command]
pub async fn drain_external_urls() -> WisResult<Vec<String>> {
    let mut queue = EXTERNAL_URLS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    queue.ready = true;
    Ok(std::mem::take(&mut queue.pending))
}

/// True when Wis is the OS default browser. Best-effort: unknown reads false.
#[tauri::command]
pub async fn default_browser_status() -> WisResult<bool> {
    let status = tauri::async_runtime::spawn_blocking(is_default_browser).await;
    Ok(status
        .map_err(|err| log::warn!("default-browser probe panicked: {err}"))
        .unwrap_or(false))
}

/// Ask the OS to make Wis the default browser. macOS shows the system
/// confirmation dialog (outcome lands asynchronously, the settings row
/// re-checks on focus); Windows registers Wis and opens Default Apps; Linux
/// applies immediately via `xdg-settings`.
#[tauri::command]
pub async fn set_default_browser(app: AppHandle) -> WisResult<()> {
    #[cfg(target_os = "macos")]
    {
        app.run_on_main_thread(|| {
            if let Err(err) = macos::request_set_default() {
                log::warn!("set default browser failed: {err}");
            }
        })
        .map_err(|err| wis_core::WisError::InvalidInput {
            message: format!("could not reach the main thread: {err}"),
        })
    }

    #[cfg(windows)]
    {
        let _ = app;
        tauri::async_runtime::spawn_blocking(|| {
            crate::win_default_browser::register()?;
            crate::win_default_browser::open_default_apps_settings()
        })
        .await
        .map_err(|err| wis_core::WisError::InvalidInput {
            message: format!("set-default task failed: {err}"),
        })?
    }

    #[cfg(target_os = "linux")]
    {
        let _ = app;
        tauri::async_runtime::spawn_blocking(set_default_linux)
            .await
            .map_err(|err| WisError::InvalidInput {
                message: format!("set-default task failed: {err}"),
            })?
    }
}

#[cfg(target_os = "macos")]
fn is_default_browser() -> bool {
    crate::external::default_browser_is_wis("https://example.com")
}

#[cfg(windows)]
fn is_default_browser() -> bool {
    crate::win_default_browser::is_default()
}

#[cfg(target_os = "linux")]
fn is_default_browser() -> bool {
    std::process::Command::new("xdg-settings")
        .args(["get", "default-web-browser"])
        .output()
        .map(|out| {
            String::from_utf8_lossy(&out.stdout)
                .trim()
                .eq_ignore_ascii_case(LINUX_DESKTOP_FILE)
        })
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn set_default_linux() -> WisResult<()> {
    let status = std::process::Command::new("xdg-settings")
        .args(["set", "default-web-browser", LINUX_DESKTOP_FILE])
        .status()
        .map_err(|err| WisError::InvalidInput {
            message: format!("xdg-settings failed to run: {err}"),
        })?;
    if !status.success() {
        return Err(WisError::InvalidInput {
            message: format!("xdg-settings exited with {status}"),
        });
    }

    Ok(())
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send, sel};
    use wis_core::{WisError, WisResult};

    // Launch Services entry point predating the NSWorkspace API; still
    // functional and the fallback for macOS < 12.
    #[link(name = "CoreServices", kind = "framework")]
    extern "C" {
        fn LSSetDefaultHandlerForURLScheme(
            scheme: *mut AnyObject,
            bundle_id: *mut AnyObject,
        ) -> i32;
    }

    fn nsstring(s: &str) -> *mut AnyObject {
        let Ok(c) = std::ffi::CString::new(s) else {
            return std::ptr::null_mut();
        };
        // SAFETY: class message with a valid NUL-terminated C string; the
        // returned autoreleased object is used within this call stack only.
        unsafe { msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()] }
    }

    /// Must run on the main thread: presents the system confirmation dialog.
    pub(super) fn request_set_default() -> WisResult<()> {
        // SAFETY: AppKit class objects are static; every returned pointer is
        // null-checked before the next send; caller guarantees main thread.
        unsafe {
            let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
            let bundle: *mut AnyObject = msg_send![class!(NSBundle), mainBundle];
            let bundle_url: *mut AnyObject = if bundle.is_null() {
                std::ptr::null_mut()
            } else {
                msg_send![bundle, bundleURL]
            };
            let scheme = nsstring("http");

            let responds: bool = if workspace.is_null() {
                false
            } else {
                msg_send![
                    workspace,
                    respondsToSelector:
                        sel!(setDefaultApplicationAtURL:toOpenURLsWithScheme:completionHandler:)
                ]
            };

            if responds && !bundle_url.is_null() && !scheme.is_null() {
                let nil_block: *mut AnyObject = std::ptr::null_mut();
                let _: () = msg_send![
                    workspace,
                    setDefaultApplicationAtURL: bundle_url,
                    toOpenURLsWithScheme: scheme,
                    completionHandler: nil_block
                ];
                return Ok(());
            }

            for scheme_name in ["http", "https"] {
                let scheme = nsstring(scheme_name);
                let bundle_id = nsstring(crate::external::WIS_BUNDLE_ID);
                if scheme.is_null() || bundle_id.is_null() {
                    return Err(WisError::InvalidInput {
                        message: "could not build Launch Services arguments".to_string(),
                    });
                }

                let status = LSSetDefaultHandlerForURLScheme(scheme, bundle_id);
                if status != 0 {
                    return Err(WisError::InvalidInput {
                        message: format!(
                            "Launch Services refused the {scheme_name} handler change: {status}"
                        ),
                    });
                }
            }

            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::accept_external_url;

    #[test]
    fn accepts_and_normalizes_web_urls() {
        assert_eq!(
            accept_external_url("https://example.com/a?b=c"),
            Some("https://example.com/a?b=c".to_string())
        );
        assert_eq!(
            accept_external_url("HTTP://Example.com"),
            Some("http://example.com/".to_string())
        );
        assert_eq!(
            accept_external_url("  https://example.com  "),
            Some("https://example.com/".to_string())
        );
        // WHATWG parsing skips surplus slashes: the first path segment
        // becomes the host, matching what every browser does.
        assert_eq!(
            accept_external_url("https:///path"),
            Some("https://path/".to_string())
        );
    }

    #[test]
    fn rejects_non_web_schemes() {
        for url in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,hi",
            "wis://internal",
            "ftp://example.com",
        ] {
            assert_eq!(accept_external_url(url), None, "accepted {url}");
        }
    }

    #[test]
    fn rejects_malformed_or_hostless_input() {
        for url in ["", "   ", "not a url", "http://"] {
            assert_eq!(accept_external_url(url), None, "accepted {url:?}");
        }
    }
}
