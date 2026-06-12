//! Video popout: a floating, always-on-top mini player in its own native
//! window, free to live outside the main browser window.
//!
//! A second video pipeline is impossible here (MSE/DRM streams cannot be
//! re-opened in another webview), so the popout *is the tab's own webview*:
//! when the chrome decides a tab should pop out, the webview is reparented
//! into a small borderless `video-popout` window and an injected script pins
//! the playing `<video>` across the whole viewport. While popped, the video
//! element is reparented to the document root (original slot restored on exit),
//! so no player wrapper can re-anchor, scale, or crop the fixed pinning, and
//! the whole `<body>` is visibility-hidden over a black root so nothing can
//! paint above it or fade it out.
//!
//! An overlay provides the controls: an X (top right),
//! click to pause/play, double-click left/right to seek -/+10s, drag
//! anywhere to move the window (native `start_dragging`), and corner zones
//! to resize aspect-locked. Returning to the tab (or X) reparents the
//! webview back into the main window and unwinds the page styling.
//!
//! Split of responsibilities: the page script only reports intents over the
//! narrow `wisPopout` message handler (it holds no Tauri capability); this
//! module owns the native window lifecycle and validates every payload; the
//! chrome decides *when* to pop out (automatically when a tab playing video
//! leaves the visible layout, or explicitly via the in-page hover button /
//! the Pop Out Video key action) and persists the remembered geometry.
//! The sender of each message is identified by the `accessibilityIdentifier`
//! stamped on the tab's WKWebView, never by page-supplied data, so a hostile
//! page can at worst fiddle with its own popout.

use std::sync::OnceLock;

use tauri::{AppHandle, Manager, State};
use wis_core::{WisError, WisResult};

use crate::state::AppState;

static APP: OnceLock<AppHandle> = OnceLock::new();

pub fn set_app_handle(app: AppHandle) {
    let _ = APP.set(app);
}

/// Label of the floating popout window (at most one exists).
pub const POPOUT_WINDOW_LABEL: &str = "video-popout";

/// Tab webview labels are `tab-<uuid>`; the uuid is what events carry.
const TAB_LABEL_PREFIX: &str = "tab-";

/// Media-state changes per tab: `{ tab, playing, present }`. `playing` gates
/// the automatic popout on tab switch; `present` (a usable video exists,
/// playing or not) gates the manual triggers. The chrome keeps both maps.
const MEDIA_EVENT: &str = "wis-popout-media";
/// The page's hover button asked to pop out: `{ tab }`. The chrome decides
/// (same gates as the Pop Out Video key action).
const REQUESTED_EVENT: &str = "wis-popout-requested";
/// The popout window opened for `{ tab }` (enter handshake completed).
const OPENED_EVENT: &str = "wis-popout-opened";
/// The popout closed for `{ tab, x?, y?, width? }`; geometry present when a
/// window was live, for the chrome to remember.
const CLOSED_EVENT: &str = "wis-popout-closed";

const ENTER_EVAL: &str = "window.__wisPopoutEnter && window.__wisPopoutEnter();";
const EXIT_EVAL: &str = "window.__wisPopoutExit && window.__wisPopoutExit();";
/// Moving a WKWebView between windows (reparent) pauses its media; this
/// re-kicks playback page-side after each move, unless the pause was the
/// user's own (X button, click-pause).
const RESUME_EVAL: &str = "window.__wisPopoutResume && window.__wisPopoutResume();";

pub const MIN_WIDTH: f64 = 240.0;
const DEFAULT_WIDTH: f64 = 440.0;
/// Breathing room kept between the popout and the screen work-area edges.
const WORK_MARGIN: f64 = 12.0;
/// Default docking inset from the main window's bottom-right corner.
const DOCK_MARGIN: f64 = 24.0;
/// Per-message resize delta cap; anything larger is a bogus payload.
const MAX_DELTA: f64 = 4000.0;

/// Popout bookkeeping on [`AppState`]. `pending` bridges the enter handshake
/// (open command issued, page not yet confirmed); `active` is the live
/// window. Both are label-keyed so stray or forged page messages for any
/// other webview are ignored.
#[derive(Default)]
pub struct VideoPopoutState {
    pub pending: Option<PendingOpen>,
    pub active: Option<ActivePopout>,
}

pub struct PendingOpen {
    pub label: String,
    pub remembered_pos: Option<(f64, f64)>,
}

pub struct ActivePopout {
    pub label: String,
    pub aspect: f64,
}

// ---------------------------------------------------------------------------
// Pure geometry (screen coordinates, logical pixels)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    NorthWest,
    NorthEast,
    SouthWest,
    SouthEast,
}

impl Corner {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "nw" => Some(Self::NorthWest),
            "ne" => Some(Self::NorthEast),
            "sw" => Some(Self::SouthWest),
            "se" => Some(Self::SouthEast),
            _ => None,
        }
    }

    fn east(self) -> bool {
        matches!(self, Self::NorthEast | Self::SouthEast)
    }

    fn south(self) -> bool {
        matches!(self, Self::SouthWest | Self::SouthEast)
    }
}

/// Sane width/height ratio; garbage collapses to 16:9.
pub fn clamp_aspect(aspect: f64) -> f64 {
    if !aspect.is_finite() || aspect <= 0.0 {
        16.0 / 9.0
    } else {
        aspect.clamp(0.2, 5.0)
    }
}

fn max_width_for(aspect: f64, work: Frame) -> f64 {
    let by_width = work.w - 2.0 * WORK_MARGIN;
    let by_height = (work.h - 2.0 * WORK_MARGIN) * aspect;
    by_width.min(by_height).max(MIN_WIDTH)
}

