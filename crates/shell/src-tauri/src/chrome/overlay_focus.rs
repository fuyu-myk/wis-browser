//! Keyboard focus goes back where it came from when an overlay closes.
//!
//! An overlay surface takes focus with `focus_self`; hiding it only hides the
//! webview, which leaves AppKit's first responder on the bare window. So the
//! webview holding focus before is remembered as the overlay takes it, and
//! handed focus again on hide while the overlay still has it. Both run on the
//! main thread; the remembered view is held weakly.

/// The first of `start` and its ancestors (via `parent`) that `matches`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn first_up(
    start: usize,
    parent: impl Fn(usize) -> Option<usize>,
    matches: impl Fn(usize) -> bool,
) -> Option<usize> {
    let mut node = Some(start);
    while let Some(at) = node {
        if matches(at) {
            return Some(at);
        }
        node = parent(at);
    }
    None
}

#[cfg(target_os = "macos")]
mod mac {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use objc2::rc::Weak;
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use objc2::{msg_send, sel};

    thread_local! {
        /// Overlay view -> the webview it took focus from.
        static RETURN_TO: RefCell<HashMap<usize, Weak<AnyObject>>> = RefCell::new(HashMap::new());
    }

    // SAFETY for every fn here: main thread only (`with_webview`), each view
    // pointer a live NSView.

    unsafe fn superview(view: usize) -> Option<usize> {
        let obj = view as *mut AnyObject;
        let responds: Bool = msg_send![obj, respondsToSelector: sel!(superview)];
        if !responds.as_bool() {
            return None;
        }
        let parent: *mut AnyObject = msg_send![obj, superview];
        (!parent.is_null()).then_some(parent as usize)
    }

    unsafe fn first_responder(view: *mut AnyObject) -> Option<(*mut AnyObject, usize)> {
        let window: *mut AnyObject = msg_send![view, window];
        if window.is_null() {
            return None;
        }
        let responder: *mut AnyObject = msg_send![window, firstResponder];
        (!responder.is_null()).then_some((window, responder as usize))
    }

    /// # Safety
    /// Main thread, with `view` a live NSView.
    pub unsafe fn is_hidden(view: *mut AnyObject) -> bool {
        let hidden: Bool = msg_send![view, isHiddenOrHasHiddenAncestor];
        hidden.as_bool()
    }

    /// Remember the webview holding focus as `overlay` takes it.
    ///
    /// # Safety
    /// Main thread, with `overlay` the live overlay WKWebView.
    pub unsafe fn remember(overlay: *mut AnyObject) {
        let Some(class) = AnyClass::get("WKWebView") else {
            return;
        };
        let Some((_, responder)) = first_responder(overlay) else {
            return;
        };
        let is_webview = |view: usize| -> bool {
            let kind: Bool = msg_send![view as *mut AnyObject, isKindOfClass: class];
            kind.as_bool()
        };
        let Some(holder) = super::first_up(responder, |v| superview(v), is_webview) else {
            return;
        };
        if holder == overlay as usize {
            return;
        }

        let Some(held) = objc2::rc::Retained::retain(holder as *mut AnyObject) else {
            return;
        };
        let weak = Weak::from_retained(&held);
        RETURN_TO.with(|map| map.borrow_mut().insert(overlay as usize, weak));
    }

    /// Give focus back to the remembered webview if `overlay` still holds it.
    /// Call before hiding `overlay`, which would hand focus to the window.
    ///
    /// # Safety
    /// Main thread, with `overlay` the live overlay WKWebView.
    pub unsafe fn give_back(overlay: *mut AnyObject) {
        let Some(weak) = RETURN_TO.with(|map| map.borrow_mut().remove(&(overlay as usize))) else {
            return;
        };
        let (Some(target), Some((window, responder))) = (weak.load(), first_responder(overlay))
        else {
            return;
        };
        let holds =
            super::first_up(responder, |v| superview(v), |v| v == overlay as usize).is_some();
        let target = objc2::rc::Retained::as_ptr(&target) as *mut AnyObject;
        let target_window: *mut AnyObject = msg_send![target, window];
        if holds && target_window == window && !is_hidden(target) {
            let _: Bool = msg_send![window, makeFirstResponder: target];
        }
    }

    #[cfg(test)]
    mod tests {
        use objc2::runtime::{AnyClass, AnyObject, Bool, Sel};
        use objc2::{class, sel};

        type Obj = *mut AnyObject;

        #[test]
        fn every_send_matches_its_method() {
            let view = class!(NSView);
            view.verify_sel::<(), Obj>(sel!(superview)).unwrap();
            view.verify_sel::<(), Obj>(sel!(window)).unwrap();
            view.verify_sel::<(), Bool>(sel!(isHiddenOrHasHiddenAncestor))
                .unwrap();
            view.verify_sel::<(Sel,), Bool>(sel!(respondsToSelector:))
                .unwrap();
            view.verify_sel::<(&AnyClass,), Bool>(sel!(isKindOfClass:))
                .unwrap();
            let window = class!(NSWindow);
            window.verify_sel::<(), Obj>(sel!(firstResponder)).unwrap();
            window
                .verify_sel::<(Obj,), Bool>(sel!(makeFirstResponder:))
                .unwrap();
        }
    }
}

#[cfg(target_os = "macos")]
pub use mac::{give_back, is_hidden, remember};

#[cfg(test)]
mod tests {
    use super::first_up;

    /// 1 <- 2 <- 3 <- 4: each node's parent is one less, down to 1.
    fn parent(node: usize) -> Option<usize> {
        (node > 1).then(|| node - 1)
    }

    #[test]
    fn the_nearest_matching_ancestor_is_found() {
        assert_eq!(first_up(4, parent, |n| n <= 2), Some(2));
    }

    #[test]
    fn the_start_itself_can_match() {
        assert_eq!(first_up(3, parent, |n| n == 3), Some(3));
    }

    #[test]
    fn no_match_up_to_the_root_is_none() {
        assert_eq!(first_up(4, parent, |n| n == 9), None);
    }
}
