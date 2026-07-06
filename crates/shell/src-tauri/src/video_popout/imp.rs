use objc2::rc::Id;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{class, declare_class, msg_send, msg_send_id, mutability, ClassType, DeclaredClass};
use tauri::{AppHandle, Emitter, Manager};

use super::{
    clamp_aspect, initial_frame, resized_frame, ActivePopout, Corner, Frame, APP, CLOSED_EVENT,
    EXIT_EVAL, MAX_DELTA, MEDIA_EVENT, OPENED_EVENT, POPOUT_WINDOW_LABEL, REQUESTED_EVENT,
    RESUME_EVAL, TAB_LABEL_PREFIX,
};
use crate::state::AppState;

declare_class!(
    struct PopoutHandler;

    unsafe impl ClassType for PopoutHandler {
        type Super = NSObject;
        type Mutability = mutability::InteriorMutable;
        const NAME: &'static str = "WisPopoutHandler";
    }

    impl DeclaredClass for PopoutHandler {
        type Ivars = ();
    }

    unsafe impl PopoutHandler {
        #[method(userContentController:didReceiveScriptMessage:)]
        fn did_receive(&self, _ucc: *mut AnyObject, message: *mut AnyObject) {
            unsafe { handle_message(message) }
        }
    }
);

impl PopoutHandler {
    fn new() -> Id<Self> {
        unsafe { msg_send_id![Self::alloc(), init] }
    }
}

// Always-active hover tracking for the PiP window.
extern "C" {
    fn objc_setAssociatedObject(
        obj: *mut AnyObject,
        key: *const std::os::raw::c_void,
        value: *mut AnyObject,
        policy: usize,
    );
}
const OBJC_ASSOCIATION_RETAIN_NONATOMIC: usize = 1;
static HOVER_ASSOC_KEY: u8 = 0;
// NSTrackingMouseEnteredAndExited | NSTrackingActiveAlways | NSTrackingInVisibleRect.
const HOVER_TRACKING_OPTS: usize = 0x01 | 0x80 | 0x200;

#[repr(C)]
#[derive(Clone, Copy)]
struct HoverRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

unsafe impl objc2::Encode for HoverRect {
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

struct HoverIvars {
    /// The reparented tab WKWebView the handler evals into. wry leaks this on
    /// drop, so the raw pointer stays valid for the handler's lifetime.
    webview: *mut AnyObject,
}

declare_class!(
    struct HoverHandler;

    unsafe impl ClassType for HoverHandler {
        type Super = NSObject;
        type Mutability = mutability::InteriorMutable;
        const NAME: &'static str = "WisPopoutHoverHandler";
    }

    impl DeclaredClass for HoverHandler {
        type Ivars = HoverIvars;
    }

    unsafe impl HoverHandler {
        #[method(mouseEntered:)]
        fn mouse_entered(&self, _event: *mut AnyObject) {
            unsafe { self.eval_hover(true) }
        }

        #[method(mouseExited:)]
        fn mouse_exited(&self, _event: *mut AnyObject) {
            unsafe { self.eval_hover(false) }
        }
    }
);

impl HoverHandler {
    fn new(webview: *mut AnyObject) -> Id<Self> {
        let this = Self::alloc().set_ivars(HoverIvars { webview });
        unsafe { msg_send_id![super(this), init] }
    }

