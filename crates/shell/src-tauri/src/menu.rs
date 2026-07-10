//! Native macOS menu wiring for Wis keyboard shortcuts.
//!
//! Tauri menu accelerators intercept keystrokes before any WKWebView gets
//! them, so the same shortcut works whether the chrome or a tab webview has
//! focus. Each menu item carries `KeyAction::id()` as its stable id; when
//! the user clicks the item (or presses its accelerator) we emit a Tauri
//! event the chrome JS listens to and dispatches.

use std::collections::HashMap;

use serde::Serialize;
use tauri::menu::{
    AboutMetadataBuilder, MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder,
};
use tauri::{App, AppHandle, Emitter};
use wis_core::KeyAction;

/// Tauri event name the chrome listens to for menu / accelerator triggers.
pub const KEYACTION_EVENT: &str = "wis-keyaction";

#[derive(Debug, Clone, Serialize)]
struct KeyActionPayload<'a> {
    action: &'a str,
}

/// One-time install at app setup: builds the native menu from `keymap`,
/// installs it, and registers the menu-event dispatch handler. The
/// handler stays live for the app's lifetime; subsequent keymap changes
/// go through [`apply_menu`] which only swaps the menu itself.
pub fn install_menu(app: &App, keymap: &HashMap<KeyAction, String>) -> tauri::Result<()> {
    let handle = app.handle().clone();
    apply_menu(&handle, keymap)?;
    let dispatch_handle = handle.clone();
    app.on_menu_event(move |_app, event| dispatch_keyaction(&dispatch_handle, event.id().as_ref()));

    Ok(())
}

