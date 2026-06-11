//! Keyboard shortcut actions and default bindings.
//!
//! [`KeyAction`] enumerates everything a Wis keyboard shortcut can invoke.
//! The shell turns each action into a Tauri menu item whose accelerator is
//! either the built-in default (returned by [`default_keymap`]) or a
//! user-supplied override (merged in by `effective_keymap`, added in the
//! configurable-shortcuts pass).
//!
//! Accelerator strings are Tauri's syntax (`Cmd+T`, `Cmd+Shift+G`,
//! `Cmd+Left`).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyAction {
    NewTab,
    NewWorkspace,
    CloseTab,
    FocusUrlBar,
    FocusUrlBarAlt,
    Reload,
    GoBack,
    GoBackAlt,
    GoForward,
    GoForwardAlt,
    SwitchToTab1,
    SwitchToTab2,
    SwitchToTab3,
    SwitchToTab4,
    SwitchToTab5,
    SwitchToTab6,
    SwitchToTab7,
    SwitchToTab8,
    SwitchToTab9,
    SwitchToLastTab,
    FindInPage,
    FindNext,
    FindPrevious,
    OpenCommandPalette,
    OpenSettings,
    OpenHistory,
    ClearBrowsingData,
    NewPrivateWindow,
    NewWindow,
    /// Split the focused pane horizontally (side-by-side), opening a new tab in
    /// the new pane.
    SplitPaneHorizontal,
    /// Split the focused pane vertically (top/bottom), opening a new tab in the
    /// new pane.
    SplitPaneVertical,
    /// Close the focused pane (and its tab), collapsing the split.
    ClosePane,
    /// Move focus to the next pane in the split.
    FocusNextPane,
    /// Move focus to the pane to the left / right / above / below the focused
    /// one. No-op when not split.
    FocusPaneLeft,
    FocusPaneRight,
    FocusPaneUp,
    FocusPaneDown,
    /// Collapse all splits back to a single pane (the focused tab).
    UnsplitPanes,
    /// Reopen the most-recently-closed tab (repeat to go further back).
    RestoreClosedTab,
    /// Activate the next / previous workspace. No-op at the ends (no wrap).
    NextWorkspace,
    PreviousWorkspace,
    /// Activate the next / previous tab in the current workspace (wraps).
    NextTab,
    PreviousTab,
    /// Reload the active tab bypassing the cache.
    HardReload,
    /// Per-tab page zoom.
    ZoomIn,
    ZoomOut,
    ZoomReset,
    /// Toggle the downloads popup.
    OpenDownloads,
    /// Toggle the bookmarks popup.
    ToggleBookmarks,
    /// Bookmark (or un-bookmark) the active page, like the URL-bar star.
    BookmarkPage,
    /// Pin or unpin the active tab.
    PinTab,
    /// Open a new tab with the active tab's URL.
    DuplicateTab,
    /// Copy the active tab's URL to the clipboard.
    CopyUrl,
    /// Toggle the fixed/floating (auto-collapsing) sidebar mode.
    ToggleSidebar,
}

