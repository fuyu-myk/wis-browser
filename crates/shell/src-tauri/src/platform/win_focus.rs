//! Windows keyboard-focus repair for browser windows.
//!
//! Every Wis webview, the chrome included, is a wry *child* webview
//! (multiwebview windows), and wry only installs its WM_SETFOCUS ->
//! `MoveFocus` forwarding subclass on the frame window for non-child
//! webviews.
//!
//! [`install`] subclasses a browser window's frame HWND and forwards a
//! frame-level WM_SETFOCUS into the window's workspace's active tab webview,
//! falling back to the chrome webview (chrome pages like settings and the
//! launchpad have no tab webview). The forward is skipped if something else
//! (a click routing into a webview) already moved focus on by the time the
//! async state lookup lands.

use tauri::{AppHandle, Manager};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{WM_DESTROY, WM_SETFOCUS};

/// Identifies this module's subclass on the frame window.
const SUBCLASS_ID: usize = 0x5749_5346; // "WISF"

struct Ctx {
    app: AppHandle,
    window_label: String,
}

/// Subclass `window`'s frame HWND so focus landing on the bare frame is
/// forwarded into a webview. Installed once per browser window at creation,
/// before the first show/focus. Must run on the main thread.
pub fn install(window: &tauri::Window) {
    let Ok(hwnd) = window.hwnd() else {
        return;
    };

    let ctx = Box::into_raw(Box::new(Ctx {
        app: window.app_handle().clone(),
        window_label: window.label().to_string(),
    }));

    // SAFETY: called on the window's owner thread.
    let ok =
        unsafe { SetWindowSubclass(HWND(hwnd.0), Some(subclass_proc), SUBCLASS_ID, ctx as usize) };
    if !ok.as_bool() {
        // SAFETY: never registered, so this is the sole owner.
        drop(unsafe { Box::from_raw(ctx) });
        log::warn!("focus subclass install failed for {}", window.label());
    }
}

/// SAFETY: standard comctl32 subclass procedure; `ref_data` is the `Box<Ctx>`
/// registered in [`install`], valid until the WM_DESTROY arm reclaims it.
unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _subclass_id: usize,
    ref_data: usize,
) -> LRESULT {
    match msg {
        WM_SETFOCUS => {
            let ctx = &*(ref_data as *const Ctx);
            restore(&ctx.app, &ctx.window_label, Some(hwnd.0 as isize));
        }
        WM_DESTROY => {
            let _ = RemoveWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID);
            drop(Box::from_raw(ref_data as *mut Ctx));
        }
        _ => {}
    }

    DefSubclassProc(hwnd, msg, wparam, lparam)
}

/// Move keyboard focus into `window_label`'s active tab webview, falling back
/// to its chrome webview. For explicit calls (an overlay that held focus just
/// closed), where the frame-focus guard must not apply.
pub fn restore_webview_focus(app: &AppHandle, window_label: &str) {
    restore(app, window_label, None);
}

fn restore(app: &AppHandle, window_label: &str, only_while_frame_focused: Option<isize>) {
    let app = app.clone();
    let window_label = window_label.to_string();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<crate::state::AppState>();
        let workspace = state.window_workspace(&window_label);
        let active_tab = {
            let browser = state.browser.lock().await;
            let ws = workspace.unwrap_or(browser.active_workspace_id);
            browser.workspace(ws).ok().and_then(|w| w.active_tab_id)
        };

        let webviews = app.webviews();
        let target = active_tab
            .map(|id| format!("tab-{}", id.as_uuid()))
            .and_then(|label| webviews.get(&label).cloned())
            .or_else(|| webviews.get(&window_label).cloned());
        let Some(webview) = target else {
            return;
        };

        crate::platform::win_webview2::with_controller(&webview, move |controller| {
            use webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC;
            use windows::Win32::UI::Input::KeyboardAndMouse::GetFocus;

            // SAFETY: runs on the UI thread via `with_controller`.
            unsafe {
                if let Some(frame) = only_while_frame_focused {
                    if GetFocus().0 as isize != frame {
                        return;
                    }
                }

                let _ = controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
            }
        });
    });
}