fn clamp_axis(v: f64, lo: f64, hi: f64) -> f64 {
    if hi < lo {
        lo
    } else {
        v.clamp(lo, hi)
    }
}

/// First frame of a fresh popout: always the predefined default size (only
/// the height varies, derived from the video's aspect), at the remembered
/// position when it still fits the work area, else docked at the main
/// window's bottom-right corner.
pub fn initial_frame(
    aspect: f64,
    main: Frame,
    work: Frame,
    remembered_pos: Option<(f64, f64)>,
) -> Frame {
    let aspect = clamp_aspect(aspect);
    let max_w = max_width_for(aspect, work);
    let w = DEFAULT_WIDTH.clamp(MIN_WIDTH, max_w);
    let h = w / aspect;
    let (x, y) = match remembered_pos {
        Some((x, y)) if x.is_finite() && y.is_finite() => (x, y),
        _ => (
            main.x + main.w - w - DOCK_MARGIN,
            main.y + main.h - h - DOCK_MARGIN,
        ),
    };

    Frame {
        x: clamp_axis(x, work.x + WORK_MARGIN, work.x + work.w - w - WORK_MARGIN),
        y: clamp_axis(y, work.y + WORK_MARGIN, work.y + work.h - h - WORK_MARGIN),
        w,
        h,
    }
}

/// Aspect-locked corner resize: the dragged corner follows the dominant axis
/// of the delta while the opposite corner stays anchored.
pub fn resized_frame(
    frame: Frame,
    corner: Corner,
    dx: f64,
    dy: f64,
    aspect: f64,
    work: Frame,
) -> Frame {
    let aspect = clamp_aspect(aspect);
    let dx = if dx.is_finite() {
        dx.clamp(-MAX_DELTA, MAX_DELTA)
    } else {
        0.0
    };
    let dy = if dy.is_finite() {
        dy.clamp(-MAX_DELTA, MAX_DELTA)
    } else {
        0.0
    };
    let dw_horizontal = if corner.east() { dx } else { -dx };
    let dw_vertical = (if corner.south() { dy } else { -dy }) * aspect;
    let dw = if dw_horizontal.abs() >= dw_vertical.abs() {
        dw_horizontal
    } else {
        dw_vertical
    };

    let w = (frame.w + dw).clamp(MIN_WIDTH, max_width_for(aspect, work));
    let h = w / aspect;

    Frame {
        x: if corner.east() {
            frame.x
        } else {
            frame.x + (frame.w - w)
        },
        y: if corner.south() {
            frame.y
        } else {
            frame.y + (frame.h - h)
        },
        w,
        h,
    }
}

fn validate_label(label: &str) -> WisResult<()> {
    let valid = label.strip_prefix(TAB_LABEL_PREFIX).is_some_and(|suffix| {
        !suffix.is_empty()
            && suffix.len() <= 64
            && suffix
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
    });
    if !valid {
        return Err(WisError::InvalidInput {
            message: format!("invalid tab webview label: {label:?}"),
        });
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Commands (chrome -> native)
// ---------------------------------------------------------------------------

/// Pop `label`'s tab out into the floating player. The page confirms via the
/// enter handshake (`entered` message), which actually creates the window;
/// if no suitable video is present the webview simply stays hidden.
/// `x`/`y` carry the chrome-remembered position of the last popout; the size
/// always starts from the predefined default.
#[tauri::command]
pub async fn open_video_popout(
    app: AppHandle,
    state: State<'_, AppState>,
    label: String,
    x: Option<f64>,
    y: Option<f64>,
) -> WisResult<()> {
    validate_label(&label)?;

    #[cfg(target_os = "macos")]
    {
        let Some(webview) = app.webviews().get(&label).cloned() else {
            return Ok(());
        };

        // Single-popout invariant: an existing popout for another tab is
        // restored first; re-opening the same tab is a no-op.
        let previous = {
            let guard = state.video_popout.lock().expect("video_popout poisoned");
            guard.active.as_ref().map(|a| a.label.clone())
        };
        if let Some(prev) = previous {
            if prev == label {
                return Ok(());
            }
            imp::close_and_notify(&app, &prev, true);
        }

        {
            let mut guard = state.video_popout.lock().expect("video_popout poisoned");
            guard.pending = Some(PendingOpen {
                label: label.clone(),
                remembered_pos: x.zip(y),
            });
        }

        webview
            .eval(ENTER_EVAL)
            .map_err(|err| WisError::InvalidInput {
                message: format!("video popout enter eval: {err}"),
            })?;
    }

    #[cfg(not(target_os = "macos"))]
    let _ = (app, state, label, x, y);

    Ok(())
}

/// Restore `label`'s tab out of the popout (page styling unwound, webview
/// reparented back into the main window, window closed). Emits the closed
/// event with the final geometry so the chrome can remember it. Safe to call
/// in any phase: a pending-only handshake is cancelled, no popout is a no-op.
#[tauri::command]
pub async fn close_video_popout(
    app: AppHandle,
    state: State<'_, AppState>,
    label: String,
) -> WisResult<()> {
    validate_label(&label)?;

    #[cfg(target_os = "macos")]
    {
        let pending_cancelled = {
            let mut guard = state.video_popout.lock().expect("video_popout poisoned");
            match &guard.pending {
                Some(p) if p.label == label => {
                    guard.pending = None;
                    true
                }
                _ => false,
            }
        };
        if pending_cancelled {
            // The page may have applied popout styling before the cancel.
            if let Some(webview) = app.webviews().get(&label).cloned() {
                let _ = webview.eval(EXIT_EVAL);
            }
        }

        imp::close_and_notify(&app, &label, true);
    }

    #[cfg(not(target_os = "macos"))]
    let _ = (app, state, label);

    Ok(())
}

/// A real navigation in a popped-out tab destroys the page (and its pinned
/// video) under the player; restore the tab to a plain hidden webview.
/// Called from the tab webview's page-load hook; no-op for other tabs.
pub fn handle_navigation(app: &AppHandle, label: &str) {
    #[cfg(target_os = "macos")]
    {
        let is_active = {
            let state = app.state::<AppState>();
            let guard = state.video_popout.lock().expect("video_popout poisoned");
            guard.active.as_ref().is_some_and(|a| a.label == label)
        };
        if is_active {
            imp::close_and_notify(app, label, false);
        }
    }

    #[cfg(not(target_os = "macos"))]
    let _ = (app, label);
}

/// Close the popout window outright (main window teardown). The reparented
/// webview dies with it; no restore is possible or needed.
pub fn close_popout_window(app: &AppHandle) {
    if let Some(window) = app.get_window(POPOUT_WINDOW_LABEL) {
        let _ = window.close();
    }
}

/// The page-side popout script, injected as a builder-level initialization
/// script (document start, main frame). Baked into the webview configuration
/// before the first document exists, so even an instantly-loading page (file
/// cache, localhost) can never race the injection. Top-level code only
/// registers listeners, so document-start execution is safe.
pub fn page_script() -> &'static str {
    POPOUT_JS
}

/// Install the `wisPopout` message handler on a tab webview (the script
/// itself rides the builder, see [`page_script`]). No-op off macOS.
pub fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
    #[cfg(target_os = "macos")]
    imp::install(webview);

    #[cfg(not(target_os = "macos"))]
    let _ = webview;
}

