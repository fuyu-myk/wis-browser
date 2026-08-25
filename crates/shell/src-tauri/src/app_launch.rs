//! Custom-scheme "open in app" interception + prompt.
//!
//! The policy seams cancel these navigations and route them here:
//! a remembered `"<scheme>|<site>"` decision launches or drops
//! immediately; anything else raises the `wis-app-launch-request` prompt
//! overlay, answered by [`app_launch_decide`]. Tab navigations enter through
//! the tab builder's `on_navigation`/`on_new_window` hooks; macOS popup
//! redirects enter through the navigation delegate `popup_chrome` installs.

use std::sync::OnceLock;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Url};
use wis_core::{TabId, WisResult};

use crate::state::AppState;

static APP: OnceLock<AppHandle> = OnceLock::new();

/// Event raised when a custom-scheme navigation needs the user's decision,
/// showing the open-in-app prompt overlay.
const APP_LAUNCH_REQUEST_EVENT: &str = "wis-app-launch-request";

/// Most distinct launches awaiting a decision at once.
const MAX_PENDING: usize = 8;

/// Record the app handle so the popup navigation delegate can reach
/// [`AppState`] and emit.
pub fn set_app_handle(app: AppHandle) {
    let _ = APP.set(app);
}

/// A cancelled navigation awaiting the user's decision in the prompt overlay.
pub struct PendingAppLaunch {
    pub url: Url,
    /// Remember-key: `"<scheme>|<initiator host>"`.
    pub key: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct AppLaunchRequest {
    request_id: u64,
    url: String,
    scheme: String,
    initiator_host: Option<String>,
    app_name: Option<String>,
    has_handler: bool,
}

/// True when `url` should be cancelled in the engine and offered as an app
/// handoff instead.
pub fn should_intercept(url: &Url) -> bool {
    wis_core::app_launch::is_app_scheme(url.scheme())
}

/// Intake for tab-originated navigations: resolves the initiating site from
/// tab state off the policy callback, then defers to [`intake`].
pub fn intake_from_tab(app: &AppHandle, tab_id: TabId, url: Url) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let initiator_host = {
            let browser = state.browser.lock().await;
            browser
                .tab(tab_id)
                .ok()
                .and_then(|t| t.url.host_str().map(str::to_string))
        };

        intake(&app, initiator_host, url);
    });
}

/// Intake for label-addressed webviews with no tab (Peek, widget panels):
/// resolves the initiating site from the webview's committed URL off the
/// policy callback, then defers to [`intake`].
pub fn intake_from_webview(app: &AppHandle, label: String, url: Url) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let initiator_host = app
            .webviews()
            .get(&label)
            .and_then(|wv| wv.url().ok())
            .and_then(|u| u.host_str().map(str::to_string));
        intake(&app, initiator_host, url);
    });
}

/// [`intake`] through the startup-recorded app handle, for native callbacks
/// with no handle in scope. Deferred off the caller's thread: the delegate
/// runs on the main thread, and `intake` may do a Launch Services lookup or
/// spawn a process.
pub fn intake_global(initiator_host: Option<String>, url: Url) {
    if let Some(app) = APP.get() {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            intake(&app, initiator_host, url);
        });
    }
}

/// Decide a cancelled custom-scheme navigation: a remembered decision launches
/// or drops immediately, otherwise the prompt overlay is raised.
/// `initiator_host` is the site that triggered the launch (`None` when
/// unknown); one prompt per remember-key is in flight at a time.
pub fn intake(app: &AppHandle, initiator_host: Option<String>, url: Url) {
    let scheme = url.scheme().to_string();
    if !wis_core::app_launch::is_app_scheme(&scheme) {
        return;
    }

    let state = app.state::<AppState>();
    let key =
        wis_core::app_launch::permission_key(&scheme, initiator_host.as_deref().unwrap_or(""));

    let remembered = state
        .app_launch_permissions
        .read()
        .expect("app_launch_permissions poisoned")
        .get(&key)
        .copied();
    match remembered {
        Some(true) => {
            if let Err(err) = launch(&url) {
                log::warn!("remembered app launch for {scheme} failed: {err}");
            }
            return;
        }
        Some(false) => return,
        None => {}
    }

    let (app_name, has_handler) = handler_app(&url);

    let request_id = {
        let mut pending = state
            .pending_app_launch
            .lock()
            .expect("pending_app_launch poisoned");
        let in_flight = pending
            .iter()
            .find(|(_, p)| p.key == key)
            .map(|(id, _)| *id);
        match in_flight {
            Some(id) => id,
            None => {
                // Distinct pending prompt cap.
                if pending.len() >= MAX_PENDING {
                    return;
                }

                let id = state
                    .app_launch_seq
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                pending.insert(
                    id,
                    PendingAppLaunch {
                        url: url.clone(),
                        key,
                    },
                );

                id
            }
        }
    };

    let _ = app.emit(
        APP_LAUNCH_REQUEST_EVENT,
        AppLaunchRequest {
            request_id,
            url: url.to_string(),
            scheme,
            initiator_host,
            app_name,
            has_handler,
        },
    );
}