impl KeyAction {
    /// Stable string id used as the menu-item identifier and the IPC event
    /// payload. Matches the serde `snake_case` representation.
    pub fn id(self) -> &'static str {
        match self {
            Self::NewTab => "new_tab",
            Self::NewWorkspace => "new_workspace",
            Self::CloseTab => "close_tab",
            Self::FocusUrlBar => "focus_url_bar",
            Self::FocusUrlBarAlt => "focus_url_bar_alt",
            Self::Reload => "reload",
            Self::GoBack => "go_back",
            Self::GoBackAlt => "go_back_alt",
            Self::GoForward => "go_forward",
            Self::GoForwardAlt => "go_forward_alt",
            Self::SwitchToTab1 => "switch_to_tab1",
            Self::SwitchToTab2 => "switch_to_tab2",
            Self::SwitchToTab3 => "switch_to_tab3",
            Self::SwitchToTab4 => "switch_to_tab4",
            Self::SwitchToTab5 => "switch_to_tab5",
            Self::SwitchToTab6 => "switch_to_tab6",
            Self::SwitchToTab7 => "switch_to_tab7",
            Self::SwitchToTab8 => "switch_to_tab8",
            Self::SwitchToTab9 => "switch_to_tab9",
            Self::SwitchToLastTab => "switch_to_last_tab",
            Self::FindInPage => "find_in_page",
            Self::FindNext => "find_next",
            Self::FindPrevious => "find_previous",
            Self::OpenCommandPalette => "open_command_palette",
            Self::OpenSettings => "open_settings",
            Self::OpenHistory => "open_history",
            Self::ClearBrowsingData => "clear_browsing_data",
            Self::NewPrivateWindow => "new_private_window",
            Self::NewWindow => "new_window",
            Self::SplitPaneHorizontal => "split_pane_horizontal",
            Self::SplitPaneVertical => "split_pane_vertical",
            Self::ClosePane => "close_pane",
            Self::FocusNextPane => "focus_next_pane",
            Self::FocusPaneLeft => "focus_pane_left",
            Self::FocusPaneRight => "focus_pane_right",
            Self::FocusPaneUp => "focus_pane_up",
            Self::FocusPaneDown => "focus_pane_down",
            Self::UnsplitPanes => "unsplit_panes",
            Self::RestoreClosedTab => "restore_closed_tab",
            Self::NextWorkspace => "next_workspace",
            Self::PreviousWorkspace => "previous_workspace",
            Self::NextTab => "next_tab",
            Self::PreviousTab => "previous_tab",
            Self::HardReload => "hard_reload",
            Self::ZoomIn => "zoom_in",
            Self::ZoomOut => "zoom_out",
            Self::ZoomReset => "zoom_reset",
            Self::OpenDownloads => "open_downloads",
            Self::ToggleBookmarks => "toggle_bookmarks",
            Self::BookmarkPage => "bookmark_page",
            Self::PinTab => "pin_tab",
            Self::DuplicateTab => "duplicate_tab",
            Self::CopyUrl => "copy_url",
            Self::ToggleSidebar => "toggle_sidebar",
        }
    }

    /// Resolve a stable id back to the action. Used by the menu-event
    /// handler when translating Tauri's menu item id into a payload.
    pub fn from_id(id: &str) -> Option<Self> {
        ALL_ACTIONS.iter().copied().find(|a| a.id() == id)
    }

    /// Every variant, in display order. Useful for menu construction and
    /// for the settings UI that lists every bindable action.
    pub fn all() -> &'static [KeyAction] {
        &ALL_ACTIONS
    }
}

const ALL_ACTIONS: [KeyAction; 54] = [
    KeyAction::NewTab,
    KeyAction::NewWorkspace,
    KeyAction::CloseTab,
    KeyAction::FocusUrlBar,
    KeyAction::FocusUrlBarAlt,
    KeyAction::Reload,
    KeyAction::GoBack,
    KeyAction::GoBackAlt,
    KeyAction::GoForward,
    KeyAction::GoForwardAlt,
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
    KeyAction::FindInPage,
    KeyAction::FindNext,
    KeyAction::FindPrevious,
    KeyAction::OpenCommandPalette,
    KeyAction::OpenSettings,
    KeyAction::OpenHistory,
    KeyAction::ClearBrowsingData,
    KeyAction::NewPrivateWindow,
    KeyAction::NewWindow,
    KeyAction::SplitPaneHorizontal,
    KeyAction::SplitPaneVertical,
    KeyAction::ClosePane,
    KeyAction::FocusNextPane,
    KeyAction::FocusPaneLeft,
    KeyAction::FocusPaneRight,
    KeyAction::FocusPaneUp,
    KeyAction::FocusPaneDown,
    KeyAction::UnsplitPanes,
    KeyAction::RestoreClosedTab,
    KeyAction::NextWorkspace,
    KeyAction::PreviousWorkspace,
    KeyAction::NextTab,
    KeyAction::PreviousTab,
    KeyAction::HardReload,
    KeyAction::ZoomIn,
    KeyAction::ZoomOut,
    KeyAction::ZoomReset,
    KeyAction::OpenDownloads,
    KeyAction::ToggleBookmarks,
    KeyAction::BookmarkPage,
    KeyAction::PinTab,
    KeyAction::DuplicateTab,
    KeyAction::CopyUrl,
    KeyAction::ToggleSidebar,
];

