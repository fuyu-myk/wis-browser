//! User-controlled settings and an immutable patch type for partial updates.

use crate::layout::{LayoutMode, RailSide, ThemeMode, UrlBarPosition};
use crate::privacy::PrivacyMode;
use crate::workspace::AccentColor;
use serde::{Deserialize, Serialize};

const DEFAULT_DOH_RESOLVER: &str = "https://mozilla.cloudflare-dns.com/dns-query";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserSettings {
    pub layout: LayoutMode,
    /// Side hosting the always-collapsed rail when `layout == TopTabs`.
    /// In sidebar layouts the UI keeps this synced with the active side so
    /// switching to TopTabs feels continuous.
    #[serde(default)]
    pub top_tabs_rail_side: RailSide,
    pub url_bar_position: UrlBarPosition,
    pub theme: ThemeMode,
    pub accent: AccentColor,
    pub glass_enabled: bool,
    pub sidebar_collapsible: bool,
    pub compact: bool,
    pub default_privacy_mode: PrivacyMode,
    pub doh_resolver: Option<String>,
    pub homepage: Option<String>,
    /// Master switch for HTTPS interception. Off by default. Even
    /// when on, MITM only engages for workspaces in `PrivacyMode::Strict`.
    /// The user must trust the Wis CA in their OS keychain for inner sites
    /// to load without certificate errors.
    #[serde(default)]
    pub mitm_enabled: bool,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            layout: LayoutMode::SidebarLeft,
            top_tabs_rail_side: RailSide::Left,
            url_bar_position: UrlBarPosition::Sidebar,
            theme: ThemeMode::System,
            accent: AccentColor::Primary,
            glass_enabled: true,
            sidebar_collapsible: true,
            compact: false,
            default_privacy_mode: PrivacyMode::Balanced,
            doh_resolver: Some(DEFAULT_DOH_RESOLVER.to_string()),
            homepage: None,
            mitm_enabled: false,
        }
    }
}

/// Partial update for [`UserSettings`]. `None` on a field means "no change";
/// `Some(value)` replaces that field. Clearing optional fields (setting them
/// back to `None`) is intentionally not expressible here: use a dedicated
/// command if/when that is needed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserSettingsPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<LayoutMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_tabs_rail_side: Option<RailSide>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url_bar_position: Option<UrlBarPosition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<ThemeMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent: Option<AccentColor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glass_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidebar_collapsible: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_privacy_mode: Option<PrivacyMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doh_resolver: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mitm_enabled: Option<bool>,
}

impl UserSettings {
    /// Return a new `UserSettings` with the patch applied. Self is not mutated.
    pub fn with_patch(&self, patch: UserSettingsPatch) -> Self {
        Self {
            layout: patch.layout.unwrap_or(self.layout),
            top_tabs_rail_side: patch.top_tabs_rail_side.unwrap_or(self.top_tabs_rail_side),
            url_bar_position: patch.url_bar_position.unwrap_or(self.url_bar_position),
            theme: patch.theme.unwrap_or(self.theme),
            accent: patch.accent.unwrap_or(self.accent),
            glass_enabled: patch.glass_enabled.unwrap_or(self.glass_enabled),
            sidebar_collapsible: patch
                .sidebar_collapsible
                .unwrap_or(self.sidebar_collapsible),
            compact: patch.compact.unwrap_or(self.compact),
            default_privacy_mode: patch
                .default_privacy_mode
                .unwrap_or(self.default_privacy_mode),
            doh_resolver: patch.doh_resolver.or_else(|| self.doh_resolver.clone()),
            homepage: patch.homepage.or_else(|| self.homepage.clone()),
            mitm_enabled: patch.mitm_enabled.unwrap_or(self.mitm_enabled),
        }
    }
}
