//! Encrypted profile backup payload: the decrypted content of a `.wisbackup`
//! file. The shell encrypts/decrypts it (wis-secrets, passphrase-derived key)
//! and wraps it in a versioned envelope; this module owns the data shape,
//! boundary validation, and the re-mint discipline so a merge-on-import can
//! never collide with existing data. Every section is optional so an export
//! carries only the categories the user toggled on.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::canvas::{canvas_in_url, canvas_name, canvas_url, check_doc};
use crate::ids::{CanvasId, TabFolderId, TabId, ThemeId, WorkspaceId};
use crate::link_routing::{sanitized_link_routes, LinkRoute};
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

/// One canvas. Its id is re-minted on import, and canvas tabs follow it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanvasBackup {
    pub id: CanvasId,
    pub name: String,
    pub doc: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A workspace's canvases.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceCanvases {
    pub workspace: WorkspaceId,
    #[serde(default)]
    pub canvases: Vec<CanvasBackup>,
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
    #[serde(default)]
    pub canvases: Vec<WorkspaceCanvases>,
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
        if let Some(profile) = &self.profile {
            for canvas in profile.canvases.iter().flat_map(|set| &set.canvases) {
                canvas_name(&canvas.name)?;
                check_doc(&canvas.doc)?;
            }
        }

        Ok(())
    }

    /// Regenerate imported IDs so a merge never collides with what's already on
    /// the device. Custom theme ids, `active_theme` and link rules are remapped; the whole
    /// workspace graph (workspace / tab / folder ids, plus the references in
    /// `tab_order`, `pinned_tab_ids`, `active_tab_id`, the pane tree, and
    /// bookmark/history workspace keys) is remapped consistently.
    #[must_use]
    pub fn reminted(mut self) -> Self {
        let ws_map = self
            .profile
            .as_mut()
            .map(remint_profile)
            .unwrap_or_default();

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

            for route in settings.link_routes.iter_mut() {
                if let Some(fresh) = ws_map.get(&route.workspace_id) {
                    route.workspace_id = *fresh;
                }
            }
        }

        self
    }
}

/// Imported link rules pointed at the live workspace standing for each
/// backup workspace: itself when it was added, the lasting one of the same
/// name when [`merge_profile`] skipped it. Rules left without a lasting
/// workspace here are dropped.
#[must_use]
pub fn link_routes_onto(
    routes: Vec<LinkRoute>,
    imported: &[Workspace],
    live: &[Workspace],
) -> Vec<LinkRoute> {
    let landed = routes
        .into_iter()
        .map(|route| {
            if live.iter().any(|w| w.id == route.workspace_id) {
                return route;
            }
            let namesake = imported
                .iter()
                .find(|w| w.id == route.workspace_id)
                .and_then(|skipped| live.iter().find(|w| w.name == skipped.name && !w.ephemeral));
            LinkRoute {
                workspace_id: namesake.map_or(route.workspace_id, |w| w.id),
                ..route
            }
        })
        .collect();
    sanitized_link_routes(landed, live)
}