    unsafe fn eval_hover(&self, on: bool) {
        let wk = self.ivars().webview;
        if wk.is_null() {
            return;
        }
        let js = if on {
            "window.__wisPopoutHover&&window.__wisPopoutHover(true);"
        } else {
            "window.__wisPopoutHover&&window.__wisPopoutHover(false);"
        };
        let s = ns_string(js);
        let _: () = msg_send![
            wk,
            evaluateJavaScript: s,
            completionHandler: std::ptr::null_mut::<AnyObject>(),
        ];
    }
}

/// Install the always-active hover tracking on the popout window's content
/// view. The handler is associated with and retained by the content view.
/// Runs inside `with_webview` so only main-thread-safe pointers are touched.
fn install_hover_tracking<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
    let _ = webview.with_webview(|w| unsafe {
        let wk = w.inner() as *mut AnyObject;
        if wk.is_null() {
            return;
        }

        let window: *mut AnyObject = msg_send![wk, window];
        if window.is_null() {
            return;
        }

        let content: *mut AnyObject = msg_send![window, contentView];
        if content.is_null() {
            return;
        }

        // Let the window post mouse-moved even when it is not key.
        let _: () = msg_send![window, setAcceptsMouseMovedEvents: objc2::runtime::Bool::YES];

        let handler = HoverHandler::new(wk);
        let rect = HoverRect {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
        };
        let area: *mut AnyObject = msg_send![class!(NSTrackingArea), alloc];
        let area: *mut AnyObject = msg_send![
            area,
            initWithRect: rect,
            options: HOVER_TRACKING_OPTS,
            owner: &*handler,
            userInfo: std::ptr::null_mut::<AnyObject>(),
        ];
        let _: () = msg_send![content, addTrackingArea: area];
        let _: () = msg_send![area, release];

        objc_setAssociatedObject(
            content,
            &HOVER_ASSOC_KEY as *const u8 as *const std::os::raw::c_void,
            Id::as_ptr(&handler) as *mut AnyObject,
            OBJC_ASSOCIATION_RETAIN_NONATOMIC,
        );
    });
}

unsafe fn ns_string(s: &str) -> *mut AnyObject {
    let c = std::ffi::CString::new(s).unwrap_or_default();
    msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()]
}

unsafe fn nsstring_to_string(ns: *mut AnyObject) -> Option<String> {
    if ns.is_null() {
        return None;
    }

    let is_str: bool = msg_send![ns, isKindOfClass: class!(NSString)];
    if !is_str {
        return None;
    }

    let utf8: *const std::os::raw::c_char = msg_send![ns, UTF8String];
    if utf8.is_null() {
        return None;
    }

    Some(
        std::ffi::CStr::from_ptr(utf8)
            .to_string_lossy()
            .into_owned(),
    )
}

unsafe fn dict_string(dict: *mut AnyObject, key: &str) -> Option<String> {
    let k = ns_string(key);
    let v: *mut AnyObject = msg_send![dict, objectForKey: k];
    nsstring_to_string(v)
}

unsafe fn dict_f64(dict: *mut AnyObject, key: &str) -> Option<f64> {
    let k = ns_string(key);
    let v: *mut AnyObject = msg_send![dict, objectForKey: k];
    if v.is_null() {
        return None;
    }

    let is_num: bool = msg_send![v, isKindOfClass: class!(NSNumber)];
    if !is_num {
        return None;
    }

    let d: f64 = msg_send![v, doubleValue];
    d.is_finite().then_some(d)
}

unsafe fn dict_bool(dict: *mut AnyObject, key: &str) -> Option<bool> {
    let k = ns_string(key);
    let v: *mut AnyObject = msg_send![dict, objectForKey: k];
    if v.is_null() {
        return None;
    }

    let is_num: bool = msg_send![v, isKindOfClass: class!(NSNumber)];
    if !is_num {
        return None;
    }

    let b: bool = msg_send![v, boolValue];
    Some(b)
}

/// The sending tab webview's label, read from the identifier this app
/// stamps on every tab WKWebView; never trusted from the message body.
unsafe fn sender_label(message: *mut AnyObject) -> Option<String> {
    let webview: *mut AnyObject = msg_send![message, webView];
    if webview.is_null() {
        return None;
    }

    let ident: *mut AnyObject = msg_send![webview, accessibilityIdentifier];
    nsstring_to_string(ident).filter(|s| s.starts_with(TAB_LABEL_PREFIX))
}

