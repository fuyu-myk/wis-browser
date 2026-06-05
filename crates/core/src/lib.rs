//! Wis browser core data model.
//!
//! Pure, immutable types describing the browser's persistent state:
//! workspaces, tabs, settings, and the operations that produce new states
//! from old ones.

pub mod error;
pub mod ids;
pub mod keybinding;
pub mod layout;
pub mod pane;
pub mod privacy;
pub mod search;
pub mod settings;
pub mod state;
pub mod tab;
pub mod widget;
pub mod workspace;

pub use error::{WisError, WisResult};
pub use ids::{TabId, WidgetId, WorkspaceId};
pub use keybinding::{default_keymap, effective_keymap, KeyAction};
pub use layout::{LayoutMode, RailSide, ThemeMode};
pub use pane::{PaneBranch, PaneNode, SplitDir};
pub use privacy::{PerSitePolicy, PrivacyMode};
pub use search::SearchEngine;
pub use settings::{UserSettings, UserSettingsPatch};
pub use state::BrowserState;
pub use tab::{FaviconRef, Tab};
pub use widget::{Widget, WidgetKind};
pub use workspace::{AccentColor, EssentialPin, Workspace};
