//! Read-only address bar for engine-created popups (macOS).
//!
//! `window.open` / `target=_blank` popups are built inside wry's UI delegate
//! as a bare `NSWindow` whose `contentView` is a `WKWebView` sharing the
//! opener's configuration (so `window.opener` and the workspace session
//! survive, which OAuth flows rely on). The shell gets no handle to that
//! window and must not patch the vendored wry, so the address bar is bolted
//! on from the app side: a lazily started main-thread poll finds any window
//! that is not Tauri-owned and whose content is a `WKWebView`, gives it a
//! titlebar accessory showing the current URL (read-only, with an https lock
//! glyph), and keeps it in sync as the popup navigates. The poll stops once
//! every popup has closed, so it costs nothing when none are open.

use tauri::AppHandle;

/// Called from `on_new_window` with the popup's target URL. Records that URL
/// so the bar can show the destination immediately (the popup's `WKWebView`
/// reports `about:blank` until it navigates), then decorates the new window on
/// the next main-loop tick (when wry has created it) and keeps a refresh poll
/// alive for later redirects.
pub fn ensure_running(app: &AppHandle, url: &str) {
    #[cfg(target_os = "macos")]
    {
        let app2 = app.clone();
        let url = url.to_string();
        let _ = app.run_on_main_thread(move || imp::kick(app2, url));
    }

    #[cfg(not(target_os = "macos"))]
    let _ = (app, url);
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::CStr;
    use std::os::raw::c_char;
    use std::sync::{Mutex, OnceLock};

    use objc2::runtime::{AnyObject, Bool};
    use objc2::{class, msg_send, sel};
    use tauri::{AppHandle, Manager};

    const POLL_INTERVAL_SECS: f64 = 0.35;
    // Stop polling a short grace period after the last popup closes (covers a
    // popup that opens and closes between two ticks).
    const STOP_AFTER_MISSES: u32 = 3;

    const BAR_H: f64 = 30.0;
    const FIELD_H: f64 = 17.0;
    const INSET: f64 = 12.0;
    const ICON: f64 = 13.0;
    const ICON_GAP: f64 = 6.0;
    const FONT_SIZE: f64 = 12.0;

    // NSView autoresizing mask bits (a bit set = that edge/size is flexible).
    const WIDTH_SIZABLE: usize = 1 << 1; // NSViewWidthSizable
    const MAX_X_MARGIN: usize = 1 << 2; // NSViewMaxXMargin (anchors to the left)

    const LINEBREAK_TRUNCATE_TAIL: usize = 4; // NSLineBreakByTruncatingTail
    const BOX_SEPARATOR: usize = 2; // NSBoxSeparator
    const LAYOUT_BOTTOM: isize = 4; // NSLayoutAttributeBottom

    // Distinct view tags so the bar's pieces can be found again each tick.
    const URL_FIELD_TAG: isize = 0x5749_5355; // "WISU"
    const LOCK_TAG: isize = 0x5749_5356;

    struct TimerState {
        timer: usize,
        misses: u32,
        /// The most recent popup's target URL, shown until its webview reports
        /// a real one. Consumed by the first window that gets decorated.
        pending: Option<String>,
    }

    fn state() -> &'static Mutex<TimerState> {
        static STATE: OnceLock<Mutex<TimerState>> = OnceLock::new();
        STATE.get_or_init(|| {
            Mutex::new(TimerState {
                timer: 0,
                misses: 0,
                pending: None,
            })
        })
    }

    /// A http(s) URL worth showing (the popup's webview is `about:blank` until
    /// it navigates, and `window.open('')` targets never carry one).
    fn is_meaningful(url: &str) -> bool {
        !url.is_empty() && url != "about:blank"
    }

    fn set_pending(url: String) {
        state().lock().unwrap().pending = is_meaningful(&url).then_some(url);
    }

    fn take_pending() -> Option<String> {
        state().lock().unwrap().pending.take()
    }

    pub fn kick(app: AppHandle, url: String) {
        set_pending(url);
        start_if_idle(&app);
        // Decorate the just-created window now instead of waiting up to one
        // poll interval, so the bar appears with the window (no content jump).
        tick(&app);
    }

    fn start_if_idle(app: &AppHandle) {
        let mut guard = state().lock().unwrap();
        if guard.timer != 0 {
            guard.misses = 0;
            return;
        }

        let app = app.clone();
        unsafe {
            use block2::RcBlock;
            let block = RcBlock::new(move |_timer: *mut AnyObject| tick(&app));
            let timer: *mut AnyObject = msg_send![
                class!(NSTimer),
                scheduledTimerWithTimeInterval: POLL_INTERVAL_SECS,
                repeats: Bool::YES,
                block: &*block,
            ];
            // Retain past the autorelease pool; released when the poll stops.
            let timer: *mut AnyObject = msg_send![timer, retain];
            guard.timer = timer as usize;
            guard.misses = 0;
        }
    }

    fn tick(app: &AppHandle) {
        let tauri_windows: Vec<usize> = app
            .windows()
            .values()
            .filter_map(|w| w.ns_window().ok().map(|p| p as usize))
            .collect();

        let live = unsafe { decorate_pass(&tauri_windows) };

        let mut guard = state().lock().unwrap();
        if live > 0 {
            guard.misses = 0;
            return;
        }

        guard.misses += 1;
        if guard.misses >= STOP_AFTER_MISSES && guard.timer != 0 {
            unsafe {
                let timer = guard.timer as *mut AnyObject;
                let _: () = msg_send![timer, invalidate];
                let _: () = msg_send![timer, release];
            }
            guard.timer = 0;
            guard.misses = 0;
        }
    }

    /// Decorate every engine popup and refresh its URL. Returns the live popup
    /// count so the caller can decide when to stop polling.
    unsafe fn decorate_pass(tauri_windows: &[usize]) -> u32 {
        let nsapp: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        if nsapp.is_null() {
            return 0;
        }

        let windows: *mut AnyObject = msg_send![nsapp, windows];
        if windows.is_null() {
            return 0;
        }

        let count: usize = msg_send![windows, count];
        let wk_class = class!(WKWebView);

        let mut live = 0u32;
        for i in 0..count {
            let win: *mut AnyObject = msg_send![windows, objectAtIndex: i];
            if win.is_null() || tauri_windows.contains(&(win as usize)) {
                continue;
            }
            let content: *mut AnyObject = msg_send![win, contentView];
            if content.is_null() {
                continue;
            }
            let is_wk: Bool = msg_send![content, isKindOfClass: wk_class];
            if !is_wk.as_bool() {
                continue;
            }

            live += 1;
            if view_with_tag(win, URL_FIELD_TAG).is_null() {
                build_bar(win, content);
            }
            refresh_url(win, content);
        }

        live
    }

    unsafe fn build_bar(win: *mut AnyObject, content: *mut AnyObject) {
        // Seed with the popup's known destination so the bar is never blank;
        // fall back to whatever the webview already has, else a loading hint.
        let initial = take_pending()
            .or_else(|| meaningful_url(content))
            .unwrap_or_else(|| "Loading...".to_string());

        let frame: NSRect = msg_send![win, frame];
        let width = frame.size.width.max(1.0);

        let container: *mut AnyObject = msg_send![class!(NSView), alloc];
        let container: *mut AnyObject =
            msg_send![container, initWithFrame: NSRect::new(0.0, 0.0, width, BAR_H)];
        let _: () = msg_send![container, setAutoresizingMask: WIDTH_SIZABLE];

        let mut text_x = INSET;
        if let Some(lock) = make_lock_view(symbol_name(&initial)) {
            let _: () = msg_send![container, addSubview: lock];
            let _: () = msg_send![lock, release];
            text_x = INSET + ICON + ICON_GAP;
        }

        let field = make_url_field(text_x, width - text_x - INSET);
        set_field_text(field, &initial);
        let _: () = msg_send![container, addSubview: field];
        let _: () = msg_send![field, release];

        let sep = make_separator(width);
        let _: () = msg_send![container, addSubview: sep];
        let _: () = msg_send![sep, release];

        let vc: *mut AnyObject = msg_send![class!(NSTitlebarAccessoryViewController), alloc];
        let vc: *mut AnyObject = msg_send![vc, init];
        let _: () = msg_send![vc, setView: container];
        let _: () = msg_send![vc, setLayoutAttribute: LAYOUT_BOTTOM];
        let _: () = msg_send![win, addTitlebarAccessoryViewController: vc];
        let _: () = msg_send![vc, release];
        let _: () = msg_send![container, release];
    }

    unsafe fn make_url_field(x: f64, w: f64) -> *mut AnyObject {
        let rect = NSRect::new(x, (BAR_H - FIELD_H) / 2.0, w.max(1.0), FIELD_H);
        let field: *mut AnyObject = msg_send![class!(NSTextField), alloc];
        let field: *mut AnyObject = msg_send![field, initWithFrame: rect];

        let _: () = msg_send![field, setEditable: Bool::NO];
        let _: () = msg_send![field, setSelectable: Bool::YES];
        let _: () = msg_send![field, setBordered: Bool::NO];
        let _: () = msg_send![field, setBezeled: Bool::NO];
        let _: () = msg_send![field, setDrawsBackground: Bool::NO];
        let font: *mut AnyObject = msg_send![class!(NSFont), systemFontOfSize: FONT_SIZE];
        let _: () = msg_send![field, setFont: font];
        let color: *mut AnyObject = msg_send![class!(NSColor), secondaryLabelColor];
        let _: () = msg_send![field, setTextColor: color];
        let _: () = msg_send![field, setTag: URL_FIELD_TAG];
        let _: () = msg_send![field, setAutoresizingMask: WIDTH_SIZABLE];
        let cell: *mut AnyObject = msg_send![field, cell];

        if !cell.is_null() {
            let _: () = msg_send![cell, setLineBreakMode: LINEBREAK_TRUNCATE_TAIL];
            let _: () = msg_send![cell, setScrollable: Bool::NO];
        }

        field
    }

    unsafe fn make_lock_view(symbol: &str) -> Option<*mut AnyObject> {
        let img = lock_image(symbol)?;
        let rect = NSRect::new(INSET, (BAR_H - ICON) / 2.0, ICON, ICON);
        let iv: *mut AnyObject = msg_send![class!(NSImageView), alloc];
        let iv: *mut AnyObject = msg_send![iv, initWithFrame: rect];

        let _: () = msg_send![iv, setImage: img];
        let _: () = msg_send![iv, setTag: LOCK_TAG];
        let _: () = msg_send![iv, setAutoresizingMask: MAX_X_MARGIN];
        tint_secondary(iv);

        Some(iv)
    }

    unsafe fn make_separator(width: f64) -> *mut AnyObject {
        let sep: *mut AnyObject = msg_send![class!(NSBox), alloc];
        let sep: *mut AnyObject = msg_send![sep, initWithFrame: NSRect::new(0.0, 0.0, width, 1.0)];

        let _: () = msg_send![sep, setBoxType: BOX_SEPARATOR];
        let _: () = msg_send![sep, setAutoresizingMask: WIDTH_SIZABLE];

        sep
    }

    unsafe fn refresh_url(win: *mut AnyObject, content: *mut AnyObject) {
        // Only act once the webview reports a real URL; before that the seeded
        // destination stays put instead of flickering to a loading hint.
        let Some(next) = meaningful_url(content) else {
            return;
        };

        let field = view_with_tag(win, URL_FIELD_TAG);
        if field.is_null() {
            return;
        }

        let current: *mut AnyObject = msg_send![field, stringValue];
        // Only write on change so a user's text selection is not cleared every tick.
        if nsstring_to_string(current) == next {
            return;
        }

        set_field_text(field, &next);
        let lock = view_with_tag(win, LOCK_TAG);
        if !lock.is_null() {
            if let Some(img) = lock_image(symbol_name(&next)) {
                let _: () = msg_send![lock, setImage: img];
            }
        }
    }

    unsafe fn set_field_text(field: *mut AnyObject, text: &str) {
        if let Some(ns) = nsstring(text) {
            let _: () = msg_send![field, setStringValue: ns];
        }
    }

    /// The webview's current URL, or `None` while it is blank/`about:blank`.
    unsafe fn meaningful_url(content: *mut AnyObject) -> Option<String> {
        let url: *mut AnyObject = msg_send![content, URL];
        if url.is_null() {
            return None;
        }

        let s: *mut AnyObject = msg_send![url, absoluteString];
        let text = nsstring_to_string(s);
        is_meaningful(&text).then_some(text)
    }

    /// SF Symbol for the URL's scheme: a lock for https, a warning for plain
    /// http, a neutral globe while the destination is unknown.
    fn symbol_name(url: &str) -> &'static str {
        if url.starts_with("https") {
            "lock.fill"
        } else if url.starts_with("http") {
            "exclamationmark.triangle.fill"
        } else {
            "globe"
        }
    }

    unsafe fn lock_image(symbol: &str) -> Option<*mut AnyObject> {
        let ns = nsstring(symbol)?;
        let img: *mut AnyObject = msg_send![
            class!(NSImage),
            imageWithSystemSymbolName: ns,
            accessibilityDescription: std::ptr::null_mut::<AnyObject>(),
        ];

        (!img.is_null()).then_some(img)
    }

    unsafe fn tint_secondary(view: *mut AnyObject) {
        let responds: Bool = msg_send![view, respondsToSelector: sel!(setContentTintColor:)];
        if responds.as_bool() {
            let color: *mut AnyObject = msg_send![class!(NSColor), secondaryLabelColor];
            let _: () = msg_send![view, setContentTintColor: color];
        }
    }

    /// Recursive `viewWithTag:` over the window's titlebar accessory views.
    unsafe fn view_with_tag(win: *mut AnyObject, tag: isize) -> *mut AnyObject {
        let vcs: *mut AnyObject = msg_send![win, titlebarAccessoryViewControllers];
        if vcs.is_null() {
            return std::ptr::null_mut();
        }

        let n: usize = msg_send![vcs, count];
        for i in 0..n {
            let vc: *mut AnyObject = msg_send![vcs, objectAtIndex: i];
            let view: *mut AnyObject = msg_send![vc, view];
            if view.is_null() {
                continue;
            }

            let found: *mut AnyObject = msg_send![view, viewWithTag: tag];
            if !found.is_null() {
                return found;
            }
        }

        std::ptr::null_mut()
    }

    unsafe fn nsstring(s: &str) -> Option<*mut AnyObject> {
        let c = std::ffi::CString::new(s).ok()?;
        let ns: *mut AnyObject = msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()];

        (!ns.is_null()).then_some(ns)
    }

    unsafe fn nsstring_to_string(ns: *mut AnyObject) -> String {
        if ns.is_null() {
            return String::new();
        }

        let utf8: *const c_char = msg_send![ns, UTF8String];
        if utf8.is_null() {
            return String::new();
        }

        CStr::from_ptr(utf8).to_string_lossy().into_owned()
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct NSPoint {
        x: f64,
        y: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct NSSize {
        width: f64,
        height: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct NSRect {
        origin: NSPoint,
        size: NSSize,
    }

    impl NSRect {
        fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
            NSRect {
                origin: NSPoint { x, y },
                size: NSSize { width, height },
            }
        }
    }

    unsafe impl objc2::Encode for NSPoint {
        const ENCODING: objc2::Encoding = objc2::Encoding::Struct(
            "CGPoint",
            &[
                <f64 as objc2::Encode>::ENCODING,
                <f64 as objc2::Encode>::ENCODING,
            ],
        );
    }

    unsafe impl objc2::Encode for NSSize {
        const ENCODING: objc2::Encoding = objc2::Encoding::Struct(
            "CGSize",
            &[
                <f64 as objc2::Encode>::ENCODING,
                <f64 as objc2::Encode>::ENCODING,
            ],
        );
    }

    unsafe impl objc2::Encode for NSRect {
        const ENCODING: objc2::Encoding = objc2::Encoding::Struct(
            "CGRect",
            &[
                <NSPoint as objc2::Encode>::ENCODING,
                <NSSize as objc2::Encode>::ENCODING,
            ],
        );
    }
}