unsafe fn handle_message(message: *mut AnyObject) {
    if message.is_null() {
        return;
    }

    let body: *mut AnyObject = msg_send![message, body];
    if body.is_null() {
        return;
    }

    let is_dict: bool = msg_send![body, isKindOfClass: class!(NSDictionary)];
    if !is_dict {
        return;
    }

    let Some(label) = sender_label(message) else {
        return;
    };
    let Some(kind) = dict_string(body, "kind") else {
        return;
    };
    let Some(app) = APP.get() else {
        return;
    };

    log::debug!("video popout: page message '{kind}' from {label}");

    match kind.as_str() {
        "media" => {
            if let Some(playing) = dict_bool(body, "playing") {
                let present = dict_bool(body, "present").unwrap_or(playing);
                emit_media(app, &label, playing, present);
            }
        }
        "request" => {
            let _ = app.emit(
                REQUESTED_EVENT,
                serde_json::json!({ "tab": tab_uuid(&label) }),
            );
        }
        "entered" => {
            let aspect = dict_f64(body, "aspect").unwrap_or(16.0 / 9.0);
            on_entered(app, &label, aspect);
        }
        "aspect" => {
            if let Some(aspect) = dict_f64(body, "aspect") {
                on_aspect_changed(app, &label, aspect);
            }
        }
        "dragstart" => on_dragstart(app, &label),
        "resize" => {
            let corner = dict_string(body, "corner").and_then(|c| Corner::parse(&c));
            let dx = dict_f64(body, "dx").unwrap_or(0.0);
            let dy = dict_f64(body, "dy").unwrap_or(0.0);
            if let Some(corner) = corner {
                if dx.abs() <= MAX_DELTA && dy.abs() <= MAX_DELTA {
                    on_resize(app, &label, corner, dx, dy);
                }
            }
        }
        // The page already unwound its own styling before posting these.
        "close" | "lost" => close_and_notify(app, &label, false),
        _ => {}
    }
}

fn tab_uuid(label: &str) -> &str {
    label.strip_prefix(TAB_LABEL_PREFIX).unwrap_or(label)
}

fn emit_media(app: &AppHandle, label: &str, playing: bool, present: bool) {
    let _ = app.emit(
        MEDIA_EVENT,
        serde_json::json!({
            "tab": tab_uuid(label), "playing": playing, "present": present,
        }),
    );
}

fn frame_of_window(window: &tauri::Window) -> Option<Frame> {
    let scale = window.scale_factor().ok()?;
    let pos = window.outer_position().ok()?.to_logical::<f64>(scale);
    let size = window.inner_size().ok()?.to_logical::<f64>(scale);

    Some(Frame {
        x: pos.x,
        y: pos.y,
        w: size.width,
        h: size.height,
    })
}

fn work_area_near(window: &tauri::Window) -> Option<Frame> {
    let monitor = window.current_monitor().ok().flatten()?;
    let scale = monitor.scale_factor();
    let area = monitor.work_area();
    let pos = area.position.to_logical::<f64>(scale);
    let size = area.size.to_logical::<f64>(scale);

    Some(Frame {
        x: pos.x,
        y: pos.y,
        w: size.width,
        h: size.height,
    })
}