/// Rebuild the menu in place using a fresh keymap. Called when the user
/// updates `settings.keybindings`. Does not register the menu-event
/// handler (done once by [`install_menu`]).
pub fn apply_menu(app: &AppHandle, keymap: &HashMap<KeyAction, String>) -> tauri::Result<()> {
    let menu = build_menu(app, keymap)?;

    #[cfg(target_os = "macos")]
    {
        menu.set_as_app_menu()?;
        app.set_menu(menu)?;
    }

    #[cfg(target_os = "windows")]
    {
        app.set_menu(menu)?;
        if let Err(err) = app.hide_menu() {
            log::warn!("hide native menu bar failed: {err}");
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = menu;

    log::info!(
        "native menu (re)built with {} bound accelerators",
        keymap.len()
    );

    Ok(())
}

/// Resolve a `KeyAction` id and broadcast it as `wis-keyaction` (menu clicks +
/// native accelerators).
fn dispatch_keyaction(app: &AppHandle, item_id: &str) {
    let Some(action) = KeyAction::from_id(item_id) else {
        return;
    };
    if let Err(err) = app.emit(
        KEYACTION_EVENT,
        KeyActionPayload {
            action: action.id(),
        },
    ) {
        log::warn!("emit {KEYACTION_EVENT} for {item_id} failed: {err}");
    }
}

fn build_menu(
    app: &AppHandle,
    keymap: &HashMap<KeyAction, String>,
) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    let about = PredefinedMenuItem::about(
        app,
        Some("About Wis"),
        Some(AboutMetadataBuilder::new().name(Some("Wis")).build()),
    )?;
    let app_menu = SubmenuBuilder::new(app, "Wis")
        .item(&about)
        .item(&action_item(
            app,
            keymap,
            KeyAction::OpenSettings,
            "Settings.",
        )?)
        .separator()
        .item(&PredefinedMenuItem::hide(app, None)?)
        .item(&PredefinedMenuItem::hide_others(app, None)?)
        .item(&PredefinedMenuItem::show_all(app, None)?)
        .separator()
        .item(&PredefinedMenuItem::quit(app, None)?)
        .build()?;

    let file_menu = SubmenuBuilder::new(app, "File")
        .item(&action_item(app, keymap, KeyAction::NewTab, "New Tab")?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::NewWorkspace,
            "New Workspace",
        )?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::NewWindow,
            "New Window",
        )?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::NewPrivateWindow,
            "New Private Window",
        )?)
        .separator()
        .item(&action_item(app, keymap, KeyAction::CloseTab, "Close Tab")?)
        .separator()
        .item(&action_item(app, keymap, KeyAction::Print, "Print…")?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::CaptureVisible,
            "Capture Screenshot",
        )?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::CaptureRegion,
            "Capture Selected Region…",
        )?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::CaptureWindow,
            "Capture Window",
        )?)
        .separator()
        .item(&action_item(
            app,
            keymap,
            KeyAction::OpenDownloads,
            "Show Downloads",
        )?)
        .build()?;

    let edit_menu = SubmenuBuilder::new(app, "Edit")
        .item(&PredefinedMenuItem::undo(app, None)?)
        .item(&PredefinedMenuItem::redo(app, None)?)
        .separator()
        .item(&PredefinedMenuItem::cut(app, None)?)
        .item(&PredefinedMenuItem::copy(app, None)?)
        .item(&PredefinedMenuItem::paste(app, None)?)
        .item(&PredefinedMenuItem::select_all(app, None)?)
        .separator()
        .item(&action_item(
            app,
            keymap,
            KeyAction::FindInPage,
            "Find in Page.",
        )?)
        .item(&action_item(app, keymap, KeyAction::FindNext, "Find Next")?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::FindPrevious,
            "Find Previous",
        )?)
        .separator()
        .item(&action_item(
            app,
            keymap,
            KeyAction::CopyUrl,
            "Copy Current URL",
        )?)
        .build()?;

    let view_menu = SubmenuBuilder::new(app, "View")
        .item(&action_item(app, keymap, KeyAction::Reload, "Reload")?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::HardReload,
            "Reload (Bypass Cache)",
        )?)
        .separator()
        .item(&action_item(
            app,
            keymap,
            KeyAction::ToggleReader,
            "Reader Mode",
        )?)
        .separator()
        .item(&action_item(app, keymap, KeyAction::ZoomIn, "Zoom In")?)
        .item(&action_item(app, keymap, KeyAction::ZoomOut, "Zoom Out")?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::ZoomReset,
            "Actual Size",
        )?)
        .separator()
        .item(&action_item(
            app,
            keymap,
            KeyAction::ToggleSidebar,
            "Toggle Floating Sidebar",
        )?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::ElementPicker,
            "Hide Element…",
        )?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::PopOutVideo,
            "Pop Out Video",
        )?)
        .separator()
        .item(&action_item(
            app,
            keymap,
            KeyAction::FocusUrlBar,
            "Focus Address Bar",
        )?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::FocusUrlBarAlt,
            "Focus Address Bar (alt)",
        )?)
        .separator()
        .item(&action_item(
            app,
            keymap,
            KeyAction::OpenCommandPalette,
            "Open Command Palette",
        )?)
        .build()?;

    let history_menu = SubmenuBuilder::new(app, "History")
        .item(&action_item(app, keymap, KeyAction::GoBack, "Back")?)
        .item(&action_item(app, keymap, KeyAction::GoForward, "Forward")?)
        .separator()
        .item(&action_item(
            app,
            keymap,
            KeyAction::GoBackAlt,
            "Back (alt)",
        )?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::GoForwardAlt,
            "Forward (alt)",
        )?)
        .separator()
        .item(&action_item(
            app,
            keymap,
            KeyAction::OpenHistory,
            "Show All History",
        )?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::ClearBrowsingData,
            "Clear Browsing Data…",
        )?)
        .build()?;

    let bookmarks_menu = SubmenuBuilder::new(app, "Bookmarks")
        .item(&action_item(
            app,
            keymap,
            KeyAction::BookmarkPage,
            "Bookmark This Page",
        )?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::ToggleBookmarks,
            "Show Bookmarks",
        )?)
        .build()?;

    let tabs_menu = SubmenuBuilder::new(app, "Tabs")
        .item(&action_item(
            app,
            keymap,
            KeyAction::SwitchToTab1,
            "Switch to Tab 1",
        )?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::SwitchToTab2,
            "Switch to Tab 2",
        )?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::SwitchToTab3,
            "Switch to Tab 3",
        )?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::SwitchToTab4,
            "Switch to Tab 4",
        )?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::SwitchToTab5,
            "Switch to Tab 5",
        )?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::SwitchToTab6,
            "Switch to Tab 6",
        )?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::SwitchToTab7,
            "Switch to Tab 7",
        )?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::SwitchToTab8,
            "Switch to Tab 8",
        )?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::SwitchToLastTab,
            "Switch to Last Tab",
        )?)
        .separator()
        .item(&action_item(app, keymap, KeyAction::NextTab, "Next Tab")?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::PreviousTab,
            "Previous Tab",
        )?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::RestoreClosedTab,
            "Reopen Closed Tab",
        )?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::PinTab,
            "Pin or Unpin Tab",
        )?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::DuplicateTab,
            "Duplicate Tab",
        )?)
        .separator()
        .item(&action_item(
            app,
            keymap,
            KeyAction::NextWorkspace,
            "Next Workspace",
        )?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::PreviousWorkspace,
            "Previous Workspace",
        )?)
        .build()?;

    let panes_menu = SubmenuBuilder::new(app, "Panes")
        .item(&action_item(
            app,
            keymap,
            KeyAction::SplitPaneHorizontal,
            "Split Right",
        )?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::SplitPaneVertical,
            "Split Down",
        )?)
        .separator()
        .item(&action_item(
            app,
            keymap,
            KeyAction::FocusPaneLeft,
            "Focus Pane Left",
        )?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::FocusPaneRight,
            "Focus Pane Right",
        )?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::FocusPaneUp,
            "Focus Pane Up",
        )?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::FocusPaneDown,
            "Focus Pane Down",
        )?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::FocusNextPane,
            "Focus Next Pane",
        )?)
        .separator()
        .item(&action_item(
            app,
            keymap,
            KeyAction::UnsplitPanes,
            "Unsplit (Single Pane)",
        )?)
        .item(&action_item(
            app,
            keymap,
            KeyAction::ClosePane,
            "Close Pane",
        )?)
        .build()?;

    let window_menu = SubmenuBuilder::new(app, "Window")
        .item(&PredefinedMenuItem::minimize(app, None)?)
        .item(&PredefinedMenuItem::maximize(app, None)?)
        .separator()
        .item(&PredefinedMenuItem::close_window(app, None)?)
        .build()?;

    MenuBuilder::new(app)
        .items(&[
            &app_menu,
            &file_menu,
            &edit_menu,
            &view_menu,
            &history_menu,
            &bookmarks_menu,
            &tabs_menu,
            &panes_menu,
            &window_menu,
        ])
        .build()
}

fn action_item(
    app: &AppHandle,
    keymap: &HashMap<KeyAction, String>,
    action: KeyAction,
    label: &str,
) -> tauri::Result<tauri::menu::MenuItem<tauri::Wry>> {
    let mut builder = MenuItemBuilder::with_id(action.id(), label);
    if let Some(accel) = keymap.get(&action) {
        builder = builder.accelerator(accel);
    }
    builder.build(app)
}
