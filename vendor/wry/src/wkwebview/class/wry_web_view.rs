// Copyright 2020-2024 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use std::{collections::HashMap, sync::Mutex};

#[cfg(target_os = "macos")]
use objc2::runtime::ProtocolObject;
use objc2::{define_class, rc::Retained, runtime::Bool, DeclaredClass};
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSDraggingDestination, NSEvent};
use objc2_foundation::{NSObjectProtocol, NSUUID};

#[cfg(target_os = "ios")]
use crate::wkwebview::ios::WKWebView::WKWebView;
#[cfg(target_os = "macos")]
use crate::{
  wkwebview::{drag_drop, synthetic_mouse_events},
  DragDropEvent,
};
#[cfg(target_os = "ios")]
use objc2_ui_kit::UIEvent as NSEvent;
#[cfg(target_os = "macos")]
use objc2_web_kit::WKWebView;

pub struct WryWebViewIvars {
  pub(crate) is_child: bool,
  #[cfg(target_os = "macos")]
  pub(crate) id: String,
  #[cfg(target_os = "macos")]
  pub(crate) drag_drop_handler: Box<dyn Fn(DragDropEvent) -> bool>,
  #[cfg(target_os = "macos")]
  pub(crate) accept_first_mouse: objc2::runtime::Bool,
  #[cfg(target_os = "ios")]
  pub(crate) input_accessory_view_builder: Option<Box<crate::InputAccessoryViewBuilder>>,
  pub(crate) custom_protocol_task_ids: Mutex<HashMap<usize, Retained<NSUUID>>>,
}

define_class!(
  #[unsafe(super(WKWebView))]
  #[ivars = WryWebViewIvars]
  pub struct WryWebView;

  /// Overridden NSView methods.
  impl WryWebView {
    #[unsafe(method(performKeyEquivalent:))]
    fn perform_key_equivalent(&self, event: &NSEvent) -> Bool {
      // Workaround for https://github.com/tauri-apps/tauri/issues/9426, refined
      // (Wis patch, see WIS-PATCH.md): upstream returns NO for every child
      // webview so a child never claims Cmd-key events away from the menu, but
      // that also means the page never sees them in JavaScript (Google Docs
      // Cmd+Z/A). Refinement: the child holding keyboard focus forwards to its
      // page first, Safari-style, EXCEPT combos the main menu binds to
      // non-editing (chrome) actions: WebKit's page-declined resend does not
      // reach the menu in this embedding, so offering those to the page would
      // kill Cmd+W/T/L outright whenever a page holds focus. Unfocused
      // children keep returning NO so they never race the focused one.
      #[cfg(target_os = "macos")]
      let claim = if !self.ivars().is_child {
        true
      } else {
        self.holds_first_responder() && !menu_reserves_event(event, &self.ivars().id)
      };
      #[cfg(target_os = "ios")]
      let claim = !self.ivars().is_child;
      if claim {
        unsafe { objc2::msg_send![super(self), performKeyEquivalent: event] }
      } else {
        Bool::NO
      }
    }

    #[cfg(target_os = "macos")]
    #[unsafe(method(acceptsFirstMouse:))]
    fn accept_first_mouse(&self, _event: &NSEvent) -> Bool {
      self.ivars().accept_first_mouse
    }

    #[cfg(target_os = "ios")]
    #[unsafe(method_id(inputAccessoryView))]
    fn input_accessory_view(&self) -> Option<Retained<objc2_ui_kit::UIView>> {
      if let Some(builder) = &self.ivars().input_accessory_view_builder {
        builder(self)
      } else {
        unsafe { objc2::msg_send![super(self), inputAccessoryView] }
      }
    }
  }
  unsafe impl NSObjectProtocol for WryWebView {}

  // Drag & Drop
  #[cfg(target_os = "macos")]
  unsafe impl NSDraggingDestination for WryWebView {
    #[unsafe(method(draggingEntered:))]
    fn dragging_entered(
      &self,
      drag_info: &ProtocolObject<dyn objc2_app_kit::NSDraggingInfo>,
    ) -> objc2_app_kit::NSDragOperation {
      drag_drop::dragging_entered(self, drag_info)
    }

    #[unsafe(method(draggingUpdated:))]
    fn dragging_updated(
      &self,
      drag_info: &ProtocolObject<dyn objc2_app_kit::NSDraggingInfo>,
    ) -> objc2_app_kit::NSDragOperation {
      drag_drop::dragging_updated(self, drag_info)
    }

    #[unsafe(method(performDragOperation:))]
    fn perform_drag_operation(
      &self,
      drag_info: &ProtocolObject<dyn objc2_app_kit::NSDraggingInfo>,
    ) -> Bool {
      drag_drop::perform_drag_operation(self, drag_info)
    }

    #[unsafe(method(draggingExited:))]
    fn dragging_exited(&self, drag_info: &ProtocolObject<dyn objc2_app_kit::NSDraggingInfo>) {
      drag_drop::dragging_exited(self, drag_info)
    }
  }

  // Synthetic mouse events
  #[cfg(target_os = "macos")]
  impl WryWebView {
    #[unsafe(method(otherMouseDown:))]
    fn other_mouse_down(&self, event: &NSEvent) {
      synthetic_mouse_events::other_mouse_down(self, event)
    }

    #[unsafe(method(otherMouseUp:))]
    fn other_mouse_up(&self, event: &NSEvent) {
      synthetic_mouse_events::other_mouse_up(self, event)
    }
  }
);

