//! Encrypted profile backup payload: the decrypted content of a `.wisbackup`
//! file. The shell encrypts/decrypts it (wis-secrets, passphrase-derived key)
//! and wraps it in a versioned envelope; this module owns the data shape,
//! boundary validation, and the re-mint discipline so a merge-on-import can
//! never collide with existing data. Every section is optional so an export
//! carries only the categories the user toggled on.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::ids::{TabFolderId, TabId, ThemeId, WorkspaceId};
use crate::pane::PaneNode;
use crate::settings::UserSettings;
use crate::state::BrowserState;
use crate::tab::Tab;
use crate::workspace::Workspace;
use crate::WisResult;

/// Bump when the payload shape changes incompatibly. An import rejects a file
/// whose version is newer than this.
pub const BACKUP_VERSION: u8 = 1;

/// One bookmark captured for backup. `folder` indexes into the owning
/// [`WorkspaceBookmarks::folders`] (or `None` for unfiled), so no storage
/// rowids cross the boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookmarkBackup {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<usize>,
    pub url: String,
    pub title: String,
    #[serde(default)]
    pub pinned: bool,
}

/// A workspace's bookmark folders (ordered names) and bookmarks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceBookmarks {
    pub workspace: WorkspaceId,
    #[serde(default)]
    pub folders: Vec<String>,
    #[serde(default)]
    pub bookmarks: Vec<BookmarkBackup>,
}

/// One history entry (url + title). Visit counts/timestamps are not preserved;
/// re-recording on import dedups by url.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryBackup {
    pub url: String,
    pub title: String,
}

/// A workspace's browsing history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceHistory {
    pub workspace: WorkspaceId,
    #[serde(default)]
    pub entries: Vec<HistoryBackup>,
}

/// The workspace graph (workspaces + their tabs) plus per-workspace bookmarks
/// and history. `bookmarks`/`history` reference workspaces by id, remapped
/// alongside the graph on re-mint.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProfileBackup {
    #[serde(default)]
    pub workspaces: Vec<Workspace>,
    #[serde(default)]
    pub tabs: Vec<Tab>,
    #[serde(default)]
    pub bookmarks: Vec<WorkspaceBookmarks>,
    #[serde(default)]
    pub history: Vec<WorkspaceHistory>,
}

/// One saved login captured for backup. The `password` is **plaintext**: it is
/// safe only because the whole payload is sealed under the user's passphrase
/// (Argon2id + ChaCha20-Poly1305). On import each is re-encrypted under the
/// device's vault master key. `(host, username)` is the identity, so an import
/// upserts rather than duplicates.
#[derive(Clone, Serialize, Deserialize)]
pub struct CredentialBackup {
    pub host: String,
    pub username: String,
    pub password: String,
}

// Hand-written so a Debug print of a backup credential can never leak the
// password into logs or error output.
impl std::fmt::Debug for CredentialBackup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CredentialBackup")
            .field("host", &self.host)
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .finish()
    }
}

/// The decrypted, validated contents of a backup. Each field is `Some` only
/// when that category was included in the export.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BackupPayload {
    /// Full `UserSettings` (layout, shortcuts, new-tab config, custom themes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<UserSettings>,
    /// Workspaces + tabs + bookmarks, and (separately toggled) history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<ProfileBackup>,
    /// Saved passwords (opt-in). Plaintext within the passphrase-sealed payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credentials: Option<Vec<CredentialBackup>>,
}

impl BackupPayload {
    /// True when no category is present (an empty export).
    pub fn is_empty(&self) -> bool {
        self.settings.is_none() && self.profile.is_none() && self.credentials.is_none()
    }

    /// Validate every included section. A backup file is untrusted input, so
    /// this runs before any of it is applied.
    pub fn validate(&self) -> WisResult<()> {
        if let Some(settings) = &self.settings {
            for theme in &settings.custom_themes {
                theme.validate()?;
            }
        }

        Ok(())
    }

