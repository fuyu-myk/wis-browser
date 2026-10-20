use super::*;

impl BrowserState {
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
        let workspace = workspace.with_privacy_mode(self.settings.default_privacy_mode);
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

    /// Set or clear a workspace's custom background. Validated at the
    /// boundary: an invalid gradient/image never enters state.
    pub fn with_workspace_background(
        &self,
        id: WorkspaceId,
        background: Option<crate::background::Background>,
    ) -> WisResult<Self> {
        self.workspace(id)?;
        if let Some(bg) = &background {
            bg.validate()?;
        }
        let workspaces = self.map_workspace(id, |w| w.with_background(background.clone()));

        Ok(Self {
            workspaces,
            ..self.clone()
        })
    }

    /// Set or clear a workspace's custom flat dot colour (validated hex).
    pub fn with_workspace_custom_accent(
        &self,
        id: WorkspaceId,
        color: Option<String>,
    ) -> WisResult<Self> {
        self.workspace(id)?;
        if let Some(c) = &color {
            if !crate::background::is_valid_hex_color(c) {
                return Err(WisError::InvalidInput {
                    message: "custom accent must be #rrggbb hex".to_string(),
                });
            }
        }
        let workspaces = self.map_workspace(id, |w| w.with_custom_accent(color.clone()));

        Ok(Self {
            workspaces,
            ..self.clone()
        })
    }

    /// Activate a custom theme by id, or `None` for the built-in presets.
    /// The id must reference an existing custom theme.
    pub fn with_active_theme(&self, id: Option<crate::ids::ThemeId>) -> WisResult<Self> {
        if let Some(theme_id) = id {
            let exists = self.settings.custom_themes.iter().any(|t| t.id == theme_id);
            if !exists {
                return Err(WisError::InvalidInput {
                    message: format!("unknown theme: {theme_id}"),
                });
            }
        }
        let settings = crate::settings::UserSettings {
            active_theme: id,
            ..self.settings.clone()
        };

        Ok(Self {
            settings,
            ..self.clone()
        })
    }
    /// Set or clear a workspace's icon (validated encoded reference).
    pub fn with_workspace_icon(&self, id: WorkspaceId, icon: Option<String>) -> WisResult<Self> {
        self.workspace(id)?;
        Self::validate_icon(&icon)?;
        let workspaces = self.map_workspace(id, |w| w.with_icon(icon.clone()));

        Ok(Self {
            workspaces,
            ..self.clone()
        })
    }
}
