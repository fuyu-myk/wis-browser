//! [`BrowserState`]: the root aggregate that holds workspaces, tabs, and
//! settings. All mutation operations are pure: they return a new value rather
//! than mutating self.

use crate::error::{WisError, WisResult};
use crate::ids::{TabId, WorkspaceId};
use crate::pane::{PaneBranch, SplitDir};
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

    /// First-run state: defaults plus a single seeded "New Tab".
    pub fn new_first_run() -> Self {
        let state = Self::new_with_defaults();
        let ws_id = state.active_workspace_id;
        let (state, tab) = state
            .with_new_tab(ws_id, "Enter search or web address")
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

    pub fn with_new_workspace(&self, name: String, accent: AccentColor) -> (Self, Workspace) {
        self.with_appended_workspace(Workspace::new(name, accent))
    }

    /// Append a private (ephemeral) workspace. Its tabs use a non-persistent
    /// data store and it is excluded from disk persistence (see the storage
    /// layer). Returns the new state and the created workspace.
    pub fn with_new_private_workspace(
        &self,
        name: String,
        accent: AccentColor,
    ) -> (Self, Workspace) {
        self.with_appended_workspace(Workspace::new_private(name, accent))
    }

    fn with_appended_workspace(&self, workspace: Workspace) -> (Self, Workspace) {
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

    pub fn with_recoloured_workspace(
        &self,
        id: WorkspaceId,
        accent: crate::workspace::AccentColor,
    ) -> WisResult<Self> {
        if !self.workspaces.iter().any(|w| w.id == id) {
            return Err(WisError::WorkspaceNotFound { id });
        }

        let workspaces = self
            .workspaces
            .iter()
            .map(|w| {
                if w.id == id {
                    w.with_accent(accent)
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

    pub fn with_workspace_privacy_mode(
        &self,
        id: WorkspaceId,
        mode: crate::privacy::PrivacyMode,
    ) -> WisResult<Self> {
        if !self.workspaces.iter().any(|w| w.id == id) {
            return Err(WisError::WorkspaceNotFound { id });
        }

        let workspaces = self
            .workspaces
            .iter()
            .map(|w| {
                if w.id == id {
                    w.with_privacy_mode(mode)
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

    pub fn with_workspace_upstream_proxy(
        &self,
        id: WorkspaceId,
        upstream: Option<crate::proxy::UpstreamProxy>,
    ) -> WisResult<Self> {
        if !self.workspaces.iter().any(|w| w.id == id) {
            return Err(WisError::WorkspaceNotFound { id });
        }

        let workspaces = self
            .workspaces
            .iter()
            .map(|w| {
                if w.id == id {
                    w.with_upstream_proxy(upstream.clone())
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
                    w.with_tab_appended(tab_id).with_focused_tab(tab_id)
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
                    w.with_tab_removed(id).with_pane_after_removed(id)
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

    /// Replace a tab's URL. The title is also bumped to the URL string as a
    /// placeholder: useful when initiating the navigation (URL bar, new
    /// tab) because the real `<title>` arrives via a separate event afterward
    /// and overwrites this placeholder. `last_active_at` is refreshed.
    pub fn with_navigated_tab(&self, id: TabId, url_str: &str) -> WisResult<(Self, Tab)> {
        let url = Url::parse(url_str).map_err(|_| WisError::InvalidUrl {
            url: url_str.to_string(),
        })?;
        let current = self.tab(id)?;
        let updated = current
            .with_url(url.clone())
            .with_title(url.to_string())
            .touched();

        Ok((self.replace_tab(id, &updated), updated))
    }

    /// Sync a tab's URL from a navigation event the webview originated
    /// itself (in-page link, history hop). Unlike [`with_navigated_tab`],
    /// the title is **left alone**: wry's `on_document_title_changed`
    /// is the source of truth for that. Idempotent when the URL doesn't
    /// actually change.
    pub fn with_tab_url_synced(&self, id: TabId, url_str: &str) -> WisResult<Self> {
        let url = Url::parse(url_str).map_err(|_| WisError::InvalidUrl {
            url: url_str.to_string(),
        })?;
        let current = self.tab(id)?;
        if current.url == url {
            return Ok(self.clone());
        }
        let updated = current.with_url(url);

        Ok(self.replace_tab(id, &updated))
    }

    /// Update a tab's title. Idempotent when the title doesn't change.
    pub fn with_tab_title(&self, id: TabId, title: String) -> WisResult<Self> {
        let current = self.tab(id)?;
        if current.title == title {
            return Ok(self.clone());
        }
        let updated = current.with_title(title);

        Ok(self.replace_tab(id, &updated))
    }

    /// Toggle a tab's `loading` flag. Idempotent when the value doesn't change.
    pub fn with_tab_loading(&self, id: TabId, loading: bool) -> WisResult<Self> {
        let current = self.tab(id)?;
        if current.loading == loading {
            return Ok(self.clone());
        }
        let updated = current.with_loading(loading);

        Ok(self.replace_tab(id, &updated))
    }

    /// Update a tab's favicon reference. Idempotent when the value doesn't change.
    pub fn with_tab_favicon(
        &self,
        id: TabId,
        favicon: Option<crate::tab::FaviconRef>,
    ) -> WisResult<Self> {
        let current = self.tab(id)?;
        if current.favicon == favicon {
            return Ok(self.clone());
        }
        let updated = current.with_favicon(favicon);

        Ok(self.replace_tab(id, &updated))
    }

    /// Internal helper: produce a new state with the tab at `id` swapped for
    /// `updated`. Caller must guarantee `id == updated.id`.
    fn replace_tab(&self, id: TabId, updated: &Tab) -> Self {
        let tabs: Vec<Tab> = self
            .tabs
            .iter()
            .map(|t| {
                if t.id == id {
                    updated.clone()
                } else {
                    t.clone()
                }
            })
            .collect();

        Self {
            tabs,
            ..self.clone()
        }
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
                    w.with_focused_tab(id)
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

    /// Split the active pane of `workspace_id` along `dir`, opening `url_str`
    /// in the new pane and focusing it. Returns the new state and the created
    /// tab.
    pub fn with_pane_split(
        &self,
        workspace_id: WorkspaceId,
        dir: SplitDir,
        place_new_first: bool,
        url_str: &str,
    ) -> WisResult<(Self, Tab)> {
        let url = Url::parse(url_str).map_err(|_| WisError::InvalidUrl {
            url: url_str.to_string(),
        })?;
        let workspace = self.workspace(workspace_id)?;
        if workspace.active_tab_id.is_none() {
            return Err(WisError::InvalidInput {
                message: "cannot split a workspace with no active tab".to_string(),
            });
        }

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
                        .with_active_pane_split(dir, tab_id, place_new_first)
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

    /// Replace the `target_leaf` pane's tab with the already-open `dragged` tab
    /// (drag a tab to a pane's centre). The displaced tab is closed only when
    /// it's the throwaway new-tab placeholder (`about:blank`).
    pub fn with_pane_leaf_replaced(
        &self,
        workspace_id: WorkspaceId,
        target_leaf: TabId,
        dragged: TabId,
    ) -> WisResult<Self> {
        let ws = self.workspace(workspace_id)?;
        if !ws.tab_order.contains(&target_leaf) || !ws.tab_order.contains(&dragged) {
            return Err(WisError::InvalidInput {
                message: "replace tabs must belong to the workspace".to_string(),
            });
        }
        if target_leaf == dragged {
            return Ok(self.clone());
        }

        const NEW_TAB_URL: &str = "about:blank";
        let close_placeholder = self
            .tab(target_leaf)
            .map(|t| t.url.as_str() == NEW_TAB_URL)
            .unwrap_or(false);

        let workspaces: Vec<Workspace> = self
            .workspaces
            .iter()
            .map(|w| {
                if w.id == workspace_id {
                    w.with_pane_leaf_swapped(target_leaf, dragged)
                } else {
                    w.clone()
                }
            })
            .collect();

        let next = Self {
            workspaces,
            ..self.clone()
        };

        if close_placeholder {
            next.with_tab_closed(target_leaf)
        } else {
            Ok(next)
        }
    }

    /// Split the `target` pane along `dir` with the already-open `moved` tab
    /// (e.g. dragged from the tab list onto a pane). `place_new_first` puts the
    /// moved tab in the left/top slot.
    pub fn with_pane_split_with(
        &self,
        workspace_id: WorkspaceId,
        target_tab: TabId,
        dir: SplitDir,
        place_new_first: bool,
        moved_tab: TabId,
    ) -> WisResult<Self> {
        let ws = self.workspace(workspace_id)?;
        if !ws.tab_order.contains(&target_tab) || !ws.tab_order.contains(&moved_tab) {
            return Err(WisError::InvalidInput {
                message: "split tabs must belong to the workspace".to_string(),
            });
        }
        let workspaces: Vec<Workspace> = self
            .workspaces
            .iter()
            .map(|w| {
                if w.id == workspace_id {
                    w.with_pane_split_with(target_tab, dir, place_new_first, moved_tab)
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

    /// Collapse all splits in a workspace back to a single pane showing the
    /// focused tab. The other panes' tabs stay open as background tabs.
    pub fn with_panes_unsplit(&self, workspace_id: WorkspaceId) -> WisResult<Self> {
        self.workspace(workspace_id)?;
        let workspaces: Vec<Workspace> = self
            .workspaces
            .iter()
            .map(|w| {
                if w.id == workspace_id {
                    w.with_pane_tree(None)
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

    /// Set the ratio (permille) of the split reached by `path` in a workspace.
    pub fn with_pane_ratio(
        &self,
        workspace_id: WorkspaceId,
        path: &[PaneBranch],
        ratio: u16,
    ) -> WisResult<Self> {
        self.workspace(workspace_id)?;
        let workspaces: Vec<Workspace> = self
            .workspaces
            .iter()
            .map(|w| {
                if w.id == workspace_id {
                    w.with_pane_ratio(path, ratio)
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
        }

        errors
    }
}