/// Enter handshake confirmed: build the floating window and move the
/// tab's webview into it. Runs on the main thread (script message
/// handlers are delivered there), which window creation requires.
fn on_entered(app: &AppHandle, label: &str, aspect: f64) {
    let state = app.state::<AppState>();
    let pending = {
        let mut guard = state.video_popout.lock().expect("video_popout poisoned");
        match guard.pending.take() {
            Some(p) if p.label == label => Some(p),
            other => {
                guard.pending = other;
                None
            }
        }
    };
    let Some(pending) = pending else {
        // Unsolicited (or cancelled-handshake) enter: unwind the page
        // styling it just applied.
        log::debug!("video popout: enter from {label} without a pending open; unwinding");
        if let Some(webview) = app.webviews().get(label).cloned() {
            let _ = webview.eval(EXIT_EVAL);
        }
        return;
    };

    let aspect = clamp_aspect(aspect);
    let Some(webview) = app.webviews().get(label).cloned() else {
        return;
    };
    let origin = webview.window();
    let origin_window = origin.label().to_string();
    let Some(origin_frame) = frame_of_window(&origin) else {
        return;
    };
    let work = work_area_near(&origin).unwrap_or(origin_frame);
    let frame = initial_frame(aspect, origin_frame, work, pending.remembered_pos);

    // A stale window can exist when a popout is re-opened before the
    // previous teardown's async close lands.
    if let Some(stale) = app.get_window(POPOUT_WINDOW_LABEL) {
        log::debug!("video popout: destroying stale popout window");
        let _ = stale.destroy();
    }

    let window = tauri::window::WindowBuilder::new(app, POPOUT_WINDOW_LABEL)
        .decorations(false)
        .transparent(true)
        .resizable(false)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        .focused(false)
        .inner_size(frame.w, frame.h)
        .position(frame.x, frame.y)
        .build();
    let window = match window {
        Ok(w) => w,
        Err(err) => {
            log::warn!("video popout: window build failed: {err}");
            let _ = webview.eval(EXIT_EVAL);
            return;
        }
    };

    if let Err(err) = webview.reparent(&window) {
        log::warn!("video popout: reparent failed: {err}");
        let _ = window.close();
        let _ = webview.eval(EXIT_EVAL);
        return;
    }

    let _ = webview.set_position(tauri::LogicalPosition::new(0.0, 0.0));
    let _ = webview.set_size(tauri::LogicalSize::new(frame.w, frame.h));
    // AppKit itself keeps the webview glued to the window from here:
    // autoresizing is applied synchronously during the window's own
    // setFrame, immune to the cross-thread message timing that left
    // bare window surface beside the video at larger sizes.
    set_fills_window(&webview, true);
    let _ = webview.show();
    install_hover_tracking(&webview);

    {
        let mut guard = state.video_popout.lock().expect("video_popout poisoned");
        guard.active = Some(ActivePopout {
            label: label.to_string(),
            aspect,
            origin_window,
        });
    }

    let _ = webview.eval(RESUME_EVAL);
    log::info!(
        "video popout: opened for {label} at {:.0}x{:.0}+{:.0}+{:.0}",
        frame.w,
        frame.h,
        frame.x,
        frame.y
    );
    let _ = app.emit(OPENED_EVENT, serde_json::json!({ "tab": tab_uuid(label) }));
}

fn on_dragstart(app: &AppHandle, label: &str) {
    let state = app.state::<AppState>();
    let is_active = {
        let guard = state.video_popout.lock().expect("video_popout poisoned");
        guard.active.as_ref().is_some_and(|a| a.label == label)
    };
    if !is_active {
        return;
    }

    if let Some(window) = app.get_window(POPOUT_WINDOW_LABEL) {
        let _ = window.start_dragging();
    }
}

fn on_resize(app: &AppHandle, label: &str, corner: Corner, dx: f64, dy: f64) {
    let state = app.state::<AppState>();
    let aspect = {
        let guard = state.video_popout.lock().expect("video_popout poisoned");
        match guard.active.as_ref() {
            Some(a) if a.label == label => a.aspect,
            _ => return,
        }
    };
    let Some(window) = app.get_window(POPOUT_WINDOW_LABEL) else {
        return;
    };
    let Some(frame) = frame_of_window(&window) else {
        return;
    };
    let work = work_area_near(&window).unwrap_or(frame);

    let next = resized_frame(frame, corner, dx, dy, aspect, work);
    if next != frame {
        apply_popout_frame(&window, next);
    }
}

/// While the webview lives in the popout window it carries the
/// width+height-sizable autoresizing mask, so AppKit keeps it exactly
/// filling the content view through every window resize. Cleared on
/// restore: in the main window the chrome owns the bounds explicitly.
fn set_fills_window<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>, fill: bool) {
    let _ = webview.with_webview(move |w| unsafe {
        let wk: *mut AnyObject = w.inner() as *mut AnyObject;
        if wk.is_null() {
            return;
        }
        // NSViewWidthSizable | NSViewHeightSizable.
        let mask: usize = if fill { (1 << 1) | (1 << 4) } else { 0 };
        let _: () = msg_send![wk, setAutoresizingMask: mask];
    });
}

/// Resize the popout window; the autoresizing mask carries its webview
/// along, so the video never sits beside bare window surface.
fn apply_popout_frame(window: &tauri::Window, frame: Frame) {
    let _ = window.set_position(tauri::LogicalPosition::new(frame.x, frame.y));
    let _ = window.set_size(tauri::LogicalSize::new(frame.w, frame.h));
}

