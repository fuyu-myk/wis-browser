//! Wis browser core data model.
//!
//! Pure, immutable types describing the browser's persistent state:
//! workspaces, tabs, settings, and the operations that produce new states
//! from old ones.

pub mod error;
pub mod ids;
pub mod layout;
pub mod privacy;
pub mod settings;
pub mod state;
pub mod tab;
pub mod widget;
pub mod workspace;

pub use error::{WisError, WisResult};
pub use ids::{TabId, WidgetId, WorkspaceId};
pub use layout::{LayoutMode, RailSide, ThemeMode, UrlBarPosition};
pub use privacy::{PerSitePolicy, PrivacyMode};
pub use settings::{UserSettings, UserSettingsPatch};
pub use state::BrowserState;
pub use tab::{FaviconRef, Tab};
pub use widget::{Widget, WidgetKind};
pub use workspace::{AccentColor, EssentialPin, Workspace};
