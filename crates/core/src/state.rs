//! [`BrowserState`] — the root aggregate that holds workspaces, tabs, and
//! settings. All mutation operations are pure: they return a new value rather
//! than mutating self.

use crate::error::{WisError, WisResult};
use crate::ids::{TabId, WorkspaceId};
use crate::settings::{UserSettings, UserSettingsPatch};
use crate::tab::Tab;
use crate::workspace::{AccentColor, Workspace};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserState {
    pub workspaces: Vec<Workspace>,
    pub tabs: Vec<Tab>,
    pub active_workspace_id: WorkspaceId,
    pub settings: UserSettings,
}

impl BrowserState {
    /// Build a fresh state with a single "Personal" workspace and no tabs.
    pub fn new_with_defaults() -> Self {
        let workspace = Workspace::new("Personal".to_string(), AccentColor::Primary);
        let active_workspace_id = workspace.id;

        Self {
            workspaces: vec![workspace],
            tabs: Vec::new(),
            active_workspace_id,
            settings: UserSettings::default(),
        }
    }

    pub fn workspace(&self, id: WorkspaceId) -> WisResult<&Workspace> {
        self.workspaces
            .iter()
            .find(|w| w.id == id)
            .ok_or(WisError::WorkspaceNotFound { id })
    }

    pub fn tab(&self, id: TabId) -> WisResult<&Tab> {
        self.tabs
            .iter()
            .find(|t| t.id == id)
            .ok_or(WisError::TabNotFound { id })
    }

    /// Iterate the tabs belonging to a workspace in the workspace's ordering.
    pub fn tabs_for(&self, workspace_id: WorkspaceId) -> WisResult<Vec<&Tab>> {
        let workspace = self.workspace(workspace_id)?;

        Ok(workspace
            .tab_order
            .iter()
            .filter_map(|id| self.tabs.iter().find(|t| t.id == *id))
            .collect())
    }

    pub fn active_workspace(&self) -> &Workspace {
        self.workspaces
            .iter()
            .find(|w| w.id == self.active_workspace_id)
            .expect(
                "BrowserState invariant: active_workspace_id must refer to an existing workspace",
            )
    }

    pub fn with_new_workspace(&self, name: String, accent: AccentColor) -> (Self, Workspace) {
        let workspace = Workspace::new(name, accent);
        let mut workspaces = self.workspaces.clone();
        workspaces.push(workspace.clone());

        (
            Self {
                workspaces,
                ..self.clone()
            },
            workspace,
        )
    }

    pub fn with_renamed_workspace(&self, id: WorkspaceId, name: String) -> WisResult<Self> {
        if !self.workspaces.iter().any(|w| w.id == id) {
            return Err(WisError::WorkspaceNotFound { id });
        }

        let workspaces = self
            .workspaces
            .iter()
            .map(|w| {
                if w.id == id {
                    w.with_name(name.clone())
                } else {
                    w.clone()
                }
            })
            .collect();

        Ok(Self {
            workspaces,
            ..self.clone()
        })
    }

    pub fn with_workspace_removed(&self, id: WorkspaceId) -> WisResult<Self> {
        if self.workspaces.len() == 1 {
            return Err(WisError::CannotDeleteLastWorkspace);
        }

        let workspace = self.workspace(id)?;
        let tab_ids_to_remove: HashSet<TabId> = workspace.tab_order.iter().copied().collect();

        let tabs = self
            .tabs
            .iter()
            .filter(|t| !tab_ids_to_remove.contains(&t.id))
            .cloned()
            .collect();

        let workspaces: Vec<Workspace> = self
            .workspaces
            .iter()
            .filter(|w| w.id != id)
            .cloned()
            .collect();

        let active_workspace_id = if self.active_workspace_id == id {
            workspaces[0].id
        } else {
            self.active_workspace_id
        };

        Ok(Self {
            workspaces,
            tabs,
            active_workspace_id,
            settings: self.settings.clone(),
        })
    }

    pub fn with_active_workspace(&self, id: WorkspaceId) -> WisResult<Self> {
        if !self.workspaces.iter().any(|w| w.id == id) {
            return Err(WisError::WorkspaceNotFound { id });
        }

        Ok(Self {
            active_workspace_id: id,
            ..self.clone()
        })
    }

    pub fn with_new_tab(&self, workspace_id: WorkspaceId, url_str: &str) -> WisResult<(Self, Tab)> {
        let url = Url::parse(url_str).map_err(|_| WisError::InvalidUrl {
            url: url_str.to_string(),
        })?;
        self.workspace(workspace_id)?;

        let tab = Tab::new(workspace_id, url);
        let tab_id = tab.id;

        let mut tabs = self.tabs.clone();
        tabs.push(tab.clone());

        let workspaces: Vec<Workspace> = self
            .workspaces
            .iter()
            .map(|w| {
                if w.id == workspace_id {
                    w.with_tab_appended(tab_id)
                } else {
                    w.clone()
                }
            })
            .collect();

        Ok((
            Self {
                workspaces,
                tabs,
                ..self.clone()
            },
            tab,
        ))
    }

