//! [`TabFolder`]: a named, collapsible grouping of *live* tabs within a workspace.
//! Distinct from bookmarks (which save URLs); this organises open tabs. Folder
//! membership lives on the tab ([`Tab::folder_id`](crate::tabs::tab::Tab::folder_id)); this type holds the folder's
//! own attributes. Immutable: all "mutations" return a new value.

use crate::ids::TabFolderId;
use crate::workspaces::workspace::AccentColor;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabFolder {
    pub id: TabFolderId,
    pub name: String,
    pub accent: AccentColor,
    /// Collapsed folders hide their tabs in the list and let those tabs sleep.
    pub collapsed: bool,
    /// Order among a workspace's folders (ascending).
    pub position: u32,
    /// Encoded icon reference (see [`crate::appearance::icon`]); `None` keeps the plain
    /// colour pill. Serde-defaults so existing folders load unchanged.
    #[serde(default)]
    pub icon: Option<String>,
}

impl TabFolder {
    pub fn new(name: String, accent: AccentColor, position: u32) -> Self {
        Self {
            id: TabFolderId::new(),
            name,
            accent,
            collapsed: false,
            position,
            icon: None,
        }
    }

    pub fn with_name(&self, name: String) -> Self {
        Self {
            name,
            ..self.clone()
        }
    }

    pub fn with_accent(&self, accent: AccentColor) -> Self {
        Self {
            accent,
            ..self.clone()
        }
    }

    pub fn with_collapsed(&self, collapsed: bool) -> Self {
        Self {
            collapsed,
            ..self.clone()
        }
    }

    pub fn with_position(&self, position: u32) -> Self {
        Self {
            position,
            ..self.clone()
        }
    }

    pub fn with_icon(&self, icon: Option<String>) -> Self {
        Self {
            icon,
            ..self.clone()
        }
    }
}
