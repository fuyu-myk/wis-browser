//! Wis browser core data model.
//!
//! Pure, immutable types describing the browser's persistent state:
//! workspaces, tabs, settings, and the operations that produce new states
//! from old ones.

pub mod ai;
pub mod appearance;
pub mod canvas;
pub mod error;
pub mod extensions;
pub mod ids;
pub mod library;
pub mod navigation;
pub mod privacy;
pub mod settings;
pub mod sites;
pub mod state;
pub mod tabs;
pub mod vault;
pub mod workspaces;

pub use ai::chat::{
    AiProvider, ChatMessage, ChatRequest, Role, SseParser, StreamEvent, ThinkingMode,
};
pub use appearance::background::{
    Background, BackgroundDrafts, GradientSpec, GradientStop, ImageBackground, ImageFit,
};
pub use appearance::design::{
    ChromeDesign, CloseButton, CornerStyle, Density, FolderStyle, GlassIntensity, GlassScopes,
    TabIndicator, TabShape,
};
pub use appearance::icon::{parse_icon_ref, IconRef};
pub use appearance::layout::{LayoutMode, MotionLevel, RailSide, ThemeMode};
pub use appearance::theme::{CustomTheme, ModeTokens, ThemeFile, ThemeTokens, THEME_FILE_VERSION};
pub use appearance::widget::{Widget, WidgetGroup, WidgetKind};
pub use error::{WisError, WisResult};
pub use ids::{
    CanvasId, DownloadId, TabFolderId, TabId, ThemeId, UserscriptId, WidgetId, WorkspaceId,
};
pub use library::download::{
    sanitize_filename, unique_filename, DownloadStatus, FALLBACK_FILENAME,
};
pub use library::netscape::{parse_netscape, to_netscape, ParsedBookmark, ParsedBookmarks};
pub use navigation::link_clean::{clean_followed_link, clean_link, CleanedLink};
pub use navigation::link_safety::{assess_link, LinkAssessment, LinkWarning, RiskLevel};
pub use navigation::load_error::{LoadError, LoadErrorKind, TlsFault};
pub use navigation::search::{
    sanitize_builtin_search_keywords, sanitize_search_engines, sanitize_search_keyword,
    CustomSearchEngine, SearchEngine, MAX_CUSTOM_ENGINES, SEARCH_TERMS_TOKEN,
};
pub use privacy::clear::clear_since;
pub use privacy::policy::{PerSitePolicy, PrivacyMode};
pub use settings::keybinding::{default_keymap, effective_keymap, KeyAction};
pub use settings::user_settings::AiSettings;
pub use settings::user_settings::{UserSettings, UserSettingsPatch};
pub use sites::autoplay::{autoplay_policy_for_host, AutoplayPolicy};
pub use sites::protection::{site_protection_for_host, SiteProtection};
pub use sites::user_agent::{ua_for_host, UaPlatform, UaProfile};
pub use sites::zoom::{
    sane_zoom_percent, stepped_zoom_percent, zoom_factor, zoom_percent_for_host, ZoomStep,
    DEFAULT_ZOOM_PERCENT, MAX_ZOOM_PERCENT, MIN_ZOOM_PERCENT, ZOOM_LADDER,
};
pub use state::BrowserState;
pub use tabs::archive::TabArchiveAfter;
pub use tabs::folder::TabFolder;
pub use tabs::new_tab::{
    default_shortcuts, sanitize_homepage, sanitize_shortcuts, NewTabTarget, ShortcutTile,
    MAX_SHORTCUTS,
};
pub use tabs::pane::{PaneBranch, PaneNode, SplitDir};
pub use tabs::tab::{FaviconRef, Tab};
pub use vault::password_csv::{
    parse_password_csv, to_password_csv, ParsedCredential, ParsedCredentials,
};
pub use vault::password_offer::PasswordOffer;
pub use workspaces::proxy::UpstreamProxy;
pub use workspaces::workspace::{AccentColor, EssentialPin, Workspace};