    pub fn with_tab_closed(&self, id: TabId) -> WisResult<Self> {
        let tab = self.tab(id)?;
        let workspace_id = tab.workspace_id;

        let tabs = self.tabs.iter().filter(|t| t.id != id).cloned().collect();

        let workspaces: Vec<Workspace> = self
            .workspaces
            .iter()
            .map(|w| {
                if w.id == workspace_id {
                    w.with_tab_removed(id)
                } else {
                    w.clone()
                }
            })
            .collect();

        Ok(Self {
            workspaces,
            tabs,
            ..self.clone()
        })
    }

    pub fn with_tab_order(&self, workspace_id: WorkspaceId, order: Vec<TabId>) -> WisResult<Self> {
        let workspace = self.workspace(workspace_id)?;

        let current: HashSet<TabId> = workspace.tab_order.iter().copied().collect();
        let next: HashSet<TabId> = order.iter().copied().collect();
        if current != next {
            return Err(WisError::InvalidInput {
                message: "new tab order must contain exactly the workspace's existing tabs"
                    .to_string(),
            });
        }

        let workspaces: Vec<Workspace> = self
            .workspaces
            .iter()
            .map(|w| {
                if w.id == workspace_id {
                    w.with_tab_order(order.clone())
                } else {
                    w.clone()
                }
            })
            .collect();

        Ok(Self {
            workspaces,
            ..self.clone()
        })
    }

    pub fn with_tab_pinned(&self, id: TabId, pinned: bool) -> WisResult<(Self, Tab)> {
        let updated_tab = self.tab(id)?.with_pinned(pinned);
        let workspace_id = updated_tab.workspace_id;

        let tabs: Vec<Tab> = self
            .tabs
            .iter()
            .map(|t| {
                if t.id == id {
                    updated_tab.clone()
                } else {
                    t.clone()
                }
            })
            .collect();

        let workspaces: Vec<Workspace> = self
            .workspaces
            .iter()
            .map(|w| {
                if w.id == workspace_id {
                    w.with_pinned(id, pinned)
                } else {
                    w.clone()
                }
            })
            .collect();

        Ok((
            Self {
                workspaces,
                tabs,
                ..self.clone()
            },
            updated_tab,
        ))
    }

    pub fn with_active_tab(&self, id: TabId) -> WisResult<Self> {
        let touched_tab = self.tab(id)?.touched();
        let workspace_id = touched_tab.workspace_id;

        let tabs: Vec<Tab> = self
            .tabs
            .iter()
            .map(|t| {
                if t.id == id {
                    touched_tab.clone()
                } else {
                    t.clone()
                }
            })
            .collect();

        let workspaces: Vec<Workspace> = self
            .workspaces
            .iter()
            .map(|w| {
                if w.id == workspace_id {
                    w.with_active_tab(id)
                } else {
                    w.clone()
                }
            })
            .collect();

        Ok(Self {
            workspaces,
            tabs,
            ..self.clone()
        })
    }

    pub fn with_settings(&self, patch: UserSettingsPatch) -> Self {
        Self {
            settings: self.settings.with_patch(patch),
            ..self.clone()
        }
    }

    /// Check the structural invariant: every TabId mentioned by a workspace
    /// (tab_order, pinned_tab_ids, active_tab_id) must exist in `self.tabs`,
    /// and every Tab's `workspace_id` must point at an existing workspace.
    ///
    /// Used by tests and by the storage layer on hydrate (in future).
    pub fn invariant_violations(&self) -> Vec<String> {
        let mut errors = Vec::new();
        let workspace_ids: HashSet<WorkspaceId> = self.workspaces.iter().map(|w| w.id).collect();
        let tab_ids: HashSet<TabId> = self.tabs.iter().map(|t| t.id).collect();

        if !workspace_ids.contains(&self.active_workspace_id) {
            errors.push(format!(
                "active_workspace_id {} not in workspaces",
                self.active_workspace_id
            ));
        }

        for ws in &self.workspaces {
            for tab_id in &ws.tab_order {
                if !tab_ids.contains(tab_id) {
                    errors.push(format!(
                        "workspace {} references missing tab {}",
                        ws.id, tab_id
                    ));
                }
            }
            for tab_id in &ws.pinned_tab_ids {
                if !ws.tab_order.contains(tab_id) {
                    errors.push(format!(
                        "workspace {} pins tab {} that is not in its tab_order",
                        ws.id, tab_id
                    ));
                }
            }
            if let Some(active) = ws.active_tab_id {
                if !ws.tab_order.contains(&active) {
                    errors.push(format!(
                        "workspace {} active_tab_id {} not in its tab_order",
                        ws.id, active
                    ));
                }
            }
        }

        for tab in &self.tabs {
            if !workspace_ids.contains(&tab.workspace_id) {
                errors.push(format!(
                    "tab {} belongs to missing workspace {}",
                    tab.id, tab.workspace_id
                ));
            }
        }

        errors
    }
}