#[cfg(target_os = "macos")]
impl WryWebView {
  /// Whether this view is (or contains) its window's first responder.
  fn holds_first_responder(&self) -> bool {
    use objc2::runtime::AnyObject;
    use objc2::ClassType;

    // SAFETY: all pointers are +0 references kept alive by the view/window
    // hierarchy for the duration of this synchronous main-thread call; both
    // nullable returns are checked before deref, and `isDescendantOf:` is
    // only sent after `isKindOfClass:` confirms an NSView receiver.
    unsafe {
      let window: *mut AnyObject = objc2::msg_send![self, window];
      if window.is_null() {
        return false;
      }
      let responder: *mut AnyObject = objc2::msg_send![&*window, firstResponder];
      if responder.is_null() {
        return false;
      }
      let is_view: bool =
        objc2::msg_send![&*responder, isKindOfClass: objc2_app_kit::NSView::class()];
      if !is_view {
        return false;
      }
      objc2::msg_send![&*responder, isDescendantOf: self]
    }
  }
}

/// Editing actions that stay page-first even though menu items bind them.
/// Clipboard actions (cut:/copy:/paste:) are not here. Pages that leave
/// these page-first combos unconsumed get them replayed natively by the
/// embedder (Wis: the wisEditKey channel).
#[cfg(target_os = "macos")]
const EDITING_ACTIONS: [&str; 3] = ["undo:", "redo:", "selectAll:"];

/// Whether the main menu binds this key event to a non-editing (chrome)
/// action that the page-first hook does not offer to webview `id`. Reserved
/// combos are never offered to the focused page. Walking
/// the live menu keeps the set correct under user-rebindable shortcuts.
/// Known edge: Cmd+Shift+digit combos are not matched
/// (charactersIgnoringModifiers applies Shift to digits).
#[cfg(target_os = "macos")]
fn menu_reserves_event(event: &NSEvent, id: &str) -> bool {
  use objc2_app_kit::{NSApplication, NSEventModifierFlags};
  use objc2_foundation::MainThreadMarker;

  let Some(mtm) = MainThreadMarker::new() else {
    return false;
  };

  let app = NSApplication::sharedApplication(mtm);
  let Some(menu) = app.mainMenu() else {
    return false;
  };

  let key = unsafe { event.charactersIgnoringModifiers() }
    .map(|s| s.to_string().to_lowercase())
    .unwrap_or_default();
  if key.is_empty() {
    return false;
  }

  let relevant = NSEventModifierFlags::Command
    | NSEventModifierFlags::Shift
    | NSEventModifierFlags::Option
    | NSEventModifierFlags::Control;
  let event_mask = unsafe { event.modifierFlags() } & relevant;

  // SAFETY: the menu is read on the main thread within this synchronous
  // call; every msg_send receiver is checked non-nil first.
  unsafe { menu_claims(&menu, &key, event_mask, relevant, id) }
}

#[cfg(target_os = "macos")]
unsafe fn menu_claims(
  menu: &objc2_app_kit::NSMenu,
  key: &str,
  event_mask: objc2_app_kit::NSEventModifierFlags,
  relevant: objc2_app_kit::NSEventModifierFlags,
  id: &str,
) -> bool {
  use objc2::runtime::AnyObject;
  use objc2_app_kit::NSEventModifierFlags;

  let count: isize = objc2::msg_send![menu, numberOfItems];
  for i in 0..count {
    let item: *mut AnyObject = objc2::msg_send![menu, itemAtIndex: i];
    if item.is_null() {
      continue;
    }

    let submenu: *mut objc2_app_kit::NSMenu = objc2::msg_send![&*item, submenu];
    if !submenu.is_null() {
      if menu_claims(&*submenu, key, event_mask, relevant, id) {
        return true;
      }

      continue;
    }
    let ke_ptr: *mut objc2_foundation::NSString = objc2::msg_send![&*item, keyEquivalent];
    if ke_ptr.is_null() {
      continue;
    }

    let ke = (*ke_ptr).to_string();
    if ke.is_empty() {
      continue;
    }

    let mut mask: NSEventModifierFlags = objc2::msg_send![&*item, keyEquivalentModifierMask];
    mask &= relevant;
    // An uppercase key equivalent implies Shift.
    if ke != ke.to_lowercase() {
      mask |= NSEventModifierFlags::Shift;
    }
    if ke.to_lowercase() != key || mask != event_mask {
      continue;
    }

    let action: Option<objc2::runtime::Sel> = objc2::msg_send![&*item, action];
    let editing = action
      .and_then(|sel| {
        sel
          .name()
          .to_str()
          .ok()
          .map(|n| EDITING_ACTIONS.contains(&n))
      })
      .unwrap_or(false);
    if editing {
      return false;
    }

    let title: *mut objc2_foundation::NSString = objc2::msg_send![&*item, title];
    let title = if title.is_null() {
      String::new()
    } else {
      (*title).to_string()
    };
    return !crate::wkwebview::offers_menu_item_to_page(id, &title);
  }

  false
}

// Custom Protocol Task Checker
impl WryWebView {
  pub(crate) fn add_custom_task_key(&self, task_id: usize) -> Retained<NSUUID> {
    let task_uuid = NSUUID::new();
    self
      .ivars()
      .custom_protocol_task_ids
      .lock()
      .unwrap()
      .insert(task_id, task_uuid.clone());
    task_uuid
  }
  pub(crate) fn remove_custom_task_key(&self, task_id: usize) {
    self
      .ivars()
      .custom_protocol_task_ids
      .lock()
      .unwrap()
      .remove(&task_id);
  }
  pub(crate) fn get_custom_task_uuid(&self, task_id: usize) -> Option<Retained<NSUUID>> {
    self
      .ivars()
      .custom_protocol_task_ids
      .lock()
      .unwrap()
      .get(&task_id)
      .cloned()
  }
}
