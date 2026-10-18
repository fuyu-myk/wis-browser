//! [`BrowserState`]: the root aggregate that holds workspaces, tabs, and
//! settings. All mutation operations are pure: they return a new value rather
//! than mutating self.
//!
//! The `with_*` transitions are split by domain into submodules (`workspace`,
//! `tab`, `folder`, `pane`), each an `impl BrowserState` block. This module
//! owns the struct, constructors, accessors, the shared helpers those blocks
//! reuse (`map_workspace`, `validate_icon`), settings, and the invariant check.

pub(crate) use crate::error::{WisError, WisResult};
pub(crate) use crate::ids::{TabFolderId, TabId, WorkspaceId};
pub(crate) use crate::pane::{PaneBranch, SplitDir};
pub(crate) use crate::settings::{UserSettings, UserSettingsPatch};
pub(crate) use crate::tab::Tab;
pub(crate) use crate::tab_folder::TabFolder;
pub(crate) use crate::workspace::{AccentColor, Workspace};
pub(crate) use serde::{Deserialize, Serialize};
pub(crate) use std::collections::HashSet;
pub(crate) use url::Url;

mod folder;
mod pane;
mod tab;
mod workspace;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

    /// First-run state: defaults plus a single seeded "New Tab".
    pub fn new_first_run() -> Self {
        let state = Self::new_with_defaults();
        let ws_id = state.active_workspace_id;
        let (state, tab) = state
            .with_new_tab(ws_id, "about:blank")
            .expect("seed new tab into fresh defaults");
        let tabs = state
            .tabs
            .iter()
            .map(|t| {
                if t.id == tab.id {
                    t.with_title("New Tab".to_string())
                } else {
                    t.clone()
                }
            })
            .collect();

        Self { tabs, ..state }
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

    /// Map the single workspace `id` through `f`, cloning the rest.
    fn map_workspace(
        &self,
        id: WorkspaceId,
        f: impl Fn(&Workspace) -> Workspace,
    ) -> Vec<Workspace> {
        self.workspaces
            .iter()
            .map(|w| if w.id == id { f(w) } else { w.clone() })
            .collect()
    }

    fn validate_icon(icon: &Option<String>) -> WisResult<()> {
        if let Some(raw) = icon {
            crate::icon::parse_icon_ref(raw)?;
        }

        Ok(())
    }

    pub fn with_settings(&self, patch: UserSettingsPatch) -> Self {
        Self {
            settings: self.settings.with_patch(patch),
            ..self.clone()
        }
    }

    pub fn with_extensions(&self, extensions: Vec<crate::extensions::InstalledExtension>) -> Self {
        Self {
            settings: UserSettings {
                extensions,
                ..self.settings.clone()
            },
            ..self.clone()
        }
    }

    /// Check the structural invariant: every TabId mentioned by a workspace
    /// (tab_order, pinned_tab_ids, active_tab_id) must exist in `self.tabs`,
    /// and every Tab's `workspace_id` must point at an existing workspace.
    ///
    /// Used by tests and by the storage layer on hydrate.
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
            if let Some(selected) = ws.selected_folder_id {
                if !ws.has_folder(selected) {
                    errors.push(format!(
                        "workspace {} selected_folder_id {} is not one of its folders",
                        ws.id, selected
                    ));
                }
            }
            if let Some(tree) = &ws.pane_tree {
                for leaf in tree.leaves() {
                    if !ws.tab_order.contains(&leaf) {
                        errors.push(format!(
                            "workspace {} pane leaf {} not in its tab_order",
                            ws.id, leaf
                        ));
                    }
                }
                if let Some(active) = ws.active_tab_id {
                    if !tree.contains(active) {
                        errors.push(format!(
                            "workspace {} active_tab_id {} is not a pane leaf",
                            ws.id, active
                        ));
                    }
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
            if let Some(folder_id) = tab.folder_id {
                let in_workspace = self
                    .workspaces
                    .iter()
                    .find(|w| w.id == tab.workspace_id)
                    .map(|w| w.has_folder(folder_id))
                    .unwrap_or(false);
                if !in_workspace {
                    errors.push(format!(
                        "tab {} references missing folder {} in workspace {}",
                        tab.id, folder_id, tab.workspace_id
                    ));
                }
                if tab.pinned {
                    errors.push(format!(
                        "tab {} is pinned but also assigned to folder {}",
                        tab.id, folder_id
                    ));
                }
            }
        }

        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_change_only_through_their_own_transition() {
        let state = BrowserState::new_first_run();
        let installed = vec![crate::extensions::InstalledExtension {
            id: "0123456789abcdef0123456789abcdef".into(),
            name: "Dark Reader".into(),
            version: "4.9".into(),
            ..Default::default()
        }];
        let next = state.with_extensions(installed.clone());
        assert_eq!(next.settings.extensions, installed);
        assert_eq!(
            next.with_settings(UserSettingsPatch::default())
                .settings
                .extensions,
            installed
        );
    }

    #[test]
    fn first_run_seeds_one_valid_blank_tab() {
        // Regression: `new_first_run` must seed a parseable URL. A bad seed
        // (e.g. a placeholder string) panics here, and at startup that panic
        // fires inside a non-unwinding AppKit callback and aborts the app.
        let state = BrowserState::new_first_run();
        assert_eq!(state.tabs.len(), 1, "first run seeds exactly one tab");
        let tab = &state.tabs[0];
        assert_eq!(tab.url.as_str(), "about:blank");
        assert_eq!(tab.title, "New Tab");
        assert!(
            state.active_workspace().tab_order.contains(&tab.id),
            "seeded tab is in the active workspace's order",
        );
    }
}
