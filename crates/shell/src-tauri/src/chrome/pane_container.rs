//! Per-pane container view for tab webviews on macOS.
//!
//! wry adds every child webview straight to the window's content view, so a
//! tab webview's superview is the whole window. Originally, WebKit reads that
//! superview in two places and assumes it is the page's own area: the attached
//! inspector lays the inspected view out across `superview.bounds`, and element
//! fullscreen keys its placeholder on the view's origin within its superview.
//!
//! Giving each tab its own container makes both assumptions true: the webview
//! fills the container at the origin, and the container carries the pane rect.

#[cfg(target_os = "macos")]
mod imp {
    use objc2::runtime::{AnyObject, Bool};
    use objc2::{class, msg_send};

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct NSPoint {
        pub x: f64,
        pub y: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct NSSize {
        pub width: f64,
        pub height: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct NSRect {
        pub origin: NSPoint,
        pub size: NSSize,
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

    fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
        NSRect {
            origin: NSPoint { x, y },
            size: NSSize { width, height },
        }
    }

    /// The radius last asked for.
    static RADIUS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    const WIDTH_SIZABLE: usize = 2;
    const HEIGHT_SIZABLE: usize = 16;

    /// The container `wk` lives in, or null when it has not been wrapped.
    unsafe fn container_of(wk: *mut AnyObject) -> *mut AnyObject {
        let superview: *mut AnyObject = msg_send![wk, superview];
        if superview.is_null() {
            return std::ptr::null_mut();
        }

        let Some(class) = container_class() else {
            return std::ptr::null_mut();
        };
        let is_wrapper: Bool = msg_send![superview, isKindOfClass: class];

        if is_wrapper.as_bool() {
            superview
        } else {
            std::ptr::null_mut()
        }
    }

    /// A interaction on a container with no content falls through to whatever
    /// is underneath. A hidden tab's container still covers its pane rect, and
    /// a bare `NSView` still answers a hit test, so it would otherwise swallow
    /// presses meant for the rendered chrome page.
    unsafe extern "C" fn hit_test(
        this: &AnyObject,
        _cmd: objc2::runtime::Sel,
        point: NSPoint,
    ) -> *mut AnyObject {
        let webview = webview_in(this as *const AnyObject as *mut AnyObject);
        if webview.is_null() {
            return std::ptr::null_mut();
        }

        let hidden: Bool = msg_send![webview, isHidden];
        if hidden.as_bool() {
            return std::ptr::null_mut();
        }

        msg_send![super(this, class!(NSView)), hitTest: point]
    }

    /// An `NSView` subclass, so a container can be told apart from the content
    /// view it sits in as well as decline interactions.
    fn container_class() -> Option<&'static objc2::runtime::AnyClass> {
        use std::sync::Once;
        static REGISTER: Once = Once::new();

        REGISTER.call_once(|| {
            if let Some(mut builder) =
                objc2::declare::ClassBuilder::new("WisPaneContainer", class!(NSView))
            {
                unsafe {
                    builder.add_method(
                        objc2::sel!(hitTest:),
                        hit_test as unsafe extern "C" fn(_, _, _) -> _,
                    );
                }
                builder.register();
            }
        });

        objc2::runtime::AnyClass::get("WisPaneContainer")
    }

    /// True when `wk` is sitting directly in its window's content view, which
    /// is where it lands both on creation and on being reparented back from
    /// the video popout. Element fullscreen puts it in a clip view of
    /// WebKit's own window instead.
    unsafe fn wants_wrapping(wk: *mut AnyObject) -> bool {
        let superview: *mut AnyObject = msg_send![wk, superview];
        if superview.is_null() {
            return false;
        }

        let window: *mut AnyObject = msg_send![wk, window];
        if window.is_null() {
            return false;
        }

        let content: *mut AnyObject = msg_send![window, contentView];

        !content.is_null() && content == superview
    }

    /// Drop containers the webview has been reparented out of.
    unsafe fn prune_empty(superview: *mut AnyObject, class: &objc2::runtime::AnyClass) {
        let subviews: *mut AnyObject = msg_send![superview, subviews];
        if subviews.is_null() {
            return;
        }

        let count: usize = msg_send![subviews, count];
        for index in (0..count).rev() {
            let view: *mut AnyObject = msg_send![subviews, objectAtIndex: index];
            if view.is_null() {
                continue;
            }

            let is_wrapper: Bool = msg_send![view, isKindOfClass: class];
            if !is_wrapper.as_bool() {
                continue;
            }

            let children: *mut AnyObject = msg_send![view, subviews];
            let children_count: usize = if children.is_null() {
                0
            } else {
                msg_send![children, count]
            };
            if children_count == 0 {
                let _: () = msg_send![view, removeFromSuperview];
            }
        }
    }

    /// The tab webview inside `view`, when `view` is a container.
    unsafe fn webview_in(view: *mut AnyObject) -> *mut AnyObject {
        let Some(class) = container_class() else {
            return std::ptr::null_mut();
        };
        let is_wrapper: Bool = msg_send![view, isKindOfClass: class];
        if !is_wrapper.as_bool() {
            return std::ptr::null_mut();
        }

        let subviews: *mut AnyObject = msg_send![view, subviews];
        if subviews.is_null() {
            return std::ptr::null_mut();
        }

        let count: usize = msg_send![subviews, count];
        for index in 0..count {
            let child: *mut AnyObject = msg_send![subviews, objectAtIndex: index];
            if child.is_null() {
                continue;
            }

            let is_wk: Bool = msg_send![child, isKindOfClass: class!(WKWebView)];
            if is_wk.as_bool() {
                return child;
            }
        }

        std::ptr::null_mut()
    }

    pub unsafe fn wrap(wk: *mut AnyObject) {
        if wk.is_null() || !container_of(wk).is_null() || !wants_wrapping(wk) {
            return;
        }

        let superview: *mut AnyObject = msg_send![wk, superview];
        if superview.is_null() {
            return;
        }

        let Some(class) = container_class() else {
            return;
        };
        prune_empty(superview, class);

        let frame: NSRect = msg_send![wk, frame];
        let container: *mut AnyObject = msg_send![class, alloc];
        let container: *mut AnyObject = msg_send![container, initWithFrame: frame];
        if container.is_null() {
            return;
        }

        const BELOW: isize = -1;
        let _: () = msg_send![
            superview,
            addSubview: container,
            positioned: BELOW,
            relativeTo: wk,
        ];
        let _: () = msg_send![container, addSubview: wk];
        let _: () = msg_send![wk, setFrame: rect(0.0, 0.0, frame.size.width, frame.size.height)];
        let _: () = msg_send![wk, setAutoresizingMask: WIDTH_SIZABLE | HEIGHT_SIZABLE];
        let _: () = msg_send![container, release];

        let radius = f64::from_bits(RADIUS.load(std::sync::atomic::Ordering::Relaxed));
        if radius > 0.0 {
            set_corner_radius(wk, radius);
        }
    }

    /// Put the pane rect on the container, and keep the webview filling it.
    ///
    /// Silent when the webview is not in its container. Element fullscreen
    /// reparents it into WebKit's own window, and a rect pushed then would
    /// move the fullscreen content into a pane-sized box.
    pub unsafe fn set_bounds(wk: *mut AnyObject, x: f64, y: f64, width: f64, height: f64) {
        wrap(wk);

        let container = container_of(wk);
        if container.is_null() {
            return;
        }
        let parent: *mut AnyObject = msg_send![container, superview];
        if parent.is_null() {
            return;
        }

        let flipped: Bool = msg_send![parent, isFlipped];
        let top = if flipped.as_bool() {
            y
        } else {
            let parent_frame: NSRect = msg_send![parent, frame];
            parent_frame.size.height - y - height
        };

        let _: () = msg_send![container, setFrame: rect(x, top, width, height)];

        let hidden: Bool = msg_send![wk, isHidden];
        if !hidden.as_bool() {
            let _: () = msg_send![wk, setFrame: rect(0.0, 0.0, width, height)];
        }
    }

    /// Round the container, so the corner survives WebKit resizing the webview
    /// inside it while the inspector is docked.
    ///
    /// Rounds the webview as well as its container.
    pub unsafe fn set_corner_radius(wk: *mut AnyObject, radius: f64) {
        RADIUS.store(radius.to_bits(), std::sync::atomic::Ordering::Relaxed);

        let container = container_of(wk);
        for target in [wk, container] {
            if target.is_null() {
                continue;
            }

            let _: () = msg_send![target, setWantsLayer: true];
            let layer: *mut AnyObject = msg_send![target, layer];
            if layer.is_null() {
                continue;
            }

            let _: () = msg_send![layer, setCornerRadius: radius];
            let _: () = msg_send![layer, setMasksToBounds: true];
        }
    }
}

/// Put `webview` inside its own pane-sized container.
pub fn wrap<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
    #[cfg(target_os = "macos")]
    {
        let _ = webview.with_webview(|w| unsafe {
            imp::wrap(w.inner() as *mut objc2::runtime::AnyObject);
        });
    }

    #[cfg(not(target_os = "macos"))]
    let _ = webview;
}

/// Move `webview`'s container to a pane rect. False on platforms without
/// one, so the caller positions the webview through Tauri instead.
pub fn set_bounds<R: tauri::Runtime>(
    webview: &tauri::webview::Webview<R>,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> bool {
    #[cfg(target_os = "macos")]
    {
        webview
            .with_webview(move |w| unsafe {
                imp::set_bounds(
                    w.inner() as *mut objc2::runtime::AnyObject,
                    x,
                    y,
                    width,
                    height,
                );
            })
            .is_ok()
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (webview, x, y, width, height);
        false
    }
}

/// Round `webview`'s container. False on platforms without one.
pub fn set_corner_radius<R: tauri::Runtime>(
    webview: &tauri::webview::Webview<R>,
    radius: f64,
) -> bool {
    #[cfg(target_os = "macos")]
    {
        webview
            .with_webview(move |w| unsafe {
                imp::set_corner_radius(w.inner() as *mut objc2::runtime::AnyObject, radius);
            })
            .is_ok()
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (webview, radius);
        false
    }
}
