//! Chrome layout and theme modes. These describe how the browser UI itself
//! is arranged: tabs on the left/right/top, independent of the page being
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

/// Where the URL / address bar is rendered.
///
/// Serde reads `"sidebar"` and `"top"` natively.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UrlBarPosition {
    /// Inside the sidebar.
    #[default]
    Sidebar,
    /// Pinned at the top of the window, in line with the traffic lights.
    Top,
}

/// Which edge of the window hosts the always-collapsed rail when
/// `LayoutMode::TopTabs` is active. In sidebar layouts this mirrors
/// the chosen side automatically so toggling between layouts feels
/// continuous.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RailSide {
    #[default]
    Left,
    Right,
}

impl<'de> Deserialize<'de> for UrlBarPosition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        match raw.as_str() {
            "sidebar" => Ok(UrlBarPosition::Sidebar),
            "top" => Ok(UrlBarPosition::Top),
            other => Err(serde::de::Error::unknown_variant(
                other,
                &["sidebar", "top"],
            )),
        }
    }
}
