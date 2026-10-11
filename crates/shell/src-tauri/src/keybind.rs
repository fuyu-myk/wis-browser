//! Page-side keyboard shortcuts.
//!
//! Dispatch priority mirrors the platforms' native behavior. A small
//! chrome-owned set (tab/window lifecycle, address bar; [`RESERVED_ACTIONS`])
//! is taken at capture phase before the page sees the keystroke, like every
//! major browser's reserved shortcuts. Everything else is **page-first**, as
//! WKWebView is on macOS (a page `preventDefault` suppresses the menu
//! equivalent): it dispatches from a bubble-phase window listener only when no
//! page handler claimed the event, so Ctrl+S in a web editor saves the
//! document instead of toggling the sidebar. A page that `stopPropagation`s
//! without `preventDefault` keeps such a keystroke too; accepted cost.
//!
//! On macOS the menu keeps the reserved set natively and offers every other
//! bound combo to a focused tab first (wry's page-first menu hook, matched by
//! menu item title; [`set_page_first_titles`]). WebKit's resend of a key the
//! page declined never reaches the menu there, so this script posts the
//! action itself, one macrotask after the dispatch, when no handler claimed
//! it.
//!
//! Trust model: a page could post fabricated action ids to this channel
//! without any keystroke. The dispatcher therefore only accepts an action
//! from the webview that is currently some workspace's **active tab**, and
//! stamps the emitted event with the source tab so the chrome additionally
//! drops actions whose source is not in the acting window's own workspace
//! (`useKeyActions`). Residual exposure: the page the user is actively
//! viewing can still trigger chrome actions on itself (close/navigate its own
//! tab), and any window's active tab can trigger the nuisance-tier find/popout
//! listeners, which do not check the source. On macOS the channel lives in
//! Wis's own script world, out of page scripts' reach.

use std::collections::HashMap;
use std::sync::OnceLock;

use tauri::{AppHandle, Emitter, Manager};
use wis_core::{KeyAction, TabId};

static APP: OnceLock<AppHandle> = OnceLock::new();

/// Menu item titles a focused webview is offered first.
#[cfg(target_os = "macos")]
struct PageFirstTitles {
    tab: Vec<String>,
    text_editing: Vec<String>,
}

#[cfg(target_os = "macos")]
static PAGE_FIRST_TITLES: std::sync::RwLock<PageFirstTitles> =
    std::sync::RwLock::new(PageFirstTitles {
        tab: Vec::new(),
        text_editing: Vec::new(),
    });

/// Record the app handle so the dispatcher arm can emit key actions.
pub fn set_app_handle(app: AppHandle) {
    let _ = APP.set(app);
}

/// Chrome-owned shortcuts a page can never intercept, mirroring the set
/// mainstream browsers reserve (tab/window lifecycle + address-bar focus).
/// Everything not listed dispatches page-first.
const RESERVED_ACTIONS: &[KeyAction] = &[
    KeyAction::NewTab,
    KeyAction::CloseTab,
    KeyAction::ClosePane,
    KeyAction::RestoreClosedTab,
    KeyAction::NewWindow,
    KeyAction::NewPrivateWindow,
    KeyAction::TogglePrivateWorkspace,
    KeyAction::NewWorkspace,
    KeyAction::NextTab,
    KeyAction::PreviousTab,
    KeyAction::SwitchToTab1,
    KeyAction::SwitchToTab2,
    KeyAction::SwitchToTab3,
    KeyAction::SwitchToTab4,
    KeyAction::SwitchToTab5,
    KeyAction::SwitchToTab6,
    KeyAction::SwitchToTab7,
    KeyAction::SwitchToTab8,
    KeyAction::SwitchToTab9,
    KeyAction::SwitchToLastTab,
    KeyAction::NextWorkspace,
    KeyAction::PreviousWorkspace,
    KeyAction::FocusUrlBar,
    KeyAction::FocusUrlBarAlt,
    KeyAction::OpenCommandPalette,
];

