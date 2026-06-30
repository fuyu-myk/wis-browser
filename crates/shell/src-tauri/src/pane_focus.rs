//! Native click-to-focus for split panes (macOS).
//!
//! A native tab webview swallows its own mouse events, so a click inside a page
//! never reaches the chrome. An app-wide local mouse-down monitor that hit-tests the click
//! against the view hierarchy is installed. It finds the `WKWebView` it landed in, reads the
//! tab id stamped on that view's `accessibilityIdentifier`, and emits
//! [`PANE_FOCUS_EVENT`] for the chrome to focus that pane.

use tauri::AppHandle;

#[cfg(target_os = "macos")]
use tauri::Emitter;

/// Emitted with a tab id (bare uuid string, matching `Tab.id`) when the user
/// clicks inside that tab's webview.
pub const PANE_FOCUS_EVENT: &str = "wis-pane-focus";

/// The `accessibilityIdentifier` stamped on each tab `WKWebView`. Mirrors the
/// webview label `tab-<uuid>`; `install` strips the prefix before emitting.
#[cfg(any(target_os = "macos", windows))]
const LABEL_PREFIX: &str = "tab-";

/// Mirror of Cocoa's `NSPoint` (a `CGPoint`), with the objc2 encoding so it can
/// be returned from / passed to `msg_send!`.
#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct NSPoint {
    x: f64,
    y: f64,
}

#[cfg(target_os = "macos")]
unsafe impl objc2::Encode for NSPoint {
    const ENCODING: objc2::Encoding = objc2::Encoding::Struct(
        "CGPoint",
        &[
            <f64 as objc2::Encode>::ENCODING,
            <f64 as objc2::Encode>::ENCODING,
        ],
    );
}

#[cfg(target_os = "macos")]
pub fn install(app: &AppHandle) {
    use block2::RcBlock;
    use objc2::msg_send;
    use objc2::runtime::{AnyObject, Bool};
    use std::ffi::CStr;
    use std::os::raw::c_char;

    // NSEventMaskLeftMouseDown = 1 << NSEventTypeLeftMouseDown(1).
    const MASK_LEFT_MOUSE_DOWN: u64 = 1 << 1;

    let app = app.clone();
    let handler = RcBlock::new(move |event: *mut AnyObject| -> *mut AnyObject {
        if event.is_null() {
            return event;
        }

        unsafe {
            let window: *mut AnyObject = msg_send![event, window];
            if window.is_null() {
                return event;
            }
            let content_view: *mut AnyObject = msg_send![window, contentView];
            if content_view.is_null() {
                return event;
            }
            let loc: NSPoint = msg_send![event, locationInWindow];
            let hit: *mut AnyObject = msg_send![content_view, hitTest: loc];
            if hit.is_null() {
                return event;
            }

            let wk_class = objc2::class!(WKWebView);
            let mut view = hit;
            while !view.is_null() {
                let is_wk: Bool = msg_send![view, isKindOfClass: wk_class];
                if is_wk.as_bool() {
                    let ident: *mut AnyObject = msg_send![view, accessibilityIdentifier];
                    if !ident.is_null() {
                        let utf8: *const c_char = msg_send![ident, UTF8String];
                        if !utf8.is_null() {
                            if let Ok(s) = CStr::from_ptr(utf8).to_str() {
                                if let Some(uuid) = s.strip_prefix(LABEL_PREFIX) {
                                    let _ = app.emit(PANE_FOCUS_EVENT, uuid.to_string());
                                }
                            }
                        }
                    }

                    break;
                }

                view = msg_send![view, superview];
            }
        }

        event
    });

    unsafe {
        let _token: *mut AnyObject = msg_send![
            objc2::class!(NSEvent),
            addLocalMonitorForEventsMatchingMask: MASK_LEFT_MOUSE_DOWN,
            handler: &*handler
        ];
    }

    std::mem::forget(handler);
}

#[cfg(not(target_os = "macos"))]
pub fn install(_app: &AppHandle) {}

/// Windows click-to-focus: WebView2 has no app-wide mouse monitor, so each tab
/// controller gets a `GotFocus` handler that emits [`PANE_FOCUS_EVENT`] with the
/// tab uuid when the user interacts with that pane. Called per tab webview from
/// `create_tab_webview`. No-op off Windows.
#[cfg(windows)]
pub fn install_controller<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
    use tauri::{Emitter, Manager};
    use webview2_com::FocusChangedEventHandler;
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller;

    let Some(uuid) = webview
        .label()
        .strip_prefix(LABEL_PREFIX)
        .map(str::to_string)
    else {
        return;
    };

    let app = webview.app_handle().clone();
    crate::win_webview2::with_controller(webview, move |controller| {
        let handler = FocusChangedEventHandler::create(Box::new(
            move |_sender: Option<ICoreWebView2Controller>,
                  _args: Option<windows::core::IUnknown>|
                  -> windows::core::Result<()> {
                // `GotFocus` also fires on window re-activation and programmatic
                // focus.
                if left_button_down() {
                    let _ = app.emit(PANE_FOCUS_EVENT, uuid.clone());
                }
                Ok(())
            },
        ));
        let mut token = 0_i64;
        // SAFETY: runs on the UI thread via `with_controller`.
        let _ = unsafe { controller.add_GotFocus(&handler, &mut token) };
    });
}

/// True while a physical left mouse button is held, so a `GotFocus` event can be
/// distinguished from a click-driven one.
#[cfg(windows)]
fn left_button_down() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_LBUTTON};
    // The high bit of GetKeyState is set while the key/button is down.
    (unsafe { GetKeyState(VK_LBUTTON.0 as i32) } as u16 & 0x8000) != 0
}

#[cfg(not(windows))]
pub fn install_controller<R: tauri::Runtime>(_webview: &tauri::webview::Webview<R>) {}