    /// Regenerate imported IDs so a merge never collides with what's already on
    /// the device. Custom theme ids and `active_theme` are remapped; the whole
    /// workspace graph (workspace / tab / folder ids, plus the references in
    /// `tab_order`, `pinned_tab_ids`, `active_tab_id`, the pane tree, and
    /// bookmark/history workspace keys) is remapped consistently.
    #[must_use]
    pub fn reminted(mut self) -> Self {
        if let Some(settings) = self.settings.as_mut() {
            let mut remap: HashMap<ThemeId, ThemeId> = HashMap::new();
            for theme in settings.custom_themes.iter_mut() {
                let fresh = ThemeId::new();
                remap.insert(theme.id, fresh);
                theme.id = fresh;
            }

            settings.active_theme = settings
                .active_theme
                .and_then(|old| remap.get(&old).copied());
        }

        if let Some(profile) = self.profile.as_mut() {
            remint_profile(profile);
        }

        self
    }
}

/// Re-mint every id in a profile graph and remap all references to it.
fn remint_profile(profile: &mut ProfileBackup) {
    let mut ws_map: HashMap<WorkspaceId, WorkspaceId> = HashMap::new();
    let mut tab_map: HashMap<TabId, TabId> = HashMap::new();
    let mut folder_map: HashMap<TabFolderId, TabFolderId> = HashMap::new();

    // Each source id is unique, so a plain insert mints exactly one fresh id.
    for ws in &profile.workspaces {
        ws_map.insert(ws.id, WorkspaceId::new());
        for folder in &ws.tab_folders {
            folder_map.insert(folder.id, TabFolderId::new());
        }
    }
    for tab in &profile.tabs {
        tab_map.insert(tab.id, TabId::new());
    }

    let map_tab = |id: &TabId| tab_map.get(id).copied();

    for ws in profile.workspaces.iter_mut() {
        ws.id = ws_map.get(&ws.id).copied().unwrap_or(ws.id);
        ws.tab_order = ws.tab_order.iter().filter_map(map_tab).collect();
        ws.pinned_tab_ids = ws.pinned_tab_ids.iter().filter_map(map_tab).collect();
        ws.active_tab_id = ws.active_tab_id.and_then(|id| map_tab(&id));
        for folder in ws.tab_folders.iter_mut() {
            folder.id = folder_map.get(&folder.id).copied().unwrap_or(folder.id);
        }

        ws.selected_folder_id = None;
        ws.pane_tree = ws.pane_tree.take().map(|node| remap_pane(node, &tab_map));

        ws.ephemeral = false;
    }

    for tab in profile.tabs.iter_mut() {
        tab.id = tab_map.get(&tab.id).copied().unwrap_or(tab.id);
        tab.workspace_id = ws_map
            .get(&tab.workspace_id)
            .copied()
            .unwrap_or(tab.workspace_id);
        tab.folder_id = tab.folder_id.and_then(|id| folder_map.get(&id).copied());

        tab.loading = false;
        tab.reader = false;
    }

    for wb in profile.bookmarks.iter_mut() {
        wb.workspace = ws_map.get(&wb.workspace).copied().unwrap_or(wb.workspace);
    }
    for wh in profile.history.iter_mut() {
        wh.workspace = ws_map.get(&wh.workspace).copied().unwrap_or(wh.workspace);
    }
}

/// Remap every tab id in a pane tree through `tab_map`.
fn remap_pane(node: PaneNode, tab_map: &HashMap<TabId, TabId>) -> PaneNode {
    match node {
        PaneNode::Leaf { tab } => PaneNode::Leaf {
            tab: tab_map.get(&tab).copied().unwrap_or(tab),
        },
        PaneNode::Split {
            dir,
            ratio,
            first,
            second,
        } => PaneNode::Split {
            dir,
            ratio,
            first: Box::new(remap_pane(*first, tab_map)),
            second: Box::new(remap_pane(*second, tab_map)),
        },
    }
}