/// Page script: match bound accelerators on keydown and forward the action id.
///
/// Normalization (modifier order, Option/Alt unification, shifted-symbol fold)
/// mirrors `normalizeAccelerator` in `lib/keybindings.ts`, and the
/// event-to-accelerator mapping mirrors `keyboardEventToAccelerator`, so the
/// page and the chrome resolve identical keystrokes to identical actions.
/// Runs in every frame (wry injects all-frames on Windows), which is wanted:
/// keydown fires only in the focused document, wherever that is.
const KEYBIND_JS: &str = r#"(function () {
  if (window.__wisKeybindInstalled) return;
  window.__wisKeybindInstalled = true;
  var SHIFT_BASE = {
    '~': '`', '!': '1', '@': '2', '#': '3', '$': '4', '%': '5', '^': '6',
    '&': '7', '*': '8', '(': '9', ')': '0', '_': '-', '+': '=', '{': '[',
    '}': ']', '|': '\\', ':': ';', '"': "'", '<': ',', '>': '.', '?': '/'
  };
  var MODS = { Cmd: 1, Ctrl: 1, Option: 1, Alt: 1, Shift: 1, Meta: 1 };
  var RESERVED = __WIS_RESERVED__;
  var DEFER = __WIS_DEFER__;
  var TEXT_KEYS = {
    ArrowLeft: 1, ArrowRight: 1, ArrowUp: 1, ArrowDown: 1, Home: 1, End: 1,
    PageUp: 1, PageDown: 1, Backspace: 1, Delete: 1
  };
  var TEXT_INPUTS = { text: 1, search: 1, url: 1, tel: 1, email: 1, password: 1, number: 1 };
  function textEditing(e) {
    if (!TEXT_KEYS[e.key]) return false;
    return (e.metaKey ? 1 : 0) + (e.ctrlKey ? 1 : 0) + (e.altKey ? 1 : 0) <= 1;
  }
  function textField(el) {
    if (!el || typeof el.tagName !== 'string') return false;
    if (el.isContentEditable) return true;
    var tag = el.tagName.toUpperCase();
    if (tag === 'TEXTAREA') return true;
    return tag === 'INPUT' && !!TEXT_INPUTS[(el.type || 'text').toLowerCase()];
  }
  function normalize(accel) {
    var mods = {};
    var key = '';
    var parts = accel.split('+');
    if (parts.length > 1 && parts[parts.length - 1] === '' && parts[parts.length - 2] === '') {
      parts.splice(-2, 2, '+');
    }
    for (var i = 0; i < parts.length; i++) {
      var p = parts[i];
      if (MODS[p]) mods[p === 'Option' ? 'Alt' : (p === 'Meta' ? 'Cmd' : p)] = true;
      else key = p;
    }
    if (mods.Shift && SHIFT_BASE[key]) key = SHIFT_BASE[key];
    if (key.length === 1) key = key.toUpperCase();
    var out = [];
    if (mods.Ctrl) out.push('Ctrl');
    if (mods.Alt) out.push('Alt');
    if (mods.Shift) out.push('Shift');
    if (mods.Cmd) out.push('Cmd');
    out.push(key);
    return out.join('+');
  }
  function build(map) {
    var lookup = {};
    for (var action in map) {
      if (Object.prototype.hasOwnProperty.call(map, action) && map[action]) {
        lookup[normalize(map[action])] = action;
      }
    }
    return lookup;
  }
  var lookup = build(__WIS_KEYMAP__);
  window.__wisKeybindUpdate = function (map) {
    try { lookup = build(map); } catch (e) {}
  };
  var NAMED = {
    ArrowLeft: 'Left', ArrowRight: 'Right', ArrowUp: 'Up', ArrowDown: 'Down',
    ' ': 'Space'
  };
  function accelOf(e) {
    var k = e.key;
    if (k === 'Meta' || k === 'Control' || k === 'Alt' || k === 'Shift' ||
        k === 'CapsLock' || k === 'Fn' || k === 'Escape') return null;
    var name = NAMED[k] || (k.length === 1 ? k.toUpperCase() : k);
    var parts = [];
    if (e.metaKey) parts.push('Cmd');
    if (e.ctrlKey) parts.push('Ctrl');
    if (e.altKey) parts.push('Option');
    if (e.shiftKey) parts.push('Shift');
    if (!parts.length && !/^F([1-9]|1[0-9]|2[0-4])$/.test(name)) return null;
    parts.push(name);
    return parts.join('+');
  }
  function actionOf(e) {
    if (!e.isTrusted) return null;
    var accel = accelOf(e);
    if (!accel) return null;
    return lookup[normalize(accel)] || null;
  }
  function post(action) {
    try {
      if (window.webkit && window.webkit.messageHandlers &&
          window.webkit.messageHandlers.wisKeybind) {
        window.webkit.messageHandlers.wisKeybind.postMessage(action);
      }
    } catch (err) {}
  }
  document.addEventListener('keydown', function (e) {
    var action = actionOf(e);
    if (!action || !RESERVED[action]) return;
    e.preventDefault();
    e.stopPropagation();
    post(action);
  }, true);
  window.addEventListener('keydown', function (e) {
    if (e.defaultPrevented) return;
    var action = actionOf(e);
    if (!action || RESERVED[action]) return;
    if (textEditing(e) && textField(e.composedPath ? e.composedPath()[0] : e.target)) return;
    if (DEFER) {
      setTimeout(function () {
        if (!e.defaultPrevented) post(action);
      }, 0);
      return;
    }
    e.preventDefault();
    post(action);
  }, false);
})();"#;

