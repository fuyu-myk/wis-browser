//! Chrome layout and theme modes. These describe how the browser UI itself
//! is arranged — tabs on the left/right/top — independent of the page being
//! rendered.

use serde::{Deserialize, Serialize};

/// Position of the tab strip relative to the viewport.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutMode {
    /// Vertical sidebar on the left.
    #[default]
    SidebarLeft,
    /// Vertical sidebar on the right.
    SidebarRight,
    /// Horizontal tab strip at the top.
    TopTabs,
}

/// Light/dark/system theme selection.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    Light,
    Dark,
    /// Follow the operating system's appearance setting.
    #[default]
    System,
}
