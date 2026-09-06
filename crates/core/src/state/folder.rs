use super::*;

impl BrowserState {
    /// Set or clear a folder's icon (validated encoded reference).
    pub fn with_tab_folder_icon(
        &self,
        workspace_id: WorkspaceId,
        folder_id: TabFolderId,
        icon: Option<String>,
    ) -> WisResult<Self> {
        self.require_folder(workspace_id, folder_id)?;
        Self::validate_icon(&icon)?;
        let workspaces = self.map_workspace(workspace_id, |w| {
            w.with_folder_icon(folder_id, icon.clone())
        });

        Ok(Self {
            workspaces,
            ..self.clone()
        })
    }

    fn require_folder(&self, workspace_id: WorkspaceId, folder_id: TabFolderId) -> WisResult<()> {
        if self.workspace(workspace_id)?.has_folder(folder_id) {
            Ok(())
        } else {
            Err(WisError::InvalidInput {
                message: format!("folder {folder_id} is not in workspace {workspace_id}"),
            })
        }
    }

    /// Create a folder at the end of `workspace_id`'s folder list.
    pub fn with_new_tab_folder(
        &self,
        workspace_id: WorkspaceId,
        name: String,
        accent: AccentColor,
    ) -> WisResult<(Self, TabFolder)> {
        let workspace = self.workspace(workspace_id)?;
        let folder = TabFolder::new(name, accent, workspace.next_folder_position());
        let workspaces =
            self.map_workspace(workspace_id, |w| w.with_tab_folder_appended(folder.clone()));

        Ok((
            Self {
                workspaces,
                ..self.clone()
            },
            folder,
        ))
    }

    pub fn with_tab_folder_renamed(
        &self,
        workspace_id: WorkspaceId,
        folder_id: TabFolderId,
        name: String,
    ) -> WisResult<Self> {
        self.require_folder(workspace_id, folder_id)?;
        let workspaces = self.map_workspace(workspace_id, |w| {
            w.with_folder_renamed(folder_id, name.clone())
        });

        Ok(Self {
            workspaces,
            ..self.clone()
        })
    }

    pub fn with_tab_folder_recolored(
        &self,
        workspace_id: WorkspaceId,
        folder_id: TabFolderId,
        accent: AccentColor,
    ) -> WisResult<Self> {
        self.require_folder(workspace_id, folder_id)?;
        let workspaces =
            self.map_workspace(workspace_id, |w| w.with_folder_recolored(folder_id, accent));

        Ok(Self {
            workspaces,
            ..self.clone()
        })
    }

    pub fn with_tab_folder_collapsed(
        &self,
        workspace_id: WorkspaceId,
        folder_id: TabFolderId,
        collapsed: bool,
    ) -> WisResult<Self> {
        self.require_folder(workspace_id, folder_id)?;
        let workspaces = self.map_workspace(workspace_id, |w| {
            w.with_folder_collapsed(folder_id, collapsed)
        });

        Ok(Self {
            workspaces,
            ..self.clone()
        })
    }

    /// Delete a folder, leaving its member tabs open but loose (no dangling
    /// membership).
    pub fn with_tab_folder_removed(
        &self,
        workspace_id: WorkspaceId,
        folder_id: TabFolderId,
    ) -> WisResult<Self> {
        self.require_folder(workspace_id, folder_id)?;

        let tabs = self
            .tabs
            .iter()
            .map(|t| {
                if t.workspace_id == workspace_id && t.folder_id == Some(folder_id) {
                    t.with_folder(None)
                } else {
                    t.clone()
                }
            })
            .collect();
        let workspaces = self.map_workspace(workspace_id, |w| w.with_folder_removed(folder_id));

        Ok(Self {
            workspaces,
            tabs,
            ..self.clone()
        })
    }

    /// Set (or clear, with `None`) the folder new tabs file into for a workspace.
    pub fn with_selected_tab_folder(
        &self,
        workspace_id: WorkspaceId,
        folder_id: Option<TabFolderId>,
    ) -> WisResult<Self> {
        if let Some(fid) = folder_id {
            self.require_folder(workspace_id, fid)?;
        } else {
            self.workspace(workspace_id)?;
        }
        let workspaces = self.map_workspace(workspace_id, |w| w.with_selected_folder(folder_id));

        Ok(Self {
            workspaces,
            ..self.clone()
        })
    }

    pub fn with_tab_folders_reordered(
        &self,
        workspace_id: WorkspaceId,
        order: Vec<TabFolderId>,
    ) -> WisResult<Self> {
        self.workspace(workspace_id)?;
        let workspaces = self.map_workspace(workspace_id, |w| w.with_tab_folders_reordered(&order));

        Ok(Self {
            workspaces,
            ..self.clone()
        })
    }

    /// Move a tab into `folder_id` (or out of any folder when `None`). Assigning
    /// to a folder unpins the tab, since pinned tabs are always loose. Filing
    /// the active tab moves the selection with it.
    pub fn with_tab_assigned_folder(
        &self,
        tab_id: TabId,
        folder_id: Option<TabFolderId>,
    ) -> WisResult<Self> {
        let tab = self.tab(tab_id)?;
        let workspace_id = tab.workspace_id;
        if let Some(fid) = folder_id {
            self.require_folder(workspace_id, fid)?;
        }

        let unpin = folder_id.is_some() && tab.pinned;
        let updated = {
            let next = tab.with_folder(folder_id);
            if unpin {
                next.with_pinned(false)
            } else {
                next
            }
        };

        let tabs = self
            .tabs
            .iter()
            .map(|t| {
                if t.id == tab_id {
                    updated.clone()
                } else {
                    t.clone()
                }
            })
            .collect();
        let workspaces = self.map_workspace(workspace_id, |w| {
            let next = if unpin {
                w.with_pinned(tab_id, false)
            } else {
                w.clone()
            };
            if next.active_tab_id == Some(tab_id) {
                next.with_selected_folder(folder_id)
            } else {
                next
            }
        });

        Ok(Self {
            workspaces,
            tabs,
            ..self.clone()
        })
    }
}
