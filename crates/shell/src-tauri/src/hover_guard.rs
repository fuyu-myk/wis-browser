//! Suppress CSS `:hover` on tab webviews occluded by a popup surface (macOS).
//!
//! An overlaying webview (widget panel) or a floating window (video popout)
//! that sits over a tab does not stop the covered tab `WKWebView` from updating
//! `:hover`. Each covering surface carries a tracking area; while the cursor is
//! over it, the covered window's tab pages get a scoped stylesheet that disables
//! `pointer-events`, removed when the cursor leaves. The suppression only holds
//! while the cursor is on the covering surface.
//!
//! Scope: the widget panel (a same-window child webview, suppresses its window's
//! tabs), the video popout window (a floating window over the tab's origin
//! window, suppresses the *origin* window's other tabs), and engine `window.open`
//! popups (free-floating native windows, suppress *every* window's tabs).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use tauri::{AppHandle, Manager};

static APP: OnceLock<AppHandle> = OnceLock::new();

pub fn set_app_handle(app: AppHandle) {
    let _ = APP.set(app);
}

const TAB_LABEL_PREFIX: &str = "tab-";

/// Inject a scoped stylesheet and add the guard class so every element stops
/// receiving pointer events; the next hover recompute then clears `:hover`.
const SUPPRESS_JS: &str = "(function(){var d=document.documentElement;if(!document.getElementById('__wisHoverGuard')){var s=document.createElement('style');s.id='__wisHoverGuard';s.textContent='html.__wis-hover-guard *{pointer-events:none!important;}';(document.head||d).appendChild(s);}d.classList.add('__wis-hover-guard');})();";
/// Drop the guard class (idempotent).
const RESTORE_JS: &str =
    "(function(){document.documentElement.classList.remove('__wis-hover-guard');})();";

/// Suppress or restore `:hover` on every tab webview in `window_label`. A
/// popped-out tab is naturally excluded, since its `window()` is the popout.
pub fn set_window_tabs_suppressed(window_label: &str, suppressed: bool) {
    let Some(app) = APP.get() else {
        return;
    };

    let js = if suppressed { SUPPRESS_JS } else { RESTORE_JS };
    for (label, wv) in app.webviews() {
        if !label.starts_with(TAB_LABEL_PREFIX) {
            continue;
        }
        if wv.window().label() != window_label {
            continue;
        }

        let _ = wv.eval(js);
    }
}

/// Install the cursor-over-panel hover guard on a widget panel webview. While
/// the cursor is over the panel, the panel's window's tab pages have `:hover`
/// suppressed. No-op off macOS.
pub fn install_panel_hover_guard<R: tauri::Runtime>(
    webview: &tauri::webview::Webview<R>,
    window_label: &str,
) {
    #[cfg(target_os = "macos")]
    imp::install_panel_hover_guard(webview, window_label.to_string());

    #[cfg(not(target_os = "macos"))]
    let _ = (webview, window_label);
}

/// Suppress or restore `:hover` on every tab webview across all windows. Used
/// for engine popups, whose covered window is not tracked.
fn set_all_tabs_suppressed(suppressed: bool) {
    let Some(app) = APP.get() else {
        return;
    };

    let js = if suppressed { SUPPRESS_JS } else { RESTORE_JS };
    for (label, wv) in app.webviews() {
        if !label.starts_with(TAB_LABEL_PREFIX) {
            continue;
        }

        let _ = wv.eval(js);
    }
}

/// Set while an engine popup has the cursor: a poll-driven safety net (see
/// [`release_popup_suppression_if_idle`]) then knows tabs may be left inert if
/// the popup closed while the cursor was inside it.
static POPUP_SUPPRESS_ARMED: AtomicBool = AtomicBool::new(false);

/// The cursor entered an engine popup: suppress every tab's hover.
fn arm_popup_suppression() {
    POPUP_SUPPRESS_ARMED.store(true, Ordering::Relaxed);
    set_all_tabs_suppressed(true);
}

/// The cursor left an engine popup: restore every tab's hover.
fn disarm_popup_suppression() {
    set_all_tabs_suppressed(false);
}

/// Safety net called by the engine-popup poll when no popup remains live (idempotent).
pub fn release_popup_suppression_if_idle() {
    if POPUP_SUPPRESS_ARMED.swap(false, Ordering::Relaxed) {
        set_all_tabs_suppressed(false);
    }
}

/// Install the hover guard on an engine popup's `WKWebView`. Called from the
/// popup poll (`popup_chrome`) once per popup. macOS only.
///
/// # Safety
/// `content_view` must be a live `WKWebView` (NSView) pointer.
#[cfg(target_os = "macos")]
pub(crate) unsafe fn attach_popup_hover_guard(content_view: *mut objc2::runtime::AnyObject) {
    imp::attach_popup_hover_guard(content_view);
}

#[cfg(target_os = "macos")]
mod imp {
    use objc2::rc::Id;
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{
        class, declare_class, msg_send, msg_send_id, mutability, ClassType, DeclaredClass,
    };

    // NSTrackingMouseEnteredAndExited | NSTrackingActiveInKeyWindow | NSTrackingInVisibleRect.
    const TRACKING_OPTS: usize = 0x01 | 0x20 | 0x200;
    // ActiveAlways.
    const POPUP_TRACKING_OPTS: usize = 0x01 | 0x80 | 0x200;
    const OBJC_ASSOCIATION_RETAIN_NONATOMIC: usize = 1;
    static PANEL_ASSOC_KEY: u8 = 0;
    static POPUP_ASSOC_KEY: u8 = 0;