/// `{ action_id: accelerator }` as a JS-safe JSON literal.
fn keymap_json(keymap: &HashMap<KeyAction, String>) -> String {
    let map: HashMap<&str, &str> = keymap
        .iter()
        .map(|(action, accel)| (action.id(), accel.as_str()))
        .collect();
    serde_json::to_string(&map)
        .unwrap_or_else(|_| "{}".into())
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

fn reserved_json() -> String {
    let map: HashMap<&str, bool> = RESERVED_ACTIONS.iter().map(|a| (a.id(), true)).collect();
    serde_json::to_string(&map).unwrap_or_else(|_| "{}".into())
}

/// The forwarder init script with the effective keymap baked in. `defer`
/// waits for a macrotask before posting a page-first action, for a page
/// that claims it from a later handler.
fn script(keymap: &HashMap<KeyAction, String>, defer: bool) -> String {
    KEYBIND_JS
        .replace("__WIS_KEYMAP__", &keymap_json(keymap))
        .replace("__WIS_RESERVED__", &reserved_json())
        .replace("__WIS_DEFER__", if defer { "true" } else { "false" })
}

/// The forwarder init script for Windows and Linux tabs.
#[cfg(any(windows, target_os = "linux"))]
pub fn page_script(keymap: &HashMap<KeyAction, String>) -> String {
    script(keymap, false)
}

/// Whether accelerator `accel` moves the caret or deletes within a text field
/// (Cmd+Left, Option+Backspace). Two of Cmd, Ctrl and Option together edit
/// nothing. Mirrors `isTextEditingKeystroke` in `lib/keybindings.ts`.
#[cfg(any(target_os = "macos", test))]
fn is_text_editing_accelerator(accel: &str) -> bool {
    const KEYS: &[&str] = &[
        "Left",
        "Right",
        "Up",
        "Down",
        "Home",
        "End",
        "PageUp",
        "PageDown",
        "Backspace",
        "Delete",
    ];
    let mut parts: Vec<&str> = accel.split('+').collect();
    let Some(key) = parts.pop() else {
        return false;
    };
    let editing_mods = parts
        .iter()
        .filter(|m| matches!(**m, "Cmd" | "Meta" | "Ctrl" | "Alt" | "Option"))
        .count();
    KEYS.contains(&key) && editing_mods <= 1
}

/// Record which menu items a focused webview is offered first, from each
/// action item's title; called on every menu build.
#[cfg(target_os = "macos")]
pub fn set_menu_titles(titles: Vec<(KeyAction, String)>, keymap: &HashMap<KeyAction, String>) {
    let mut first = PageFirstTitles {
        tab: Vec::new(),
        text_editing: Vec::new(),
    };
    for (action, title) in titles {
        if keymap
            .get(&action)
            .is_some_and(|accel| is_text_editing_accelerator(accel))
        {
            first.text_editing.push(title.clone());
        }
        if !RESERVED_ACTIONS.contains(&action) {
            first.tab.push(title);
        }
    }
    if let Ok(mut current) = PAGE_FIRST_TITLES.write() {
        *current = first;
    }
}

/// Let a focused webview see its page-first combos before the menu does.
/// Undo, redo and select all go to any web page first (`edit_keys` replays
/// them), but stay with the menu in Wis's own webviews, whose inputs need the
/// native command. A caret-moving shortcut reaches every webview first, so a
/// text field keeps it; `useKeybindCapture` runs it in Wis's own.
#[cfg(target_os = "macos")]
pub fn offer_page_first_combos_to_tabs() {
    wry::set_page_first_menu_hook(Box::new(|label, title, editing| {
        if editing {
            return crate::webview_prefs::is_remote(label);
        }
        PAGE_FIRST_TITLES.read().is_ok_and(|titles| {
            let first = if label.starts_with("tab-") {
                &titles.tab
            } else {
                &titles.text_editing
            };
            first.iter().any(|t| t == title)
        })
    }));
}

/// Install the page-first fallback on a macOS tab, in every frame. Windows
/// and Linux carry [`page_script`] on their own channels instead.
pub fn install<R: tauri::Runtime>(
    webview: &tauri::webview::Webview<R>,
    keymap: &HashMap<KeyAction, String>,
) {
    #[cfg(target_os = "macos")]
    imp::install(webview, script(keymap, true));

    #[cfg(not(target_os = "macos"))]
    let _ = (webview, keymap);
}

/// Dispatcher arm for `wisKeybind`: validate the posted action id, require the
/// posting webview to be a workspace's active tab, then emit the standard `wis-keyaction`
/// event stamped with the source tab.
pub(crate) fn dispatch_action(label: &str, body: &serde_json::Value) {
    let Some(app) = APP.get() else {
        return;
    };
    let Some(id) = body.as_str() else {
        return;
    };
    if KeyAction::from_id(id).is_none() {
        return;
    }
    let Some(tab_id) = label
        .strip_prefix("tab-")
        .and_then(|s| uuid::Uuid::parse_str(s).ok())
        .map(TabId::from_uuid)
    else {
        return;
    };

    let app = app.clone();
    let id = id.to_string();
    tauri::async_runtime::spawn(async move {
        let is_active_tab = {
            let state = app.state::<crate::state::AppState>();
            let browser = state.browser.lock().await;
            browser
                .workspaces
                .iter()
                .any(|w| w.active_tab_id == Some(tab_id))
        };
        if !is_active_tab {
            return;
        }

        let payload = serde_json::json!({
            "action": id,
            "tab": tab_id.as_uuid().to_string(),
        });
        if let Err(err) = app.emit(crate::menu::KEYACTION_EVENT, payload) {
            log::warn!("emit wis-keyaction for {id} failed: {err}");
        }
    });
}

/// Push a changed keymap into every live tab webview so rebinds apply without
/// a reload (the baked-in init script only covers webviews created after).
/// Top-level documents only.
pub fn apply_to_all_tabs(app: &AppHandle, keymap: &HashMap<KeyAction, String>) {
    let json = keymap_json(keymap);
    for (label, webview) in app.webviews() {
        if label.starts_with("tab-") {
            #[cfg(windows)]
            let key = format!(", '{}'", crate::win_dispatch::token(&label));
            #[cfg(not(windows))]
            let key = String::new();
            let _ = crate::content_world::eval(
                &webview,
                format!("window.__wisKeybindUpdate && window.__wisKeybindUpdate({json}{key});"),
            );
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::CStr;
    use std::os::raw::c_char;

    use objc2::rc::Id;
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{
        class, declare_class, msg_send, msg_send_id, mutability, ClassType, DeclaredClass,
    };

    use crate::content_world::macos as content_world;

    struct HandlerIvars {
        label: String,
    }

    declare_class!(
        struct KeybindHandler;

        unsafe impl ClassType for KeybindHandler {
            type Super = NSObject;
            type Mutability = mutability::InteriorMutable;
            const NAME: &'static str = "WisKeybindHandler";
        }

        impl DeclaredClass for KeybindHandler {
            type Ivars = HandlerIvars;
        }

        unsafe impl KeybindHandler {
            #[method(userContentController:didReceiveScriptMessage:)]
            fn did_receive(&self, _ucc: *mut AnyObject, message: *mut AnyObject) {
                let label = self.ivars().label.clone();
                unsafe { handle_message(&label, message) }
            }
        }
    );

    impl KeybindHandler {
        fn new(label: String) -> Id<Self> {
            let this = Self::alloc().set_ivars(HandlerIvars { label });
            unsafe { msg_send_id![super(this), init] }
        }
    }

    unsafe fn nsstring_to_string(ns: *mut AnyObject) -> Option<String> {
        if ns.is_null() {
            return None;
        }

        let is_str: bool = msg_send![ns, isKindOfClass: class!(NSString)];
        if !is_str {
            return None;
        }

        let utf8: *const c_char = msg_send![ns, UTF8String];
        if utf8.is_null() {
            return None;
        }

        Some(CStr::from_ptr(utf8).to_string_lossy().into_owned())
    }

    unsafe fn handle_message(label: &str, message: *mut AnyObject) {
        if message.is_null() {
            return;
        }

        let body: *mut AnyObject = msg_send![message, body];
        let Some(action) = nsstring_to_string(body) else {
            return;
        };

        super::dispatch_action(label, &serde_json::Value::String(action));
    }

    pub(super) fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>, script: String) {
        let label = webview.label().to_string();
        let _ = webview.with_webview(move |w| unsafe {
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

            let handler = KeybindHandler::new(label.clone());
            content_world::add_handler(controller, &*handler, "wisKeybind");
            content_world::install_script(wk, controller, &label, "wisKeybind", &script, false);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caret_moving_accelerators_are_recognised() {
        for accel in [
            "Cmd+Left",
            "Cmd+Right",
            "Cmd+Shift+Up",
            "Alt+Backspace",
            "Ctrl+Left",
            "Home",
        ] {
            assert!(is_text_editing_accelerator(accel), "{accel}");
        }
        for accel in ["Alt+Cmd+Left", "Cmd+[", "Cmd+L", "Ctrl+Alt+Delete", "F5"] {
            assert!(!is_text_editing_accelerator(accel), "{accel}");
        }
    }

    #[test]
    fn both_variants_fill_every_placeholder() {
        let keymap = wis_core::default_keymap();
        for defer in [false, true] {
            assert!(!script(&keymap, defer).contains("__WIS_"));
        }
        assert!(script(&keymap, true).contains("var DEFER = true;"));
        assert!(script(&keymap, false).contains("var DEFER = false;"));
    }
}