/// The popped video's intrinsic aspect changed (adaptive-stream rendition
/// switch, autoplay next video): re-lock the window to the new ratio,
/// keeping the current width and top-left anchor, so `object-fit:
/// contain` never letterboxes.
fn on_aspect_changed(app: &AppHandle, label: &str, aspect: f64) {
    let state = app.state::<AppState>();
    let aspect = clamp_aspect(aspect);
    {
        let mut guard = state.video_popout.lock().expect("video_popout poisoned");
        match guard.active.as_mut() {
            Some(a) if a.label == label => {
                if (a.aspect - aspect).abs() < 0.01 {
                    return;
                }
                a.aspect = aspect;
            }
            _ => return,
        }
    }
    let Some(window) = app.get_window(POPOUT_WINDOW_LABEL) else {
        return;
    };
    let Some(frame) = frame_of_window(&window) else {
        return;
    };

    let work = work_area_near(&window).unwrap_or(frame);
    let next = resized_frame(frame, Corner::SouthEast, 0.0, 0.0, aspect, work);
    if next != frame {
        apply_popout_frame(&window, next);
    }
}

/// Tear the popout down: unwind page styling (when asked), reparent the
/// webview back into the main window hidden, close the floating window,
/// and tell the chrome (with the final geometry when one was live).
/// Idempotent; emits the closed event even when nothing was active so
/// chrome-side bookkeeping can never wedge.
pub fn close_and_notify(app: &AppHandle, label: &str, eval_exit: bool) {
    let state = app.state::<AppState>();
    let (was_active, origin_window) = {
        let mut guard = state.video_popout.lock().expect("video_popout poisoned");
        match guard.active.as_ref() {
            Some(a) if a.label == label => {
                let origin = a.origin_window.clone();
                guard.active = None;
                (true, Some(origin))
            }
            _ => (false, None),
        }
    };

    let window = app.get_window(POPOUT_WINDOW_LABEL);
    let final_frame = if was_active {
        window.as_ref().and_then(frame_of_window)
    } else {
        None
    };

    if was_active {
        if let Some(webview) = app.webviews().get(label).cloned() {
            if eval_exit {
                let _ = webview.eval(EXIT_EVAL);
            }
            set_fills_window(&webview, false);
            let _ = webview.hide();
            // Reparent back into the tab's origin window; fall back to main if
            // that window has since closed.
            let home = origin_window
                .as_deref()
                .and_then(|label| app.get_window(label))
                .or_else(|| app.get_window("main"));
            if let Some(home) = home {
                if let Err(err) = webview.reparent(&home) {
                    log::warn!("video popout: reparent back failed: {err}");
                }
            }
            // The reparent back paused the video; resume it so folding
            // the popout in never silences a playing video.
            let _ = webview.eval(RESUME_EVAL);
        }
        if let Some(window) = window {
            let _ = window.close();
        }
    }

    log::debug!("video popout: closed for {label} (was_active={was_active})");
    let payload = match final_frame {
        Some(f) => serde_json::json!({
            "tab": tab_uuid(label), "x": f.x, "y": f.y,
        }),
        None => serde_json::json!({ "tab": tab_uuid(label) }),
    };
    let _ = app.emit(CLOSED_EVENT, payload);
}

/// Install the `wisPopout` message handler. Handlers (unlike user
/// scripts) apply retroactively to a loaded page, so this cannot race
/// the first document.
pub fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
    let _ = webview.with_webview(|w| unsafe {
        let wk: *mut AnyObject = w.inner() as *mut AnyObject;
        if wk.is_null() {
            return;
        }

        let config: *mut AnyObject = msg_send![wk, configuration];
        if config.is_null() {
            return;
        }

        let controller: *mut AnyObject = msg_send![config, userContentController];
        if controller.is_null() {
            return;
        }

        let handler = PopoutHandler::new();
        let name = ns_string("wisPopout");
        let _: () = msg_send![controller, addScriptMessageHandler: &*handler, name: name];
    });
}