    extern "C" {
        fn objc_setAssociatedObject(
            obj: *mut AnyObject,
            key: *const std::os::raw::c_void,
            value: *mut AnyObject,
            policy: usize,
        );
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct ZeroRect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    }

    unsafe impl objc2::Encode for ZeroRect {
        const ENCODING: objc2::Encoding = objc2::Encoding::Struct(
            "CGRect",
            &[
                objc2::Encoding::Struct(
                    "CGPoint",
                    &[
                        <f64 as objc2::Encode>::ENCODING,
                        <f64 as objc2::Encode>::ENCODING,
                    ],
                ),
                objc2::Encoding::Struct(
                    "CGSize",
                    &[
                        <f64 as objc2::Encode>::ENCODING,
                        <f64 as objc2::Encode>::ENCODING,
                    ],
                ),
            ],
        );
    }

    struct PanelHoverIvars {
        window_label: String,
    }

    declare_class!(
        struct PanelHoverHandler;

        unsafe impl ClassType for PanelHoverHandler {
            type Super = NSObject;
            type Mutability = mutability::InteriorMutable;
            const NAME: &'static str = "WisPanelHoverHandler";
        }

        impl DeclaredClass for PanelHoverHandler {
            type Ivars = PanelHoverIvars;
        }

        unsafe impl PanelHoverHandler {
            #[method(mouseEntered:)]
            fn mouse_entered(&self, _event: *mut AnyObject) {
                super::set_window_tabs_suppressed(&self.ivars().window_label, true);
            }

            #[method(mouseExited:)]
            fn mouse_exited(&self, _event: *mut AnyObject) {
                super::set_window_tabs_suppressed(&self.ivars().window_label, false);
            }
        }
    );

    impl PanelHoverHandler {
        fn new(window_label: String) -> Id<Self> {
            let this = Self::alloc().set_ivars(PanelHoverIvars { window_label });
            unsafe { msg_send_id![super(this), init] }
        }
    }

    declare_class!(
        /// Owner of an engine popup's tracking area. Suppresses every tab's hover
        /// while the cursor is on the popup.
        struct PopupHoverHandler;

        unsafe impl ClassType for PopupHoverHandler {
            type Super = NSObject;
            type Mutability = mutability::InteriorMutable;
            const NAME: &'static str = "WisPopupHoverHandler";
        }

        impl DeclaredClass for PopupHoverHandler {
            type Ivars = ();
        }

        unsafe impl PopupHoverHandler {
            #[method(mouseEntered:)]
            fn mouse_entered(&self, _event: *mut AnyObject) {
                super::arm_popup_suppression();
            }

            #[method(mouseExited:)]
            fn mouse_exited(&self, _event: *mut AnyObject) {
                super::disarm_popup_suppression();
            }
        }
    );

    impl PopupHoverHandler {
        fn new() -> Id<Self> {
            unsafe { msg_send_id![Self::alloc(), init] }
        }
    }

    pub(super) unsafe fn attach_popup_hover_guard(content: *mut AnyObject) {
        if content.is_null() {
            return;
        }

        let handler = PopupHoverHandler::new();
        let rect = ZeroRect {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
        };
        let area: *mut AnyObject = msg_send![class!(NSTrackingArea), alloc];
        let area: *mut AnyObject = msg_send![
            area,
            initWithRect: rect,
            options: POPUP_TRACKING_OPTS,
            owner: &*handler,
            userInfo: std::ptr::null_mut::<AnyObject>(),
        ];
        let _: () = msg_send![content, addTrackingArea: area];
        let _: () = msg_send![area, release];

        objc_setAssociatedObject(
            content,
            &POPUP_ASSOC_KEY as *const u8 as *const std::os::raw::c_void,
            Id::as_ptr(&handler) as *mut AnyObject,
            OBJC_ASSOCIATION_RETAIN_NONATOMIC,
        );
    }

    pub(super) fn install_panel_hover_guard<R: tauri::Runtime>(
        webview: &tauri::webview::Webview<R>,
        window_label: String,
    ) {
        let _ = webview.with_webview(move |w| unsafe {
            let wk = w.inner() as *mut AnyObject;
            if wk.is_null() {
                return;
            }

            let handler = PanelHoverHandler::new(window_label);
            let rect = ZeroRect {
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: 0.0,
            };
            let area: *mut AnyObject = msg_send![class!(NSTrackingArea), alloc];
            let area: *mut AnyObject = msg_send![
                area,
                initWithRect: rect,
                options: TRACKING_OPTS,
                owner: &*handler,
                userInfo: std::ptr::null_mut::<AnyObject>(),
            ];
            let _: () = msg_send![wk, addTrackingArea: area];
            let _: () = msg_send![area, release];

            // A tracking area does not retain its owner; tie the handler to the
            // panel view so it lives as long as the panel webview.
            objc_setAssociatedObject(
                wk,
                &PANEL_ASSOC_KEY as *const u8 as *const std::os::raw::c_void,
                Id::as_ptr(&handler) as *mut AnyObject,
                OBJC_ASSOCIATION_RETAIN_NONATOMIC,
            );
        });
    }
}