/// Default keyboard accelerators. `NewWindow` has no entry on purpose: the
/// menu item appears with no shortcut until multi-window support arrives.
pub fn default_keymap() -> HashMap<KeyAction, String> {
    let pairs: &[(KeyAction, &str)] = &[
        (KeyAction::NewTab, "Cmd+T"),
        (KeyAction::NewWorkspace, "Cmd+N"),
        (KeyAction::CloseTab, "Cmd+W"),
        (KeyAction::FocusUrlBar, "Cmd+E"),
        (KeyAction::Reload, "Cmd+R"),
        (KeyAction::GoBack, "Cmd+["),
        (KeyAction::GoBackAlt, "Cmd+Left"),
        (KeyAction::GoForward, "Cmd+]"),
        (KeyAction::GoForwardAlt, "Cmd+Right"),
        (KeyAction::SwitchToTab1, "Cmd+1"),
        (KeyAction::SwitchToTab2, "Cmd+2"),
        (KeyAction::SwitchToTab3, "Cmd+3"),
        (KeyAction::SwitchToTab4, "Cmd+4"),
        (KeyAction::SwitchToTab5, "Cmd+5"),
        (KeyAction::SwitchToTab6, "Cmd+6"),
        (KeyAction::SwitchToTab7, "Cmd+7"),
        (KeyAction::SwitchToTab8, "Cmd+8"),
        (KeyAction::SwitchToLastTab, "Cmd+9"),
        (KeyAction::FindInPage, "Cmd+F"),
        (KeyAction::FindNext, "Cmd+G"),
        (KeyAction::FindPrevious, "Cmd+Shift+G"),
        (KeyAction::OpenCommandPalette, "Cmd+L"),
        (KeyAction::OpenSettings, "Cmd+,"),
        (KeyAction::OpenHistory, "Cmd+Shift+H"),
        (KeyAction::ClearBrowsingData, "Cmd+Shift+Backspace"),
        (KeyAction::NewPrivateWindow, "Cmd+Shift+N"),
        (KeyAction::SplitPaneHorizontal, "Cmd+D"),
        (KeyAction::SplitPaneVertical, "Cmd+Shift+D"),
        (KeyAction::ClosePane, "Alt+Cmd+W"),
        (KeyAction::FocusPaneLeft, "Alt+Cmd+Left"),
        (KeyAction::FocusPaneRight, "Alt+Cmd+Right"),
        (KeyAction::FocusPaneUp, "Alt+Cmd+Up"),
        (KeyAction::FocusPaneDown, "Alt+Cmd+Down"),
        (KeyAction::UnsplitPanes, "Alt+Cmd+U"),
        (KeyAction::RestoreClosedTab, "Cmd+Shift+T"),
        (KeyAction::NextWorkspace, "Alt+Cmd+E"),
        (KeyAction::PreviousWorkspace, "Alt+Cmd+Q"),
        (KeyAction::NextTab, "Cmd+Shift+]"),
        (KeyAction::PreviousTab, "Cmd+Shift+["),
        (KeyAction::HardReload, "Cmd+Shift+R"),
        (KeyAction::ZoomIn, "Cmd+="),
        (KeyAction::ZoomOut, "Cmd+-"),
        (KeyAction::ZoomReset, "Cmd+0"),
        (KeyAction::OpenDownloads, "Cmd+J"),
        (KeyAction::ToggleBookmarks, "Alt+Cmd+B"),
        (KeyAction::BookmarkPage, "Cmd+Shift+B"),
        (KeyAction::PinTab, "Cmd+Shift+P"),
        (KeyAction::DuplicateTab, "Cmd+Shift+K"),
        (KeyAction::CopyUrl, "Cmd+Shift+C"),
        (KeyAction::ToggleSidebar, "Cmd+S"),
    ];
    pairs.iter().map(|(a, s)| (*a, (*s).to_string())).collect()
}

/// Merge user-supplied overrides on top of the defaults. Actions
/// without a default and without an override are simply absent from the
/// result, which the menu layer interprets as "no shortcut".
pub fn effective_keymap(overrides: &HashMap<KeyAction, String>) -> HashMap<KeyAction, String> {
    let mut map = default_keymap();
    for (action, accel) in overrides {
        map.insert(*action, accel.clone());
    }

    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_has_a_stable_id_round_trip() {
        for action in KeyAction::all() {
            let id = action.id();
            assert_eq!(KeyAction::from_id(id), Some(*action), "{id}");
        }
    }

    #[test]
    fn default_keymap_includes_every_action_except_reserved() {
        // `NewWindow` is reserved for multi-window support in future.
        const UNBOUND: &[KeyAction] = &[
            KeyAction::NewWindow,
            KeyAction::FocusUrlBarAlt,
            KeyAction::FocusNextPane,
            KeyAction::SwitchToTab9,
        ];
        let map = default_keymap();
        for action in KeyAction::all() {
            if UNBOUND.contains(action) {
                assert!(
                    !map.contains_key(action),
                    "{} must stay unbound",
                    action.id()
                );
            } else {
                assert!(map.contains_key(action), "{} is unbound", action.id());
            }
        }
    }

    #[test]
    fn overrides_replace_defaults() {
        let mut overrides = HashMap::new();
        overrides.insert(KeyAction::NewTab, "Cmd+Shift+T".to_string());
        let effective = effective_keymap(&overrides);
        assert_eq!(effective.get(&KeyAction::NewTab).unwrap(), "Cmd+Shift+T");
        // Untouched action keeps default.
        assert_eq!(effective.get(&KeyAction::Reload).unwrap(), "Cmd+R");
    }

    #[test]
    fn serde_uses_snake_case() {
        let json = serde_json::to_string(&KeyAction::OpenCommandPalette).unwrap();
        assert_eq!(json, "\"open_command_palette\"");
        let back: KeyAction = serde_json::from_str("\"new_workspace\"").unwrap();
        assert_eq!(back, KeyAction::NewWorkspace);
    }
}
