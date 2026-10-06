//! Windows page-side keyboard shortcuts.
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
//! Trust model: a page could post fabricated action ids to this channel
//! without any keystroke. The dispatcher therefore only accepts an action
//! from the webview that is currently some workspace's **active tab**, and
//! stamps the emitted event with the source tab so the chrome additionally
//! drops actions whose source is not in the acting window's own workspace
//! (`useKeyActions`). Residual exposure: the page the user is actively
//! viewing can still trigger chrome actions on itself (close/navigate its own
//! tab), and any window's active tab can trigger the nuisance-tier find/popout
//! listeners, which do not check the source. macOS is unaffected (native
//! accelerators, no such channel).

use std::collections::HashMap;
use std::sync::OnceLock;

use tauri::{AppHandle, Emitter, Manager};
use wis_core::{KeyAction, TabId};

static APP: OnceLock<AppHandle> = OnceLock::new();

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
  function normalize(accel) {
    var mods = {};
    var key = '';
    var parts = accel.split('+');
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

/// The forwarder init script with the effective keymap baked in.
pub fn page_script(keymap: &HashMap<KeyAction, String>) -> String {
    KEYBIND_JS
        .replace("__WIS_KEYMAP__", &keymap_json(keymap))
        .replace("__WIS_RESERVED__", &reserved_json())
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
            let _ = webview.eval(format!(
                "window.__wisKeybindUpdate && window.__wisKeybindUpdate({json});"
            ));
        }
    }
}
