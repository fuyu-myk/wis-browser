//! Wis browser core data model.
//!
//! Pure, immutable types describing the browser's persistent state:
//! workspaces, tabs, settings, and the operations that produce new states
//! from old ones.

pub mod ai;
pub mod ai_models;
pub mod app_launch;
pub mod autoplay;
pub mod background;
pub mod backup;
pub mod bookmark_tree;
pub mod canvas;
pub mod canvas_tools;
pub mod clear;
pub mod design;
pub mod download;
pub mod error;
pub mod icon;
pub mod ids;
pub mod keybinding;
pub mod layout;
pub mod link_clean;
pub mod link_safety;
pub mod load_error;
pub mod netscape;
pub mod new_tab;
pub mod pane;
pub mod password_csv;
pub mod password_offer;
pub mod privacy;
pub mod proxy;
pub mod search;
pub mod settings;
pub mod site;
pub mod state;
pub mod tab;
pub mod tab_folder;
pub mod theme;
pub mod usage;
pub mod user_agent;
pub mod userscript;
pub mod widget;
pub mod workspace;
pub mod zoom;

pub use ai::{AiProvider, ChatMessage, ChatRequest, Role, SseParser, StreamEvent, ThinkingMode};
pub use autoplay::{autoplay_policy_for_host, AutoplayPolicy};
pub use background::{
    Background, BackgroundDrafts, GradientSpec, GradientStop, ImageBackground, ImageFit,
};
pub use clear::clear_since;
pub use design::{
    ChromeDesign, CloseButton, CornerStyle, Density, FolderStyle, GlassIntensity, GlassScopes,
    TabIndicator, TabShape,
};
pub use download::{sanitize_filename, unique_filename, DownloadStatus, FALLBACK_FILENAME};
pub use error::{WisError, WisResult};
pub use icon::{parse_icon_ref, IconRef};
pub use ids::{
    CanvasId, DownloadId, TabFolderId, TabId, ThemeId, UserscriptId, WidgetId, WorkspaceId,
};
pub use keybinding::{default_keymap, effective_keymap, KeyAction};
pub use layout::{LayoutMode, MotionLevel, RailSide, ThemeMode};
pub use link_clean::{clean_link, CleanedLink};
pub use link_safety::{assess_link, LinkAssessment, LinkWarning, RiskLevel};
pub use load_error::{LoadError, LoadErrorKind, TlsFault};
pub use netscape::{parse_netscape, to_netscape, ParsedBookmark, ParsedBookmarks};
pub use new_tab::{
    default_shortcuts, sanitize_homepage, sanitize_shortcuts, NewTabTarget, ShortcutTile,
    MAX_SHORTCUTS,
};
pub use pane::{PaneBranch, PaneNode, SplitDir};
pub use password_csv::{parse_password_csv, to_password_csv, ParsedCredential, ParsedCredentials};
pub use password_offer::PasswordOffer;
pub use privacy::{PerSitePolicy, PrivacyMode};
pub use proxy::UpstreamProxy;
pub use search::{
    sanitize_builtin_search_keywords, sanitize_search_engines, sanitize_search_keyword,
    CustomSearchEngine, SearchEngine, MAX_CUSTOM_ENGINES, SEARCH_TERMS_TOKEN,
};
pub use settings::AiSettings;
pub use settings::{UserSettings, UserSettingsPatch};
pub use state::BrowserState;
pub use tab::{FaviconRef, Tab};
pub use tab_folder::TabFolder;
pub use theme::{CustomTheme, ModeTokens, ThemeFile, ThemeTokens, THEME_FILE_VERSION};
pub use user_agent::{ua_for_host, UaPlatform, UaProfile};
pub use widget::{Widget, WidgetGroup, WidgetKind};
pub use workspace::{AccentColor, EssentialPin, Workspace};
pub use zoom::{
    sane_zoom_percent, stepped_zoom_percent, zoom_factor, zoom_percent_for_host, ZoomStep,
    DEFAULT_ZOOM_PERCENT, MAX_ZOOM_PERCENT, MIN_ZOOM_PERCENT, ZOOM_LADDER,
};
