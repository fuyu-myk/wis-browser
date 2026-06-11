//! Wis browser core data model.
//!
//! Pure, immutable types describing the browser's persistent state:
//! workspaces, tabs, settings, and the operations that produce new states
//! from old ones.

pub mod background;
pub mod download;
pub mod error;
pub mod icon;
pub mod ids;
pub mod keybinding;
pub mod layout;
pub mod link_safety;
pub mod pane;
pub mod privacy;
pub mod proxy;
pub mod search;
pub mod settings;
pub mod state;
pub mod tab;
pub mod tab_folder;
pub mod theme;
pub mod user_agent;
pub mod widget;
pub mod workspace;

pub use background::{
    Background, BackgroundDrafts, GradientSpec, GradientStop, ImageBackground, ImageFit,
};
pub use download::{sanitize_filename, unique_filename, DownloadStatus, FALLBACK_FILENAME};
pub use error::{WisError, WisResult};
pub use icon::{parse_icon_ref, IconRef};
pub use ids::{DownloadId, TabFolderId, TabId, ThemeId, WidgetId, WorkspaceId};
pub use keybinding::{default_keymap, effective_keymap, KeyAction};
pub use layout::{LayoutMode, RailSide, ThemeMode};
pub use link_safety::{assess_link, LinkAssessment, LinkWarning, RiskLevel};
pub use pane::{PaneBranch, PaneNode, SplitDir};
pub use privacy::{PerSitePolicy, PrivacyMode};
pub use proxy::UpstreamProxy;
pub use search::SearchEngine;
pub use settings::{UserSettings, UserSettingsPatch};
pub use state::BrowserState;
pub use tab::{FaviconRef, Tab};
pub use tab_folder::TabFolder;
pub use theme::{CustomTheme, ModeTokens, ThemeFile, ThemeTokens, THEME_FILE_VERSION};
pub use user_agent::{ua_for_host, UaProfile, DEFAULT_MAC_OS_VERSION};
pub use widget::{Widget, WidgetKind};
pub use workspace::{AccentColor, EssentialPin, Workspace};
