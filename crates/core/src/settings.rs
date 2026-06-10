//! User-controlled settings and an immutable patch type for partial updates.

use std::collections::HashMap;

use crate::keybinding::KeyAction;
use crate::layout::{LayoutMode, RailSide, ThemeMode};
use crate::privacy::PrivacyMode;
use crate::search::SearchEngine;
use crate::user_agent::UaProfile;
use crate::widget::Widget;
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
    pub theme: ThemeMode,
    pub accent: AccentColor,
    pub glass_enabled: bool,
    pub sidebar_collapsible: bool,
    pub compact: bool,
    pub default_privacy_mode: PrivacyMode,
    pub doh_resolver: Option<String>,
    pub homepage: Option<String>,
    /// Default search engine for non-URL address-bar input. Serde-default so
    /// existing persisted settings deserialise to DuckDuckGo.
    #[serde(default)]
    pub search_engine: SearchEngine,
    /// When true, the shell fetches the EasyPrivacy filter list on launch
    /// (and on demand) into the per-user cache, then hot-swaps it into the
    /// running tracker engine. When false, the bundled list is used.
    #[serde(default = "default_filter_list_auto_update")]
    pub filter_list_auto_update: bool,
    /// When true, focusing the URL bar lights its border in the accent
    /// colour. Off by default; the LaunchPad omnisearch always glows.
    #[serde(default)]
    pub url_bar_focus_glow: bool,
    /// When true, the browser opens directly into a private (ephemeral)
    /// workspace on launch instead of restoring the persisted session.
    #[serde(default)]
    pub always_private: bool,
    /// When true, background tab webviews are discarded after they have been
    /// hidden for [`Self::tab_sleep_minutes`], freeing memory; they reload on
    /// next focus. The active tab and pinned tabs are never slept.
    #[serde(default = "default_tab_sleep_enabled")]
    pub tab_sleep_enabled: bool,
    /// Idle minutes a background tab stays hidden before its webview is
    /// discarded. Only consulted when [`Self::tab_sleep_enabled`].
    #[serde(default = "default_tab_sleep_minutes")]
    pub tab_sleep_minutes: u32,
    /// When true, in-memory session cookies (login sessions) are snapshotted,
    /// encrypted, and restored on the next launch, so logins survive a restart.
    /// Restored sessions still expire after a fixed cap (see the shell's session
    /// cookie TTL). Off means logins end when the app quits (most private).
    #[serde(default = "default_persist_sessions")]
    pub persist_sessions: bool,
    /// When true, remote webviews enable WebKit's fraudulent-website warning
    /// (Apple-proxied Safe Browsing): a full-page warning before suspected
    /// phishing/malware sites. Complements the structural heuristics in
    /// [`crate::link_safety`] with a real threat database. On by default;
    /// exposed as a toggle because the check performs network lookups.
    #[serde(default = "default_fraud_warning_enabled")]
    pub fraud_warning_enabled: bool,
    /// Default user agent presented to remote pages. Safari by default so the
    /// WebKit engine is identified correctly. Serde-default upgrades existing
    /// settings to the fix automatically.
    #[serde(default)]
    pub user_agent_profile: UaProfile,
    /// Per-host user agent overrides for sites that still special-case a
    /// particular browser. Keyed by `www.`-stripped host.
    #[serde(default)]
    pub ua_site_overrides: HashMap<String, UaProfile>,
    /// Global quick-launch sidebar widgets (social apps + AI providers), in
    /// display order. Shown in every workspace's sidebar; clicking one opens
    /// its URL in a split pane. Empty by default.
    #[serde(default)]
    pub sidebar_widgets: Vec<Widget>,
    /// When true, at most [`Self::widget_panel_limit`] widget panels stay
    /// loaded (hidden for instant reopening); beyond that the
    /// least-recently-used hidden one is reclaimed. Off means limitless.
    #[serde(default = "default_widget_panel_limit_enabled")]
    pub widget_panel_limit_enabled: bool,
    /// Loaded widget panels kept before the oldest hidden one is reclaimed.
    /// Only consulted when [`Self::widget_panel_limit_enabled`].
    #[serde(default = "default_widget_panel_limit")]
    pub widget_panel_limit: u32,
    /// User-supplied accelerator overrides keyed by [`KeyAction`]. Each
    /// entry replaces the corresponding entry from [`crate::default_keymap`]
    /// at runtime (see [`crate::effective_keymap`]). Absent entries fall
    /// back to the default. A serde-default empty map means existing
    /// persisted settings deserialise unchanged.
    #[serde(default)]
    pub keybindings: HashMap<KeyAction, String>,
}

