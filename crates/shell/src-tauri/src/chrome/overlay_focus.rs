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

/// Which webview of each window took focus last, and where each overlay took
/// focus from, by label.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Default)]
struct Ledger {
    focused: std::collections::HashMap<String, String>,
    return_to: std::collections::HashMap<String, String>,
}

#[cfg_attr(not(windows), allow(dead_code))]
impl Ledger {
    fn got_focus(&mut self, window: &str, webview: &str) {
        self.focused.insert(window.to_owned(), webview.to_owned());
    }

    /// Note where `overlay` takes focus from, unless it already has it.
    fn remember(&mut self, window: &str, overlay: &str) {
        if let Some(from) = self.focused.get(window).filter(|from| *from != overlay) {
            self.return_to.insert(overlay.to_owned(), from.clone());
        }
    }

    /// The webview to focus as `overlay` hides, if `overlay` still has focus.
    /// A hidden overlay stops counting as focused as focus moves to the window.
    fn give_back(&mut self, window: &str, overlay: &str) -> Option<String> {
        let held = self.focused.get(window).is_some_and(|from| from == overlay);
        if held {
            self.focused.remove(window);
        }
        let target = self.return_to.remove(overlay)?;
        held.then_some(target)
    }
}

#[cfg(windows)]
mod win {
    use std::sync::{Mutex, MutexGuard, OnceLock};

    use tauri::Manager;
    use webview2_com::FocusChangedEventHandler;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2Controller, COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC,
    };

    use super::Ledger;
    use crate::platform::win_webview2::with_controller;

    fn ledger() -> MutexGuard<'static, Ledger> {
        static LEDGER: OnceLock<Mutex<Ledger>> = OnceLock::new();
        LEDGER
            .get_or_init(Mutex::default)
            .lock()
            .expect("focus ledger poisoned")
    }

    /// Note each time `webview` takes focus. Once per webview, as it is made.
    pub fn watch<R: tauri::Runtime>(webview: &tauri::Webview<R>) {
        let window = webview.window().label().to_owned();
        let label = webview.label().to_owned();
        with_controller(webview, move |controller| {
            let handler = FocusChangedEventHandler::create(Box::new(
                move |_sender: Option<ICoreWebView2Controller>,
                      _args: Option<windows::core::IUnknown>|
                      -> windows::core::Result<()> {
                    ledger().got_focus(&window, &label);
                    Ok(())
                },
            ));
            let mut token = 0_i64;
            // SAFETY: runs on the UI thread via `with_controller`.
            if let Err(err) = unsafe { controller.add_GotFocus(&handler, &mut token) } {
                log::warn!("focus watch: {err}");
            }
        });
    }

    fn is_shown(controller: &ICoreWebView2Controller) -> bool {
        let mut visible = windows::core::BOOL::default();
        // SAFETY: called on the UI thread from a `with_controller` closure.
        unsafe { controller.IsVisible(&mut visible) }.is_ok() && visible.as_bool()
    }

    fn move_focus(controller: &ICoreWebView2Controller) {
        // SAFETY: called on the UI thread from a `with_controller` closure.
        if let Err(err) =
            unsafe { controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC) }
        {
            log::warn!("focus move: {err}");
        }
    }

    /// Focus `webview`. An overlay first notes where focus comes from, and a
    /// hidden one is left alone.
    pub fn focus<R: tauri::Runtime>(webview: &tauri::Webview<R>, is_overlay: bool) {
        let window = webview.window().label().to_owned();
        let label = webview.label().to_owned();
        with_controller(webview, move |controller| {
            if is_overlay {
                if !is_shown(controller) {
                    return;
                }
                ledger().remember(&window, &label);
            }
            move_focus(controller);
        });
    }

    /// Queue focus back to the remembered webview, ahead of the overlay's hide
    /// and after any focus request already queued for it.
    pub fn give_back<R: tauri::Runtime>(overlay: &tauri::Webview<R>) {
        let window = overlay.window().label().to_owned();
        let label = overlay.label().to_owned();
        let app = overlay.app_handle().clone();
        with_controller(overlay, move |_| {
            let Some(target) = ledger().give_back(&window, &label) else {
                return;
            };
            let Some(target) = app.webviews().get(&target).cloned() else {
                return;
            };
            with_controller(&target, |controller| {
                if is_shown(controller) {
                    move_focus(controller);
                }
            });
        });
    }
}

#[cfg(windows)]
pub use win::{focus as focus_win, give_back as give_back_win};