#[cfg(target_os = "macos")]
mod imp {
    use objc2::rc::Id;
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{
        class, declare_class, msg_send, msg_send_id, mutability, ClassType, DeclaredClass,
    };
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
        let Some(main) = app.get_window("main") else {
            return;
        };
        let Some(main_frame) = frame_of_window(&main) else {
            return;
        };
        let work = work_area_near(&main).unwrap_or(main_frame);
        let frame = initial_frame(aspect, main_frame, work, pending.remembered_pos);

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

        {
            let mut guard = state.video_popout.lock().expect("video_popout poisoned");
            guard.active = Some(ActivePopout {
                label: label.to_string(),
                aspect,
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
        let was_active = {
            let mut guard = state.video_popout.lock().expect("video_popout poisoned");
            match guard.active.as_ref() {
                Some(a) if a.label == label => {
                    guard.active = None;
                    true
                }
                _ => false,
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
                if let Some(main) = app.get_window("main") {
                    if let Err(err) = webview.reparent(&main) {
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
}

/// Injected page script (main frame, document end). Tracks the page's
/// dominant video and reports media-state changes (`playing` for the
/// automatic trigger, `present` for the manual ones); shows a small pop-out
/// button pinned to the video under the cursor; on demand
/// (`__wisPopoutEnter`) reparents the chosen video to the document root,
/// pins it across the viewport over a hidden body and black root, and
/// mounts the control overlay (X, click pause/play, double-click seek,
/// drag-to-move handoff, corner resize zones).
/// Muted, tiny, or detached videos never qualify as "playing", so muted
/// autoplay ads cannot trigger the automatic popout.
const POPOUT_JS: &str = r#"(function () {
  'use strict';
  if (window.__wisPopoutInstalled) return;
  window.__wisPopoutInstalled = true;
  if (window.top !== window) return;

  function post(msg) {
    try {
      var h = window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.wisPopout;
      if (h) h.postMessage(msg);
    } catch (e) {}
  }

  var current = null;
  var manualTarget = null;
  var manualAt = 0;
  var lastPlaying = false;
  var lastPresent = false;
  var reportTimer = null;
  var resumeTarget = null;
  var userPaused = false;

  function sizeOk(v) {
    if (!v || !v.isConnected || !v.videoWidth || !v.videoHeight) return false;
    var r = v.getBoundingClientRect();
    return r.width >= 180 && r.height >= 100;
  }
  function pick(playingOnly) {
    var vids = document.querySelectorAll('video');
    var best = null;
    var bestArea = 0;
    for (var i = 0; i < vids.length; i++) {
      var v = vids[i];
      if (!sizeOk(v)) continue;
      if (playingOnly && (v.paused || v.ended || v.muted)) continue;
      var r = v.getBoundingClientRect();
      var area = r.width * r.height;
      if (area > bestArea) { bestArea = area; best = v; }
    }
    return best;
  }
  function bestVideo() { return pick(true); }
  function anyVideo() { return pick(false); }
  function report() {
    if (reportTimer) return;
    reportTimer = setTimeout(function () {
      reportTimer = null;
      var v = bestVideo();
      if (v) current = v;
      var playing = !!v;
      var present = playing || !!anyVideo();
      if (playing !== lastPlaying || present !== lastPresent) {
        lastPlaying = playing;
        lastPresent = present;
        post({ kind: 'media', playing: playing, present: present });
      }
    }, 120);
  }
  ['play', 'playing', 'pause', 'ended', 'volumechange', 'emptied', 'loadedmetadata'].forEach(function (t) {
    document.addEventListener(t, report, true);
  });

  // Manual pop-out affordance: a small button pinned to the right edge of
  // the video under the cursor.
  var hoverBtn = null;
  var hoverFor = null;
  var hoverRaf = 0;
  function hoverButton() {
    if (hoverBtn) return hoverBtn;
    var b = document.createElement('button');
    b.id = '__wis-popout-hover';
    b.type = 'button';
    b.title = 'Pop out video';
    b.style.cssText = 'position:fixed;display:none;width:34px;height:34px;padding:0;border:0;border-radius:9px;background:rgba(15,15,20,0.62);color:#fff;cursor:pointer;z-index:2147483646;transition:background 0.15s ease;';
    var NS = 'http://www.w3.org/2000/svg';
    var svg = document.createElementNS(NS, 'svg');
    svg.setAttribute('width', '18');
    svg.setAttribute('height', '18');
    svg.setAttribute('viewBox', '0 0 18 18');
    svg.setAttribute('fill', 'none');
    svg.setAttribute('stroke', 'currentColor');
    svg.setAttribute('stroke-width', '1.6');
    svg.style.cssText = 'display:block;margin:0 auto;';
    var outer = document.createElementNS(NS, 'rect');
    outer.setAttribute('x', '1.5');
    outer.setAttribute('y', '3');
    outer.setAttribute('width', '15');
    outer.setAttribute('height', '12');
    outer.setAttribute('rx', '2');
    var inner = document.createElementNS(NS, 'rect');
    inner.setAttribute('x', '8.5');
    inner.setAttribute('y', '9');
    inner.setAttribute('width', '6');
    inner.setAttribute('height', '4.5');
    inner.setAttribute('rx', '1');
    inner.setAttribute('fill', 'currentColor');
    inner.setAttribute('stroke', 'none');
    svg.appendChild(outer);
    svg.appendChild(inner);
    b.appendChild(svg);
    b.addEventListener('mouseenter', function () { b.style.background = 'rgba(15,15,20,0.85)'; });
    b.addEventListener('mouseleave', function () { b.style.background = 'rgba(15,15,20,0.62)'; });
    b.addEventListener('click', function (e) {
      e.preventDefault();
      e.stopPropagation();
      if (hoverFor) {
        manualTarget = hoverFor;
        manualAt = Date.now();
      }
      hideHover();
      post({ kind: 'request' });
    });
    document.documentElement.appendChild(b);
    hoverBtn = b;
    return b;
  }
  function hideHover() {
    if (hoverBtn) hoverBtn.style.display = 'none';
    hoverFor = null;
  }
  function placeHover(cx, cy) {
    if (active) { hideHover(); return; }
    var vids = document.querySelectorAll('video');
    var target = null;
    var targetRect = null;
    var bestArea = 0;
    for (var i = 0; i < vids.length; i++) {
      var v = vids[i];
      if (!sizeOk(v)) continue;
      var r = v.getBoundingClientRect();
      if (cx < r.left || cx > r.right || cy < r.top || cy > r.bottom) continue;
      var area = r.width * r.height;
      if (area > bestArea) { bestArea = area; target = v; targetRect = r; }
    }
    if (!target) { hideHover(); return; }
    var b = hoverButton();
    b.style.left = Math.round(targetRect.right - 44) + 'px';
    b.style.top = Math.round(targetRect.top + (targetRect.height - 34) / 2) + 'px';
    b.style.display = 'block';
    hoverFor = target;
  }
  document.addEventListener('mousemove', function (e) {
    if (hoverRaf) return;
    var cx = e.clientX;
    var cy = e.clientY;
    hoverRaf = requestAnimationFrame(function () {
      hoverRaf = 0;
      placeHover(cx, cy);
    });
  }, { capture: true, passive: true });
  document.addEventListener('scroll', hideHover, { capture: true, passive: true });

  var CSS =
    'html.__wis-popout-mode{background:#000!important;transform:none!important;filter:none!important;}' +
    'html.__wis-popout-mode body{visibility:hidden!important;background:#000!important;}' +
    'video.__wis-popout-video{position:fixed!important;inset:0 auto auto 0!important;width:100vw!important;height:100vh!important;max-width:none!important;max-height:none!important;min-width:0!important;min-height:0!important;margin:0!important;padding:0!important;border:0!important;border-radius:0!important;transform:none!important;visibility:visible!important;opacity:1!important;display:block!important;z-index:2147483644!important;object-fit:contain!important;background:#000!important;}' +
    'video.__wis-popout-video::-webkit-media-controls{display:none!important;}' +
    'video.__wis-popout-video::-webkit-media-controls-enclosure{display:none!important;}' +
    '#__wis-popout-overlay{position:fixed;inset:0;z-index:2147483645;cursor:default;-webkit-user-select:none;user-select:none;font-family:-apple-system,BlinkMacSystemFont,sans-serif;}' +
    '#__wis-popout-x{position:absolute;top:10px;right:10px;width:28px;height:28px;border:0;border-radius:50%;background:rgba(15,15,20,0.62);color:#fff;font-size:17px;line-height:28px;text-align:center;padding:0;cursor:pointer;opacity:0;transition:opacity 0.18s ease,background 0.18s ease;z-index:3;}' +
    '#__wis-popout-overlay:hover #__wis-popout-x,#__wis-popout-overlay.is-fresh #__wis-popout-x{opacity:1;}' +
    '#__wis-popout-x:hover{background:rgba(15,15,20,0.85);}' +
    '#__wis-popout-vol{position:absolute;left:10px;bottom:10px;display:flex;align-items:center;gap:6px;height:28px;padding:0 10px 0 3px;border-radius:14px;background:rgba(15,15,20,0.62);opacity:0;transition:opacity 0.18s ease;z-index:3;}' +
    '#__wis-popout-overlay:hover #__wis-popout-vol,#__wis-popout-overlay.is-fresh #__wis-popout-vol{opacity:1;}' +
    '#__wis-popout-vol button{width:24px;height:24px;border:0;border-radius:50%;background:transparent;color:#fff;cursor:pointer;padding:0;display:grid;place-items:center;}' +
    '#__wis-popout-vol button:hover{background:rgba(255,255,255,0.18);}' +
    '#__wis-popout-vol button svg{width:15px;height:15px;display:block;}' +
    '#__wis-popout-vol input{-webkit-appearance:none;appearance:none;width:72px;height:3px;border-radius:2px;background:rgba(255,255,255,0.35);outline:none;cursor:pointer;margin:0;}' +
    '#__wis-popout-vol input::-webkit-slider-thumb{-webkit-appearance:none;appearance:none;width:11px;height:11px;border-radius:50%;background:#fff;border:0;}' +
    '.__wis-popout-corner{position:absolute;width:18px;height:18px;z-index:2;}' +
    '.__wis-popout-corner[data-corner=nw]{top:0;left:0;cursor:nwse-resize;}' +
    '.__wis-popout-corner[data-corner=se]{bottom:0;right:0;cursor:nwse-resize;}' +
    '.__wis-popout-corner[data-corner=ne]{top:0;right:0;cursor:nesw-resize;}' +
    '.__wis-popout-corner[data-corner=sw]{bottom:0;left:0;cursor:nesw-resize;}' +
    '#__wis-popout-paused{position:absolute;top:50%;left:50%;width:64px;height:64px;margin:-32px 0 0 -32px;border-radius:50%;background:rgba(15,15,20,0.55);opacity:0;transition:opacity 0.18s ease;pointer-events:none;z-index:1;}' +
    '#__wis-popout-paused::after{content:\'\';position:absolute;top:50%;left:50%;margin:-13px 0 0 -8px;border-left:22px solid #fff;border-top:13px solid transparent;border-bottom:13px solid transparent;}' +
    '#__wis-popout-badge{position:absolute;top:50%;left:50%;transform:translate(-50%,-50%);padding:10px 16px;border-radius:18px;background:rgba(15,15,20,0.62);color:#fff;font-size:15px;font-weight:600;opacity:0;pointer-events:none;z-index:2;}' +
    '#__wis-popout-badge.is-on{animation:__wis-popout-fade 0.7s ease forwards;}' +
    '@keyframes __wis-popout-fade{0%{opacity:1;}70%{opacity:1;}100%{opacity:0;}}';

  var active = null;

  window.__wisPopoutEnter = function () {
    if (active) return;
    hideHover();
    var v = null;
    if (manualTarget && manualTarget.isConnected && manualTarget.videoWidth && Date.now() - manualAt < 6000) {
      v = manualTarget;
    }
    manualTarget = null;
    if (!v) v = bestVideo();
    if (!v && current && current.isConnected && current.videoWidth) v = current;
    if (!v) v = anyVideo();
    if (!v) return;
    userPaused = false;
    resumeTarget = !v.paused && !v.ended ? v : null;

    var style = document.createElement('style');
    style.id = '__wis-popout-style';
    style.textContent = CSS;
    var overlay = document.createElement('div');
    overlay.id = '__wis-popout-overlay';
    overlay.className = 'is-fresh';

    ['nw', 'ne', 'sw', 'se'].forEach(function (c) {
      var zone = document.createElement('div');
      zone.className = '__wis-popout-corner';
      zone.setAttribute('data-corner', c);
      overlay.appendChild(zone);
    });
    var x = document.createElement('button');
    x.id = '__wis-popout-x';
    x.type = 'button';
    x.textContent = '×';
    var paused = document.createElement('div');
    paused.id = '__wis-popout-paused';
    var badge = document.createElement('div');
    badge.id = '__wis-popout-badge';
    var vol = document.createElement('div');
    vol.id = '__wis-popout-vol';
    var volBtn = document.createElement('button');
    volBtn.type = 'button';
    var volOn = volIcon(false);
    var volOff = volIcon(true);
    volBtn.appendChild(volOn);
    volBtn.appendChild(volOff);
    var volSlider = document.createElement('input');
    volSlider.type = 'range';
    volSlider.min = '0';
    volSlider.max = '1';
    volSlider.step = '0.01';
    volSlider.setAttribute('aria-label', 'Volume');
    vol.appendChild(volBtn);
    vol.appendChild(volSlider);
    overlay.appendChild(x);
    overlay.appendChild(paused);
    overlay.appendChild(badge);
    overlay.appendChild(vol);

    // Reparent the video to <html> while popped.
    // The original slot is restored on exit.
    var restore = { parent: v.parentNode, next: v.nextSibling };
    v.classList.add('__wis-popout-video');
    document.documentElement.classList.add('__wis-popout-mode');
    document.documentElement.appendChild(style);
    document.documentElement.appendChild(v);
    document.documentElement.appendChild(overlay);

    active = {
      video: v, style: style, overlay: overlay, restore: restore,
      x: x, paused: paused, badge: badge,
      vol: vol, volBtn: volBtn, volOn: volOn, volOff: volOff,
      volSlider: volSlider, volDragging: false,
      watch: 0, freshTimer: 0, clickTimer: 0, lastClick: 0, down: null,
      aspectTimer: 0
    };

    syncPaused();
    syncVolume();
    v.addEventListener('play', syncPaused);
    v.addEventListener('pause', syncPaused);
    v.addEventListener('resize', onAspect);
    v.addEventListener('volumechange', syncVolume);
    volBtn.addEventListener('click', onVolButton);
    volSlider.addEventListener('input', onVolSlider);
    volSlider.addEventListener('pointerdown', function () { if (active) active.volDragging = true; });
    volSlider.addEventListener('pointerup', function () { if (active) active.volDragging = false; });
    volSlider.addEventListener('pointercancel', function () { if (active) active.volDragging = false; });
    overlay.addEventListener('pointerdown', onDown);
    overlay.addEventListener('pointermove', onMove);
    overlay.addEventListener('pointerup', onUp);
    overlay.addEventListener('pointercancel', onCancel);
    overlay.addEventListener('wheel', onWheel, { passive: false });
    overlay.addEventListener('contextmenu', prevent);
    x.addEventListener('click', onClose);
    window.addEventListener('pagehide', onPagehide);

    active.freshTimer = setTimeout(function () { overlay.classList.remove('is-fresh'); }, 1800);
    active.watch = setInterval(watch, 1500);

    post({ kind: 'entered', aspect: v.videoWidth / v.videoHeight });
  };

  window.__wisPopoutExit = function () {
    if (active) {
      var v = active.video;
      resumeTarget = !v.paused && !v.ended ? v : null;
    }
    teardown(null);
  };

  // Moving the webview between windows pauses media engine-side; the shell
  // calls this after each move to kick a video that was playing back into
  // playback. The pause can land a beat after the move, so it retries over
  // a short window; a user-intended pause (X, click) is never overridden.
  window.__wisPopoutResume = function () {
    var v = resumeTarget;
    resumeTarget = null;
    if (!v) return;
    var deadline = Date.now() + 1200;
    function kick() {
      if (!v.isConnected || v.ended || userPaused) return;
      if (v.paused) {
        var p = v.play();
        if (p && p.catch) p.catch(function () {});
      }
      if (Date.now() < deadline) setTimeout(kick, 200);
    }
    kick();
  };

  function teardown(notifyKind) {
    if (!active) return;
    var a = active;
    active = null;
    clearInterval(a.watch);
    clearTimeout(a.freshTimer);
    clearTimeout(a.clickTimer);
    clearTimeout(a.aspectTimer);
    a.video.removeEventListener('play', syncPaused);
    a.video.removeEventListener('pause', syncPaused);
    a.video.removeEventListener('resize', onAspect);
    a.video.removeEventListener('volumechange', syncVolume);
    window.removeEventListener('pagehide', onPagehide);
    try {
      var r = a.restore;
      if (r && r.parent && r.parent.isConnected && a.video.isConnected) {
        if (r.next && r.next.parentNode === r.parent) {
          r.parent.insertBefore(a.video, r.next);
        } else {
          r.parent.appendChild(a.video);
        }
      }
    } catch (e) {}
    try { a.video.classList.remove('__wis-popout-video'); } catch (e) {}
    try { document.documentElement.classList.remove('__wis-popout-mode'); } catch (e) {}
    [a.style, a.overlay].forEach(function (n) {
      try { n.remove(); } catch (e) {}
    });
    if (notifyKind) post({ kind: notifyKind });
  }

  function watch() {
    if (!active) return;
    if (!active.video.isConnected) { teardown('lost'); return; }
    if (!active.style.isConnected) document.documentElement.appendChild(active.style);
    if (!active.overlay.isConnected) document.documentElement.appendChild(active.overlay);
    var root = document.documentElement.classList;
    if (!root.contains('__wis-popout-mode')) root.add('__wis-popout-mode');
    if (!active.video.classList.contains('__wis-popout-video')) {
      active.video.classList.add('__wis-popout-video');
    }
  }

  function onPagehide() { teardown('lost'); }
  function prevent(e) { e.preventDefault(); }
  function onAspect() {
    if (!active) return;
    clearTimeout(active.aspectTimer);
    active.aspectTimer = setTimeout(function () {
      if (!active) return;
      var v = active.video;
      if (v.videoWidth && v.videoHeight) {
        post({ kind: 'aspect', aspect: v.videoWidth / v.videoHeight });
      }
    }, 150);
  }
  function syncPaused() {
    if (!active) return;
    active.paused.style.opacity = active.video.paused ? '1' : '0';
  }
  function volIcon(slashed) {
    var NS = 'http://www.w3.org/2000/svg';
    var svg = document.createElementNS(NS, 'svg');
    svg.setAttribute('viewBox', '0 0 24 24');
    svg.setAttribute('fill', 'none');
    svg.setAttribute('stroke', 'currentColor');
    svg.setAttribute('stroke-width', '2');
    svg.setAttribute('stroke-linecap', 'round');
    svg.setAttribute('stroke-linejoin', 'round');
    var paths = slashed
      ? ['M11 4.7 6.6 8.2H3v7.6h3.6L11 19.3z', 'm21 9-6 6', 'm15 9 6 6']
      : ['M11 4.7 6.6 8.2H3v7.6h3.6L11 19.3z', 'M15.5 8.5a5 5 0 0 1 0 7', 'M18.6 5.4a9 9 0 0 1 0 13.2'];
    for (var i = 0; i < paths.length; i++) {
      var p = document.createElementNS(NS, 'path');
      p.setAttribute('d', paths[i]);
      svg.appendChild(p);
    }
    return svg;
  }
  // Mirrors the element's live volume/mute (the page player drives the same
  // element, so its slider and this one stay in lockstep both ways).
  function syncVolume() {
    if (!active) return;
    var v = active.video;
    var silent = v.muted || v.volume === 0;
    active.volOn.style.display = silent ? 'none' : 'block';
    active.volOff.style.display = silent ? 'block' : 'none';
    active.volBtn.title = v.muted ? 'Unmute' : 'Mute';
    active.volBtn.setAttribute('aria-label', v.muted ? 'Unmute' : 'Mute');
    if (!active.volDragging) active.volSlider.value = String(v.volume);
  }
  function onVolButton(e) {
    e.preventDefault();
    e.stopPropagation();
    if (!active) return;
    active.video.muted = !active.video.muted;
  }
  function onVolSlider() {
    if (!active) return;
    var v = active.video;
    var value = parseFloat(active.volSlider.value);
    if (!isFinite(value)) return;
    v.volume = Math.min(1, Math.max(0, value));
    if (v.volume > 0 && v.muted) v.muted = false;
  }
  function onWheel(e) {
    e.preventDefault();
    if (!active || !active.vol.contains(e.target)) return;
    var v = active.video;
    var next = Math.min(1, Math.max(0, v.volume + (e.deltaY < 0 ? 0.05 : -0.05)));
    v.volume = next;
    if (next > 0 && v.muted) v.muted = false;
  }
  function onClose(e) {
    e.preventDefault();
    e.stopPropagation();
    if (!active) return;
    userPaused = true;
    resumeTarget = null;
    try { active.video.pause(); } catch (err) {}
    teardown('close');
  }
  function onDown(e) {
    if (!active || e.button !== 0) return;
    if (active.x.contains(e.target) || active.vol.contains(e.target)) return;
    var corner = e.target && e.target.getAttribute ? e.target.getAttribute('data-corner') : null;
    active.down = { sx: e.screenX, sy: e.screenY, corner: corner, moved: false, ax: 0, ay: 0, raf: 0 };
    try { active.overlay.setPointerCapture(e.pointerId); } catch (err) {}
    e.preventDefault();
  }
  function onMove(e) {
    if (!active || !active.down) return;
    var d = active.down;
    if (!d.moved) {
      if (Math.abs(e.screenX - d.sx) < 4 && Math.abs(e.screenY - d.sy) < 4) return;
      d.moved = true;
      if (!d.corner) {
        active.down = null;
        post({ kind: 'dragstart' });
        return;
      }
      d.sx = e.screenX;
      d.sy = e.screenY;
      return;
    }
    if (!d.corner) return;
    d.ax += e.screenX - d.sx;
    d.ay += e.screenY - d.sy;
    d.sx = e.screenX;
    d.sy = e.screenY;
    if (!d.raf) {
      d.raf = requestAnimationFrame(function () {
        if (!active || active.down !== d) return;
        d.raf = 0;
        if (d.ax || d.ay) {
          post({ kind: 'resize', corner: d.corner, dx: d.ax, dy: d.ay });
          d.ax = 0;
          d.ay = 0;
        }
      });
    }
  }
  function onUp(e) {
    if (!active || !active.down) return;
    var d = active.down;
    active.down = null;
    if (d.raf) cancelAnimationFrame(d.raf);
    if (d.moved) {
      if (d.corner && (d.ax || d.ay)) post({ kind: 'resize', corner: d.corner, dx: d.ax, dy: d.ay });
      return;
    }
    handleClick(e);
  }
  function onCancel() { if (active) active.down = null; }
  function handleClick(e) {
    if (!active) return;
    var now = Date.now();
    var third = active.overlay.clientWidth / 3;
    var cx = e.clientX;
    if (now - active.lastClick < 300) {
      active.lastClick = 0;
      clearTimeout(active.clickTimer);
      active.clickTimer = 0;
      if (cx < third) seek(-10);
      else if (cx > 2 * third) seek(10);
      else toggle();
      return;
    }
    active.lastClick = now;
    active.clickTimer = setTimeout(function () {
      if (!active) return;
      active.clickTimer = 0;
      toggle();
    }, 300);
  }
  function toggle() {
    if (!active) return;
    var v = active.video;
    if (v.paused) {
      userPaused = false;
      var p = v.play();
      if (p && p.catch) p.catch(function () {});
    } else {
      userPaused = true;
      v.pause();
    }
  }
  function seek(delta) {
    if (!active) return;
    var v = active.video;
    var t = v.currentTime + delta;
    if (t < 0) t = 0;
    if (isFinite(v.duration) && t > v.duration) t = v.duration;
    try { v.currentTime = t; } catch (e) { return; }
    flash(delta < 0 ? '-10s' : '+10s');
  }
  function flash(text) {
    if (!active) return;
    var b = active.badge;
    b.textContent = text;
    b.classList.remove('is-on');
    void b.offsetWidth;
    b.classList.add('is-on');
  }
})();"#;

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Frame = Frame {
        x: 0.0,
        y: 0.0,
        w: 1728.0,
        h: 1080.0,
    };
    const MAIN: Frame = Frame {
        x: 100.0,
        y: 60.0,
        w: 1280.0,
        h: 820.0,
    };

    #[test]
    fn clamp_aspect_rejects_garbage() {
        assert_eq!(clamp_aspect(f64::NAN), 16.0 / 9.0);
        assert_eq!(clamp_aspect(0.0), 16.0 / 9.0);
        assert_eq!(clamp_aspect(-2.0), 16.0 / 9.0);
        assert_eq!(clamp_aspect(100.0), 5.0);
        assert_eq!(clamp_aspect(0.01), 0.2);
        assert_eq!(clamp_aspect(1.5), 1.5);
    }

    #[test]
    fn initial_frame_docks_bottom_right_of_main() {
        let f = initial_frame(16.0 / 9.0, MAIN, WORK, None);
        assert_eq!(f.w, 440.0);
        assert!((f.h - 440.0 * 9.0 / 16.0).abs() < 0.001);
        assert!((f.x - (MAIN.x + MAIN.w - f.w - 24.0)).abs() < 0.001);
        assert!((f.y - (MAIN.y + MAIN.h - f.h - 24.0)).abs() < 0.001);
    }

    #[test]
    fn initial_frame_always_starts_at_the_default_width() {
        // A remembered position never changes the predefined starting size.
        let f = initial_frame(16.0 / 9.0, MAIN, WORK, Some((300.0, 200.0)));
        assert_eq!(f.w, 440.0);
        assert_eq!((f.x, f.y), (300.0, 200.0));
    }

    #[test]
    fn initial_frame_clamps_remembered_position_into_work_area() {
        let f = initial_frame(16.0 / 9.0, MAIN, WORK, Some((-500.0, 99999.0)));
        assert!(f.x >= WORK.x && f.x + f.w <= WORK.x + WORK.w);
        assert!(f.y >= WORK.y && f.y + f.h <= WORK.y + WORK.h);
        assert!((f.h - f.w / (16.0 / 9.0)).abs() < 0.001);
    }

    #[test]
    fn initial_frame_caps_width_by_work_height() {
        // A short work area cannot hold the default width at 16:9; the
        // width must shrink so the height fits.
        let short = Frame {
            x: 0.0,
            y: 0.0,
            w: 800.0,
            h: 200.0,
        };
        let f = initial_frame(16.0 / 9.0, MAIN, short, None);
        assert!(f.w < 440.0);
        assert!(f.h <= short.h);
        assert!((f.h - f.w / (16.0 / 9.0)).abs() < 0.001);
    }

    #[test]
    fn resize_se_keeps_origin_anchored() {
        let frame = Frame {
            x: 200.0,
            y: 150.0,
            w: 480.0,
            h: 270.0,
        };
        let next = resized_frame(frame, Corner::SouthEast, 160.0, 10.0, 16.0 / 9.0, WORK);
        assert_eq!(next.x, frame.x);
        assert_eq!(next.y, frame.y);
        assert_eq!(next.w, 640.0);
        assert!((next.h - 360.0).abs() < 0.001);
    }

    #[test]
    fn resize_nw_keeps_bottom_right_anchored() {
        let frame = Frame {
            x: 400.0,
            y: 300.0,
            w: 480.0,
            h: 270.0,
        };
        let next = resized_frame(frame, Corner::NorthWest, -160.0, -10.0, 16.0 / 9.0, WORK);
        assert!((next.x + next.w - (frame.x + frame.w)).abs() < 0.001);
        assert!((next.y + next.h - (frame.y + frame.h)).abs() < 0.001);
        assert_eq!(next.w, 640.0);
    }

    #[test]
    fn resize_clamps_to_min_width() {
        let frame = Frame {
            x: 200.0,
            y: 150.0,
            w: 300.0,
            h: 168.75,
        };
        let next = resized_frame(frame, Corner::SouthEast, -5000.0, 0.0, 16.0 / 9.0, WORK);
        assert_eq!(next.w, MIN_WIDTH);
        assert!((next.h - MIN_WIDTH / (16.0 / 9.0)).abs() < 0.001);
    }

    #[test]
    fn resize_follows_dominant_axis() {
        let frame = Frame {
            x: 200.0,
            y: 150.0,
            w: 480.0,
            h: 270.0,
        };
        // Vertical delta dominates: width grows by dy * aspect.
        let next = resized_frame(frame, Corner::SouthEast, 10.0, 90.0, 16.0 / 9.0, WORK);
        assert!((next.w - (480.0 + 90.0 * 16.0 / 9.0)).abs() < 0.001);
    }

    #[test]
    fn zero_delta_resize_relocks_height_to_new_aspect() {
        // The aspect-changed path: same width and anchor, height re-derived.
        let frame = Frame {
            x: 200.0,
            y: 150.0,
            w: 480.0,
            h: 270.0,
        };
        let next = resized_frame(frame, Corner::SouthEast, 0.0, 0.0, 4.0 / 3.0, WORK);
        assert_eq!(next.x, frame.x);
        assert_eq!(next.y, frame.y);
        assert_eq!(next.w, frame.w);
        assert!((next.h - 480.0 / (4.0 / 3.0)).abs() < 0.001);
    }

    #[test]
    fn corner_parse_rejects_unknown() {
        assert_eq!(Corner::parse("nw"), Some(Corner::NorthWest));
        assert_eq!(Corner::parse("se"), Some(Corner::SouthEast));
        assert_eq!(Corner::parse("xx"), None);
        assert_eq!(Corner::parse(""), None);
    }

    #[test]
    fn label_validation_requires_tab_prefix() {
        assert!(validate_label("tab-3f2a1c").is_ok());
        assert!(validate_label("tab-").is_err());
        assert!(validate_label("widget-panel-x").is_err());
        assert!(validate_label("tab-../etc").is_err());
    }
}