/// Resolve a prompted app launch. Stale ids are a no-op. A remembered
/// decision is mirrored here immediately. Non-remembered decisions
/// prompt again.
#[tauri::command]
pub async fn app_launch_decide(
    state: tauri::State<'_, AppState>,
    request_id: u64,
    granted: bool,
    remember: bool,
) -> WisResult<()> {
    let Some(pending) = state
        .pending_app_launch
        .lock()
        .expect("pending_app_launch poisoned")
        .remove(&request_id)
    else {
        return Ok(());
    };

    if remember {
        state
            .app_launch_permissions
            .write()
            .expect("app_launch_permissions poisoned")
            .insert(pending.key, granted);
    }
    if granted {
        launch(&pending.url)?;
    }

    Ok(())
}

/// Hand `url` to the OS-registered application. Callers have already vetted
/// the scheme through `is_app_scheme`.
fn launch(url: &Url) -> WisResult<()> {
    imp_launch(url)
}

#[cfg(target_os = "macos")]
fn imp_launch(url: &Url) -> WisResult<()> {
    let mut cmd = std::process::Command::new("open");
    cmd.arg(url.as_str());
    cmd.spawn()
        .map(reap_detached)
        .map_err(|err| wis_core::WisError::InvalidInput {
            message: format!("failed to open the app for {}: {err}", url.scheme()),
        })
}

/// Wait on the helper child off-thread; a dropped `Child` is never reaped
/// and would linger as a zombie per launch.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn reap_detached(mut child: std::process::Child) {
    std::thread::spawn(move || {
        let _ = child.wait();
    });
}

#[cfg(windows)]
fn imp_launch(url: &Url) -> WisResult<()> {
    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let url_w = HSTRING::from(url.as_str());
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(url_w.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    // Values above 32 are success.
    if result.0 as usize > 32 {
        Ok(())
    } else {
        Err(wis_core::WisError::InvalidInput {
            message: format!("no app could open a {} link", url.scheme()),
        })
    }
}

#[cfg(target_os = "linux")]
fn imp_launch(url: &Url) -> WisResult<()> {
    let mut cmd = std::process::Command::new("xdg-open");
    cmd.arg(url.as_str());
    cmd.spawn()
        .map(reap_detached)
        .map_err(|err| wis_core::WisError::InvalidInput {
            message: format!("failed to open the app for {}: {err}", url.scheme()),
        })
}

/// The display name of the app the OS would launch for `url`, plus whether
/// one is registered at all.
fn handler_app(url: &Url) -> (Option<String>, bool) {
    #[cfg(target_os = "macos")]
    {
        imp::handler_app(url)
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = url;
        (None, true)
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;

    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    /// Launch Services lookup mirroring `external::default_browser_is_wis`;
    /// `displayNameAtPath` yields the localized name without the `.app` suffix.
    pub(super) fn handler_app(url: &tauri::Url) -> (Option<String>, bool) {
        let Ok(c_url) = CString::new(url.as_str()) else {
            return (None, false);
        };

        objc2::rc::autoreleasepool(|_| unsafe {
            let url_string: *mut AnyObject =
                msg_send![objc2::class!(NSString), stringWithUTF8String: c_url.as_ptr()];
            if url_string.is_null() {
                return (None, false);
            }
            let nsurl: *mut AnyObject = msg_send![objc2::class!(NSURL), URLWithString: url_string];
            if nsurl.is_null() {
                return (None, false);
            }
            let workspace: *mut AnyObject = msg_send![objc2::class!(NSWorkspace), sharedWorkspace];
            if workspace.is_null() {
                return (None, false);
            }
            let app_url: *mut AnyObject = msg_send![workspace, URLForApplicationToOpenURL: nsurl];
            if app_url.is_null() {
                return (None, false);
            }

            let path: *mut AnyObject = msg_send![app_url, path];
            if path.is_null() {
                return (None, true);
            }
            let fm: *mut AnyObject = msg_send![objc2::class!(NSFileManager), defaultManager];
            if fm.is_null() {
                return (None, true);
            }
            let name: *mut AnyObject = msg_send![fm, displayNameAtPath: path];
            if name.is_null() {
                return (None, true);
            }
            let utf8: *const c_char = msg_send![name, UTF8String];
            if utf8.is_null() {
                return (None, true);
            }

            (
                Some(CStr::from_ptr(utf8).to_string_lossy().into_owned()),
                true,
            )
        })
    }
}
