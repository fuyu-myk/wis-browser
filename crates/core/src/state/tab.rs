use super::*;

impl BrowserState {
    /// Append a focused new tab in `folder` (None = loose). Shared by
    /// [`with_new_tab`](Self::with_new_tab) and [`with_new_loose_tab`](Self::with_new_loose_tab).
    fn with_tab_created(
        &self,
        workspace_id: WorkspaceId,
        url_str: &str,
        folder: Option<TabFolderId>,
        focus: bool,
    ) -> WisResult<(Self, Tab)> {
        let url = Url::parse(url_str).map_err(|_| WisError::InvalidUrl {
            url: url_str.to_string(),
        })?;
        self.workspace(workspace_id)?;

        let tab = Tab::new(workspace_id, url).with_folder(folder);
        let tab_id = tab.id;

        let mut tabs = self.tabs.clone();
        tabs.push(tab.clone());

        let workspaces: Vec<Workspace> = self
            .workspaces
            .iter()
            .map(|w| {
                if w.id == workspace_id {
                    let appended = w.with_tab_appended(tab_id);
                    // A background tab is appended but does not steal focus.
                    if focus {
                        appended.with_focused_tab(tab_id)
                    } else {
                        appended
                    }
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

    /// Folder a new tab in `workspace_id` should inherit: the workspace's
    /// selected folder, else the active tab's folder (so it lands beside its
    /// opener), else loose.
    fn inherited_folder(&self, workspace_id: WorkspaceId) -> Option<TabFolderId> {
        self.workspace(workspace_id).ok().and_then(|workspace| {
            workspace.selected_folder_id.or_else(|| {
                workspace
                    .active_tab_id
                    .and_then(|aid| self.tab(aid).ok())
                    .and_then(|t| t.folder_id)
            })
        })
    }

    /// New tabs file into the workspace's selected folder if one is set;
    /// otherwise they inherit the active tab's folder, landing beside their
    /// opener. Used by Cmd+T, link opens, and bookmark opens.
    pub fn with_new_tab(&self, workspace_id: WorkspaceId, url_str: &str) -> WisResult<(Self, Tab)> {
        let folder = self.inherited_folder(workspace_id);
        self.with_tab_created(workspace_id, url_str, folder, true)
    }

    /// Like [`Self::with_new_tab`] but the new tab is **not** focused: the opener stays
    /// active. Backs Cmd/middle-click "open link in a new background tab".
    pub fn with_new_background_tab(
        &self,
        workspace_id: WorkspaceId,
        url_str: &str,
    ) -> WisResult<(Self, Tab)> {
        let folder = self.inherited_folder(workspace_id);
        self.with_tab_created(workspace_id, url_str, folder, false)
    }

    /// Append a new tab that is always loose (outside any folder), regardless of
    /// the selected folder. Used by the explicit "+ New tab" affordance.
    pub fn with_new_loose_tab(
        &self,
        workspace_id: WorkspaceId,
        url_str: &str,
    ) -> WisResult<(Self, Tab)> {
        self.with_tab_created(workspace_id, url_str, None, true)
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
        // Pinned tabs are always loose: pinning removes folder membership.
        let updated_tab = if pinned {
            self.tab(id)?.with_pinned(true).with_folder(None)
        } else {
            self.tab(id)?.with_pinned(false)
        };
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

    // -- tab folders --------------------------------------------------------

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
    /// itself (in-page link, history hop). Unlike [`with_navigated_tab`](Self::with_navigated_tab),
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

    pub fn with_tab_zoom(&self, id: TabId, zoom: f64) -> WisResult<Self> {
        let current = self.tab(id)?;
        if current.zoom == zoom {
            return Ok(self.clone());
        }

        Ok(self.replace_tab(id, &current.with_zoom(zoom)))
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

    /// Toggle a tab's `reader` flag. Idempotent when the value doesn't change.
    pub fn with_tab_reader(&self, id: TabId, reader: bool) -> WisResult<Self> {
        let current = self.tab(id)?;
        if current.reader == reader {
            return Ok(self.clone());
        }

        let updated = current.with_reader(reader);

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
}