fn default_filter_list_auto_update() -> bool {
    true
}

fn default_tab_sleep_enabled() -> bool {
    true
}

fn default_tab_sleep_minutes() -> u32 {
    15
}

fn default_persist_sessions() -> bool {
    true
}

fn default_fraud_warning_enabled() -> bool {
    true
}

fn default_widget_panel_limit_enabled() -> bool {
    true
}

fn default_widget_panel_limit() -> u32 {
    3
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            layout: LayoutMode::SidebarLeft,
            top_tabs_rail_side: RailSide::Left,
            theme: ThemeMode::System,
            accent: AccentColor::Primary,
            glass_enabled: true,
            sidebar_collapsible: true,
            compact: false,
            default_privacy_mode: PrivacyMode::Balanced,
            doh_resolver: Some(DEFAULT_DOH_RESOLVER.to_string()),
            homepage: None,
            search_engine: SearchEngine::default(),
            filter_list_auto_update: true,
            url_bar_focus_glow: false,
            always_private: false,
            tab_sleep_enabled: true,
            tab_sleep_minutes: 15,
            persist_sessions: true,
            fraud_warning_enabled: true,
            user_agent_profile: UaProfile::default(),
            ua_site_overrides: HashMap::new(),
            sidebar_widgets: Vec::new(),
            widget_panel_limit_enabled: true,
            widget_panel_limit: 3,
            keybindings: HashMap::new(),
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
    pub search_engine: Option<SearchEngine>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter_list_auto_update: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url_bar_focus_glow: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub always_private: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_sleep_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_sleep_minutes: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persist_sessions: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fraud_warning_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent_profile: Option<UaProfile>,
    /// Replaces the whole override map when present (the frontend holds the
    /// full map). `None` keeps the current map.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ua_site_overrides: Option<HashMap<String, UaProfile>>,
    /// Replaces the whole widget list when present (the frontend holds the
    /// full ordered list). `None` keeps the current list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidebar_widgets: Option<Vec<Widget>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub widget_panel_limit_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub widget_panel_limit: Option<u32>,
    /// Merged onto the existing `keybindings` map. Sparse entries replace
    /// just those actions; absent actions keep their current binding.
    /// `Some(empty_map)` is a no-op (the patch is sparse, not a wipe).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keybindings: Option<HashMap<KeyAction, String>>,
}

impl UserSettings {
    /// Return a new `UserSettings` with the patch applied. Self is not mutated.
    pub fn with_patch(&self, patch: UserSettingsPatch) -> Self {
        Self {
            layout: patch.layout.unwrap_or(self.layout),
            top_tabs_rail_side: patch.top_tabs_rail_side.unwrap_or(self.top_tabs_rail_side),
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
            search_engine: patch.search_engine.unwrap_or(self.search_engine),
            filter_list_auto_update: patch
                .filter_list_auto_update
                .unwrap_or(self.filter_list_auto_update),
            url_bar_focus_glow: patch.url_bar_focus_glow.unwrap_or(self.url_bar_focus_glow),
            always_private: patch.always_private.unwrap_or(self.always_private),
            tab_sleep_enabled: patch.tab_sleep_enabled.unwrap_or(self.tab_sleep_enabled),
            tab_sleep_minutes: patch.tab_sleep_minutes.unwrap_or(self.tab_sleep_minutes),
            persist_sessions: patch.persist_sessions.unwrap_or(self.persist_sessions),
            fraud_warning_enabled: patch
                .fraud_warning_enabled
                .unwrap_or(self.fraud_warning_enabled),
            user_agent_profile: patch
                .user_agent_profile
                .unwrap_or_else(|| self.user_agent_profile.clone()),
            ua_site_overrides: patch
                .ua_site_overrides
                .unwrap_or_else(|| self.ua_site_overrides.clone()),
            sidebar_widgets: patch
                .sidebar_widgets
                .unwrap_or_else(|| self.sidebar_widgets.clone()),
            widget_panel_limit_enabled: patch
                .widget_panel_limit_enabled
                .unwrap_or(self.widget_panel_limit_enabled),
            widget_panel_limit: patch.widget_panel_limit.unwrap_or(self.widget_panel_limit),
            keybindings: match patch.keybindings {
                None => self.keybindings.clone(),
                Some(overrides) => {
                    let mut merged = self.keybindings.clone();
                    for (action, accel) in overrides {
                        if accel.is_empty() {
                            merged.remove(&action);
                        } else {
                            merged.insert(action, accel);
                        }
                    }

                    merged
                }
            },
        }
    }
}