/// Registers the focus watch on every webview as it is made (Windows).
#[cfg(windows)]
pub fn plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("wis-focus-return")
        .on_webview_ready(|webview| win::watch(&webview))
        .build()
}

#[cfg(target_os = "linux")]
mod gtk_focus {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use gtk::glib::object::{Cast, ObjectExt, WeakRef};
    use gtk::glib::StaticType;
    use gtk::prelude::{GtkWindowExt, WidgetExt};

    thread_local! {
        /// Overlay label -> the webview it took focus from.
        static RETURN_TO: RefCell<HashMap<String, WeakRef<gtk::Widget>>> =
            RefCell::new(HashMap::new());
    }

    fn focus_of(view: &gtk::Widget) -> Option<(gtk::Widget, gtk::Widget)> {
        let toplevel = view.toplevel()?;
        let focus = toplevel.downcast_ref::<gtk::Window>()?.focused_widget()?;
        Some((toplevel, focus))
    }

    /// Remember the webview holding focus as `overlay` takes it. Main thread.
    pub fn remember(label: &str, overlay: &webkit2gtk::WebView) {
        let overlay = overlay.upcast_ref::<gtk::Widget>();
        let Some((_, focus)) = focus_of(overlay) else {
            return;
        };
        let Some(holder) = focus.ancestor(webkit2gtk::WebView::static_type()) else {
            return;
        };
        if holder != *overlay {
            RETURN_TO.with(|map| {
                map.borrow_mut()
                    .insert(label.to_owned(), holder.downgrade())
            });
        }
    }

    /// Give focus back if `overlay` still holds it. Main thread, before hiding.
    pub fn give_back(label: &str, overlay: &webkit2gtk::WebView) {
        let Some(weak) = RETURN_TO.with(|map| map.borrow_mut().remove(label)) else {
            return;
        };
        let overlay = overlay.upcast_ref::<gtk::Widget>();
        let (Some(target), Some((toplevel, focus))) = (weak.upgrade(), focus_of(overlay)) else {
            return;
        };
        let holds = focus == *overlay || focus.is_ancestor(overlay);
        if holds && target.toplevel().as_ref() == Some(&toplevel) && target.is_mapped() {
            target.grab_focus();
        }
    }
}

#[cfg(target_os = "linux")]
pub use gtk_focus::{give_back as give_back_gtk, remember as remember_gtk};

#[cfg(test)]
mod ledger_tests {
    use super::Ledger;

    fn opened(ledger: &mut Ledger) {
        ledger.got_focus("main", "tab-a");
        ledger.remember("main", "overlay-main-menu");
        ledger.got_focus("main", "overlay-main-menu");
    }

    #[test]
    fn a_closing_overlay_hands_focus_back_to_where_it_came_from() {
        let mut ledger = Ledger::default();
        opened(&mut ledger);
        assert_eq!(
            ledger.give_back("main", "overlay-main-menu").as_deref(),
            Some("tab-a")
        );
        assert_eq!(ledger.give_back("main", "overlay-main-menu"), None);
    }

    #[test]
    fn focus_moved_elsewhere_is_left_alone() {
        let mut ledger = Ledger::default();
        opened(&mut ledger);
        ledger.got_focus("main", "tab-b");
        assert_eq!(ledger.give_back("main", "overlay-main-menu"), None);
    }

    #[test]
    fn refocusing_an_open_overlay_keeps_the_first_origin() {
        let mut ledger = Ledger::default();
        opened(&mut ledger);
        ledger.remember("main", "overlay-main-menu");
        assert_eq!(
            ledger.give_back("main", "overlay-main-menu").as_deref(),
            Some("tab-a")
        );
    }

    #[test]
    fn a_hidden_overlay_no_longer_counts_as_focused() {
        let mut ledger = Ledger::default();
        ledger.got_focus("main", "overlay-main-menu");
        assert_eq!(ledger.give_back("main", "overlay-main-menu"), None);
        ledger.remember("main", "overlay-main-find");
        ledger.got_focus("main", "overlay-main-find");
        assert_eq!(ledger.give_back("main", "overlay-main-find"), None);
    }

    #[test]
    fn windows_are_tracked_apart() {
        let mut ledger = Ledger::default();
        ledger.got_focus("other", "tab-z");
        ledger.remember("main", "overlay-main-menu");
        ledger.got_focus("main", "overlay-main-menu");
        assert_eq!(ledger.give_back("main", "overlay-main-menu"), None);
    }
}

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