/// Re-mint every id in a profile graph and remap all references to it,
/// returning the workspace id map.
fn remint_profile(profile: &mut ProfileBackup) -> HashMap<WorkspaceId, WorkspaceId> {
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
    for ws in profile.workspaces.iter_mut() {
        *ws = ws.with_selected_folder_of_active(&profile.tabs);
    }

    for wb in profile.bookmarks.iter_mut() {
        wb.workspace = ws_map.get(&wb.workspace).copied().unwrap_or(wb.workspace);
    }
    for wh in profile.history.iter_mut() {
        wh.workspace = ws_map.get(&wh.workspace).copied().unwrap_or(wh.workspace);
    }

    let mut canvas_map: HashMap<CanvasId, CanvasId> = HashMap::new();
    for set in profile.canvases.iter_mut() {
        set.workspace = ws_map.get(&set.workspace).copied().unwrap_or(set.workspace);
        for canvas in set.canvases.iter_mut() {
            let fresh = CanvasId::new();
            canvas_map.insert(canvas.id, fresh);
            canvas.id = fresh;
        }
    }
    for tab in profile.tabs.iter_mut() {
        if let Some(fresh) = canvas_in_url(&tab.url).and_then(|old| canvas_map.get(&old)) {
            tab.url = canvas_url(*fresh);
        }
    }

    ws_map
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
            state.tabs.push(tab.touched());
        }

        added.push(ws_id);
    }

    (state, added)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{ModeTokens, ThemeTokens};
    use crate::workspace::AccentColor;

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
    fn remint_gives_canvases_fresh_ids_and_points_their_tabs_at_them() {
        let state = BrowserState::new_with_defaults();
        let ws_id = state.active_workspace_id;
        let old_canvas = CanvasId::new();
        let (state, _) = state
            .with_new_tab(
                ws_id,
                &format!("wis-canvas://localhost/?canvas={}", old_canvas.as_uuid()),
            )
            .unwrap();

        let profile = BackupPayload {
            profile: Some(ProfileBackup {
                workspaces: vec![state.workspaces[0].clone()],
                tabs: state.tabs.clone(),
                canvases: vec![WorkspaceCanvases {
                    workspace: ws_id,
                    canvases: vec![CanvasBackup {
                        id: old_canvas,
                        name: "Plans".into(),
                        doc: "{}".into(),
                        created_at: 1,
                        updated_at: 2,
                    }],
                }],
                ..Default::default()
            }),
            ..Default::default()
        }
        .reminted()
        .profile
        .unwrap();

        let ws = &profile.workspaces[0];
        let set = &profile.canvases[0];
        let canvas = &set.canvases[0];
        assert_eq!(set.workspace, ws.id);
        assert_ne!(canvas.id, old_canvas);
        let tab = profile
            .tabs
            .iter()
            .find(|t| t.url.scheme() == "wis-canvas")
            .unwrap();
        assert_eq!(
            tab.url.as_str(),
            format!("wis-canvas://localhost/?canvas={}", canvas.id.as_uuid())
        );
    }

    #[test]
    fn backups_with_unreadable_canvases_are_refused() {
        let canvas = |name: &str, doc: &str| CanvasBackup {
            id: CanvasId::new(),
            name: name.into(),
            doc: doc.into(),
            created_at: 1,
            updated_at: 1,
        };
        let payload = |c: CanvasBackup| BackupPayload {
            profile: Some(ProfileBackup {
                canvases: vec![WorkspaceCanvases {
                    workspace: WorkspaceId::new(),
                    canvases: vec![c],
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(payload(canvas("Plans", "{}")).validate().is_ok());
        assert!(payload(canvas("Plans", "[]")).validate().is_err());
        assert!(payload(canvas("  ", "{}")).validate().is_err());
    }

    #[test]
    fn remint_profile_derives_selected_folder_from_active_tab() {
        let state = BrowserState::new_with_defaults();
        let ws_id = state.active_workspace_id;
        let (state, a) = state.with_new_tab(ws_id, "https://a.example").unwrap();
        let (state, folder) = state
            .with_new_tab_folder(ws_id, "Work".into(), crate::workspace::AccentColor::Mint)
            .unwrap();
        let state = state
            .with_tab_assigned_folder(a.id, Some(folder.id))
            .unwrap()
            .with_selected_tab_folder(ws_id, None)
            .unwrap();

        let payload = BackupPayload {
            profile: Some(ProfileBackup {
                workspaces: vec![state.workspaces[0].clone()],
                tabs: state.tabs.clone(),
                ..Default::default()
            }),
            ..Default::default()
        }
        .reminted();

        let ws = &payload.profile.unwrap().workspaces[0];
        assert_ne!(ws.tab_folders[0].id, folder.id);
        assert_eq!(ws.selected_folder_id, Some(ws.tab_folders[0].id));
    }

    fn route(pattern: &str, workspace_id: WorkspaceId) -> LinkRoute {
        LinkRoute {
            pattern: pattern.to_string(),
            workspace_id,
        }
    }

    #[test]
    fn remint_points_link_routes_at_the_fresh_workspaces() {
        let state = BrowserState::new_with_defaults();
        let old = state.workspaces[0].id;
        let elsewhere = WorkspaceId::new();
        let settings = UserSettings {
            link_routes: vec![route("figma.com", old), route("github.com", elsewhere)],
            ..Default::default()
        };

        let payload = BackupPayload {
            settings: Some(settings),
            profile: Some(ProfileBackup {
                workspaces: state.workspaces.clone(),
                ..Default::default()
            }),
            ..Default::default()
        }
        .reminted();

        let fresh = payload.profile.unwrap().workspaces[0].id;
        assert_eq!(
            payload.settings.unwrap().link_routes,
            vec![route("figma.com", fresh), route("github.com", elsewhere)]
        );
    }

    #[test]
    fn imported_link_routes_land_on_a_lasting_live_workspace_or_are_dropped() {
        let live = BrowserState::new_with_defaults();
        let (live, _) = live.with_new_private_workspace("Secret".into(), AccentColor::Primary);
        let mut namesake = live.workspaces[0].clone();
        namesake.id = WorkspaceId::new();
        let mut secret = namesake.clone();
        secret.id = WorkspaceId::new();
        secret.name = "Secret".to_string();
        let mut added = namesake.clone();
        added.id = WorkspaceId::new();
        added.name = "Imported".to_string();
        let imported = vec![namesake.clone(), secret.clone(), added.clone()];
        let (live, _) = merge_profile(
            live,
            &ProfileBackup {
                workspaces: imported.clone(),
                ..Default::default()
            },
        );

        let routes = link_routes_onto(
            vec![
                route("figma.com", namesake.id),
                route("github.com", added.id),
                route("secret.com", secret.id),
                route("gone.com", WorkspaceId::new()),
            ],
            &imported,
            &live.workspaces,
        );

        assert_eq!(
            routes,
            vec![
                route("figma.com", live.workspaces[0].id),
                route("github.com", added.id)
            ]
        );
    }

    #[test]
    fn a_settings_only_restore_keeps_only_rules_for_workspaces_here() {
        let live = BrowserState::new_with_defaults();
        let here = live.workspaces[0].id;

        let routes = link_routes_onto(
            vec![
                route("figma.com", WorkspaceId::new()),
                route("github.com", here),
            ],
            &[],
            &live.workspaces,
        );

        assert_eq!(routes, vec![route("github.com", here)]);
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

    #[test]
    fn merging_counts_as_using_the_imported_tabs() {
        let state = BrowserState::new_with_defaults();
        let mut workspace = state.workspaces[0].clone();
        workspace.id = WorkspaceId::new();
        workspace.name = "Imported".to_string();
        let long_ago = chrono::Utc::now() - chrono::Duration::days(30);
        let tab = crate::tab::Tab {
            workspace_id: workspace.id,
            last_active_at: long_ago,
            ..crate::tab::Tab::new(workspace.id, "https://saved.test/".parse().unwrap())
        };

        let (merged, _) = merge_profile(
            state,
            &ProfileBackup {
                workspaces: vec![workspace],
                tabs: vec![tab.clone()],
                ..Default::default()
            },
        );
        assert!(merged.tab(tab.id).unwrap().last_active_at > long_ago);
    }
}
