//! Workspace — a named group of tabs with its own accent colour,
//! essentials, and privacy preset.

use crate::ids::{TabId, WorkspaceId};
use crate::privacy::PrivacyMode;
use serde::{Deserialize, Serialize};

/// Accent colours available for a workspace,
/// to colour the workspace pill, active-tab indicator, etc.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccentColor {
    #[default]
    Primary,
    Mint,
    Rose,
    Sun,
    Lila,
    Gray,
    Ink,
    Azur,
}

/// One of the four "essentials" pinned shortcuts shown above the tab list.
/// Slot indices are 0..=3.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EssentialPin {
    pub slot: u8,
    pub label: String,
    pub url: String,
    pub icon: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
    pub accent: AccentColor,
    pub icon: Option<String>,
    pub tab_order: Vec<TabId>,
    pub pinned_tab_ids: Vec<TabId>,
    pub essentials: Vec<EssentialPin>,
    pub active_tab_id: Option<TabId>,
    pub privacy_mode: PrivacyMode,
}

impl Workspace {
    pub fn new(name: String, accent: AccentColor) -> Self {
        Self {
            id: WorkspaceId::new(),
            name,
            accent,
            icon: None,
            tab_order: Vec::new(),
            pinned_tab_ids: Vec::new(),
            essentials: Vec::new(),
            active_tab_id: None,
            privacy_mode: PrivacyMode::default(),
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

    /// Append a tab to the workspace. If the workspace had no active tab,
    /// the new tab becomes active.
    pub fn with_tab_appended(&self, tab_id: TabId) -> Self {
        let mut next = self.clone();

        if !next.tab_order.contains(&tab_id) {
            next.tab_order.push(tab_id);
        }

        if next.active_tab_id.is_none() {
            next.active_tab_id = Some(tab_id);
        }

        next
    }

    /// Remove a tab. If it was active, the previous tab in `tab_order` becomes
    /// active (or `None` if no tabs remain).
    pub fn with_tab_removed(&self, tab_id: TabId) -> Self {
        let mut next = self.clone();
        let removed_index = next.tab_order.iter().position(|t| *t == tab_id);

        next.tab_order.retain(|t| *t != tab_id);
        next.pinned_tab_ids.retain(|t| *t != tab_id);

        if next.active_tab_id == Some(tab_id) {
            next.active_tab_id = match removed_index {
                Some(idx) if idx > 0 => next.tab_order.get(idx - 1).copied(),
                Some(_) => next.tab_order.first().copied(),
                None => next.tab_order.last().copied(),
            };
        }

        next
    }

    /// Replace the tab ordering wholesale. The caller is responsible for
    /// ensuring the new order contains exactly the same tab IDs as before
    /// (validated at the [`crate::state::BrowserState`] level).
    pub fn with_tab_order(&self, order: Vec<TabId>) -> Self {
        Self {
            tab_order: order,
            ..self.clone()
        }
    }

    pub fn with_active_tab(&self, tab_id: TabId) -> Self {
        Self {
            active_tab_id: Some(tab_id),
            ..self.clone()
        }
    }

    pub fn with_pinned(&self, tab_id: TabId, pinned: bool) -> Self {
        let mut next = self.clone();
        if pinned {
            if !next.pinned_tab_ids.contains(&tab_id) {
                next.pinned_tab_ids.push(tab_id);
            }
        } else {
            next.pinned_tab_ids.retain(|t| *t != tab_id);
        }
        
        next
    }

    pub fn with_privacy_mode(&self, mode: PrivacyMode) -> Self {
        Self {
            privacy_mode: mode,
            ..self.clone()
        }
    }
}
