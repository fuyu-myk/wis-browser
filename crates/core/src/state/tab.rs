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
                        appended
                            .with_focused_tab(tab_id)
                            .with_selected_folder(folder)
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

    /// New tabs file into the workspace's selected folder; with no selection
    /// they are loose. Used by Cmd+T, link opens, and bookmark opens.
    pub fn with_new_tab(&self, workspace_id: WorkspaceId, url_str: &str) -> WisResult<(Self, Tab)> {
        let folder = self.workspace(workspace_id)?.selected_folder_id;
        self.with_tab_created(workspace_id, url_str, folder, true)
    }

    /// Like [`Self::with_new_tab`] but the new tab is **not** focused and lands
    /// beside its opener (see [`Self::with_new_related_tab`]). Backs
    /// Cmd/middle-click "open link in a new background tab".
    pub fn with_new_background_tab(
        &self,
        workspace_id: WorkspaceId,
        url_str: &str,
    ) -> WisResult<(Self, Tab)> {
        self.with_new_related_tab(workspace_id, url_str, false)
    }

    /// Open a tab right after the active tab, behind any tabs already opened from
    /// it, so a run of link opens keeps link order. Appends like
    /// [`Self::with_new_tab`] when there is no active tab or the new tab files
    /// into a different folder than the active one.
    pub fn with_new_related_tab(
        &self,
        workspace_id: WorkspaceId,
        url_str: &str,
        focus: bool,
    ) -> WisResult<(Self, Tab)> {
        let folder = self.workspace(workspace_id)?.selected_folder_id;
        let position = self.related_insert_index(workspace_id, folder);
        let (next, tab) = self.with_tab_created(workspace_id, url_str, folder, focus)?;
        let Some(position) = position else {
            return Ok((next, tab));
        };
        let mut order = next.workspace(workspace_id)?.tab_order.clone();

        order.retain(|t| *t != tab.id);
        order.insert(position.min(order.len()), tab.id);

        Ok((next.with_tab_order(workspace_id, order)?, tab))
    }

    fn related_insert_index(
        &self,
        workspace_id: WorkspaceId,
        folder: Option<TabFolderId>,
    ) -> Option<usize> {
        let workspace = self.workspace(workspace_id).ok()?;
        let active = self.tab(workspace.active_tab_id?).ok()?;
        if active.folder_id != folder {
            return None;
        }

        let index = workspace.tab_order.iter().position(|t| *t == active.id)?;
        let opened_since = workspace.tab_order[index + 1..]
            .iter()
            .take_while(|id| {
                self.tab(**id)
                    .map(|t| t.folder_id == folder && t.created_at >= active.last_active_at)
                    .unwrap_or(false)
            })
            .count();

        Some(index + 1 + opened_since)
    }

    /// Append a new tab that is always loose (outside any folder), regardless of
    /// the selected folder, which clears since the loose tab takes focus. Used
    /// by the explicit "+ New tab" affordance and reopen-closed-tab.
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

        let tabs: Vec<Tab> = self.tabs.iter().filter(|t| t.id != id).cloned().collect();

        let workspaces: Vec<Workspace> = self
            .workspaces
            .iter()
            .map(|w| {
                if w.id != workspace_id {
                    return w.clone();
                }

                let next = w.with_tab_removed(id).with_pane_after_removed(id);
                if w.active_tab_id != Some(id) {
                    return next;
                }

                next.with_selected_folder_of_active(&tabs)
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
                if w.id != workspace_id {
                    return w.clone();
                }

                let next = w.with_pinned(id, pinned);
                if w.active_tab_id == Some(id) {
                    next.with_selected_folder_of_active(&tabs)
                } else {
                    next
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

    /// Replace a tab's URL. The title is also bumped to the site name as a
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
            .with_title(crate::tab::placeholder_title(&url))
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

    /// Toggle a tab's `loading` flag. Idempotent when the value doesn't change.
    pub fn with_tab_loading(&self, id: TabId, loading: bool) -> WisResult<Self> {
        let current = self.tab(id)?;
        if current.loading == loading {
            return Ok(self.clone());
        }
        let updated = current.with_loading(loading);

        Ok(self.replace_tab(id, &updated))
    }

    /// Set (or clear) a tab's load failure. Idempotent when unchanged.
    pub fn with_tab_load_error(
        &self,
        id: TabId,
        load_error: Option<crate::load_error::LoadError>,
    ) -> WisResult<Self> {
        let current = self.tab(id)?;
        if current.load_error == load_error {
            return Ok(self.clone());
        }

        Ok(self.replace_tab(id, &current.with_load_error(load_error)))
    }

    /// Update a tab's back/forward history availability. Idempotent when
    /// neither direction changes.
    pub fn with_tab_nav_state(&self, id: TabId, back: bool, forward: bool) -> WisResult<Self> {
        let current = self.tab(id)?;
        if current.can_go_back == back && current.can_go_forward == forward {
            return Ok(self.clone());
        }
        let updated = current.with_nav_state(back, forward);

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
                    let focused = w
                        .with_focused_tab(id)
                        .with_selected_folder(touched_tab.folder_id);
                    match touched_tab.folder_id {
                        Some(folder_id) => focused.with_folder_collapsed(folder_id, false),
                        None => focused,
                    }
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