/// Splice a (already re-minted) profile's workspaces + tabs into `state`,
/// **skipping any workspace whose name already exists**. Returns the new
/// state and the ids of the workspaces actually added, so the caller can
/// attach their bookmarks/history.
#[must_use]
pub fn merge_profile(
    mut state: BrowserState,
    profile: &ProfileBackup,
) -> (BrowserState, Vec<WorkspaceId>) {
    let existing: HashSet<String> = state.workspaces.iter().map(|w| w.name.clone()).collect();
    let mut added = Vec::new();
    for ws in &profile.workspaces {
        if existing.contains(&ws.name) {
            continue;
        }

        let ws_id = ws.id;
        state.workspaces.push(ws.clone());
        for tab in profile.tabs.iter().filter(|t| t.workspace_id == ws_id) {
            state.tabs.push(tab.clone());
        }

        added.push(ws_id);
    }

    (state, added)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{ModeTokens, ThemeTokens};

    fn mode() -> ModeTokens {
        let c = || "#000000".to_string();
        ModeTokens {
            accent: c(),
            bg: c(),
            surface: c(),
            surface_raised: c(),
            chrome_bg: c(),
            content_bg: c(),
            text_primary: c(),
            text_secondary: c(),
            border: c(),
        }
    }

    #[test]
    fn empty_payload_is_empty() {
        assert!(BackupPayload::default().is_empty());
    }

    #[test]
    fn remint_regenerates_theme_ids_and_remaps_active() {
        let mut settings = UserSettings::default();
        let original = ThemeId::new();
        settings.custom_themes = vec![crate::theme::CustomTheme {
            id: original,
            name: "Mine".to_string(),
            tokens: ThemeTokens {
                light: mode(),
                dark: mode(),
            },
        }];
        settings.active_theme = Some(original);

        let payload = BackupPayload {
            settings: Some(settings),
            ..Default::default()
        }
        .reminted();

        let themes = &payload.settings.as_ref().unwrap().custom_themes;
        assert_eq!(themes.len(), 1);
        // The id changed, and active_theme still points at the new theme.
        assert_ne!(themes[0].id, original);
        assert_eq!(
            payload.settings.as_ref().unwrap().active_theme,
            Some(themes[0].id)
        );
    }

    #[test]
    fn remint_profile_remaps_every_reference() {
        // Build a real workspace with two tabs via the state builders, so the
        // graph (tab_order, active_tab_id, workspace_id) is internally valid.
        let state = BrowserState::new_with_defaults();
        let ws_id = state.active_workspace_id;
        let (state, _t1) = state.with_new_tab(ws_id, "https://a.example").unwrap();
        let (state, _t2) = state.with_new_tab(ws_id, "https://b.example").unwrap();

        let old_ws_id = state.workspaces[0].id;
        let old_tab_ids: HashSet<TabId> = state.tabs.iter().map(|t| t.id).collect();

        let payload = BackupPayload {
            profile: Some(ProfileBackup {
                workspaces: vec![state.workspaces[0].clone()],
                tabs: state.tabs.clone(),
                ..Default::default()
            }),
            ..Default::default()
        }
        .reminted();

        let profile = payload.profile.unwrap();
        let ws = &profile.workspaces[0];
        // Workspace and tab ids are all fresh.
        assert_ne!(ws.id, old_ws_id);
        let new_tab_ids: HashSet<TabId> = profile.tabs.iter().map(|t| t.id).collect();
        assert!(
            old_tab_ids.is_disjoint(&new_tab_ids),
            "tab ids must be remapped"
        );
        // Tabs belong to the new workspace; references stay valid.
        assert!(profile.tabs.iter().all(|t| t.workspace_id == ws.id));
        assert_eq!(ws.tab_order.len(), 2);
        assert!(ws.tab_order.iter().all(|id| new_tab_ids.contains(id)));
        if let Some(active) = ws.active_tab_id {
            assert!(new_tab_ids.contains(&active));
        }
    }

    #[test]
    fn merge_profile_skips_same_named_workspace() {
        let state = BrowserState::new_with_defaults();
        let existing_name = state.workspaces[0].name.clone();

        // Same name as the existing workspace -> skipped.
        let mut clash = state.workspaces[0].clone();
        clash.id = WorkspaceId::new();
        clash.name = existing_name;
        let (merged, added) = merge_profile(
            state.clone(),
            &ProfileBackup {
                workspaces: vec![clash],
                ..Default::default()
            },
        );
        assert!(added.is_empty());
        assert_eq!(merged.workspaces.len(), 1);

        // A fresh name is added alongside.
        let mut fresh = state.workspaces[0].clone();
        fresh.id = WorkspaceId::new();
        fresh.name = "Imported".to_string();
        let (merged, added) = merge_profile(
            state,
            &ProfileBackup {
                workspaces: vec![fresh],
                ..Default::default()
            },
        );
        assert_eq!(added.len(), 1);
        assert_eq!(merged.workspaces.len(), 2);
    }
}
