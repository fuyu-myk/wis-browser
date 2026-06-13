use super::*;

impl BrowserState {
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
}
