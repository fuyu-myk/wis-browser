//! All CRUD against the SQLite schema.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, Transaction};
use url::Url;
use uuid::Uuid;
use wis_core::{
    AccentColor, BrowserState, EssentialPin, FaviconRef, PrivacyMode, Tab, TabFolder, TabFolderId,
    TabId, UserSettings, Workspace, WorkspaceId,
};

use crate::error::{StorageError, StorageResult};
use crate::sqlite::convert::{enum_from_text, enum_to_text};

const META_ACTIVE_WORKSPACE: &str = "active_workspace_id";
const META_WINDOWED_WORKSPACES: &str = "windowed_workspaces";
const SETTINGS_KEY: &str = "user_settings";

// ===========================================================================
// SAVE
// ===========================================================================

pub fn save(tx: &Transaction<'_>, state: &BrowserState) -> StorageResult<()> {
    clear_all(tx)?;

    let persisted: Vec<&Workspace> = state.workspaces.iter().filter(|w| !w.ephemeral).collect();

    for (position, workspace) in persisted.iter().enumerate() {
        insert_workspace(tx, workspace, position as i64)?;

        for pin in &workspace.essentials {
            insert_essential(tx, workspace.id, pin)?;
        }

        for (tab_position, tab_id) in workspace.tab_order.iter().enumerate() {
            let tab = state.tabs.iter().find(|t| t.id == *tab_id).ok_or_else(|| {
                StorageError::DataCorruption(format!(
                    "workspace {} references missing tab {tab_id}",
                    workspace.id
                ))
            })?;
            insert_tab(tx, tab, tab_position as i64)?;
        }
    }

    save_settings(tx, &state.settings)?;

    let persisted_active = persisted
        .iter()
        .any(|w| w.id == state.active_workspace_id)
        .then_some(state.active_workspace_id)
        .or_else(|| persisted.first().map(|w| w.id));
    if let Some(active) = persisted_active {
        upsert_meta(tx, META_ACTIVE_WORKSPACE, &uuid_string(active.into()))?;
    }

    Ok(())
}

fn clear_all(tx: &Transaction<'_>) -> StorageResult<()> {
    for table in ["essentials", "tabs", "workspaces", "settings"] {
        tx.execute(&format!("DELETE FROM {table}"), [])?;
    }

    Ok(())
}

/// Persist the workspaces that had their own window at exit (see the [`Storage`](crate::Storage)
/// trait). Serialised as a JSON array under the `windowed_workspaces` meta key.
pub fn save_windowed_workspaces(conn: &Connection, ids: &[WorkspaceId]) -> StorageResult<()> {
    let json = serde_json::to_string(ids)?;
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        params![META_WINDOWED_WORKSPACES, json],
    )?;

    Ok(())
}

/// Read the workspaces to reopen in their own windows on launch. A missing or
/// unparsable entry yields an empty list (restore just the main window).
pub fn load_windowed_workspaces(conn: &Connection) -> StorageResult<Vec<WorkspaceId>> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = ?1",
            params![META_WINDOWED_WORKSPACES],
            |row| row.get(0),
        )
        .map(Some)
        .or_else(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })?;

    Ok(value
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default())
}

fn insert_workspace(
    tx: &Transaction<'_>,
    workspace: &Workspace,
    position: i64,
) -> StorageResult<()> {
    let pane_tree_json = workspace
        .pane_tree
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let upstream_proxy_json = workspace
        .upstream_proxy
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let tab_folders_json = if workspace.tab_folders.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&workspace.tab_folders)?)
    };
    let background_json = workspace
        .background
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let background_drafts_json = if workspace.background_drafts.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&workspace.background_drafts)?)
    };
    tx.execute(
        "INSERT INTO workspaces (id, position, name, accent, icon, privacy_mode, active_tab_id, pane_tree, upstream_proxy, tab_folders, background, custom_accent, background_drafts)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            uuid_string(workspace.id.into()),
            position,
            workspace.name,
            enum_to_text(&workspace.accent)?,
            workspace.icon,
            enum_to_text(&workspace.privacy_mode)?,
            workspace.active_tab_id.map(|id| uuid_string(id.into())),
            pane_tree_json,
            upstream_proxy_json,
            tab_folders_json,
            background_json,
            workspace.custom_accent,
            background_drafts_json,
        ],
    )?;

    Ok(())
}

fn insert_tab(tx: &Transaction<'_>, tab: &Tab, position: i64) -> StorageResult<()> {
    tx.execute(
        "INSERT INTO tabs (id, workspace_id, position, url, title, favicon_hash,
                           pinned, muted, created_at, last_active_at, zoom, folder_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            uuid_string(tab.id.into()),
            uuid_string(tab.workspace_id.into()),
            position,
            tab.url,
            tab.title,
            tab.favicon.as_ref().map(|f| f.hash.clone()),
            tab.pinned,
            tab.muted,
            tab.created_at,
            tab.last_active_at,
            tab.zoom,
            tab.folder_id.map(|id| uuid_string(id.into())),
        ],
    )?;

    Ok(())
}

fn insert_essential(
    tx: &Transaction<'_>,
    workspace_id: WorkspaceId,
    pin: &EssentialPin,
) -> StorageResult<()> {
    tx.execute(
        "INSERT INTO essentials (workspace_id, slot, label, url, icon)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            uuid_string(workspace_id.into()),
            pin.slot as i64,
            pin.label,
            pin.url,
            pin.icon,
        ],
    )?;

    Ok(())
}

fn save_settings(tx: &Transaction<'_>, settings: &UserSettings) -> StorageResult<()> {
    let json = serde_json::to_string(settings)?;
    tx.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)",
        params![SETTINGS_KEY, json],
    )?;

    Ok(())
}

fn upsert_meta(tx: &Transaction<'_>, key: &str, value: &str) -> StorageResult<()> {
    tx.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;

    Ok(())
}

// ===========================================================================
// LOAD
// ===========================================================================

pub fn load(conn: &Connection) -> StorageResult<Option<BrowserState>> {
    let workspaces = load_workspaces(conn)?;
    if workspaces.is_empty() {
        return Ok(None);
    }

    // Loaded in (workspace_id, position) order
    let mut tabs = load_tabs(conn)?;
    let essentials = load_essentials(conn)?;
    let settings = load_settings(conn)?.unwrap_or_default();
    let active_workspace_id = load_active_workspace(conn)?
        .or_else(|| workspaces.first().map(|w| w.id))
        .ok_or_else(|| {
            StorageError::DataCorruption(
                "no active workspace and no workspaces to fall back to".into(),
            )
        })?;

    let workspaces = assemble_workspaces(workspaces, &tabs, essentials);

    // The BrowserState convention is that `tabs` follows insertion order,
    // which equals `created_at` ASC. Re-sort after assemble has consumed the
    // position-ordered Vec.
    tabs.sort_by_key(|t| t.created_at);

    let state = BrowserState {
        workspaces,
        tabs,
        active_workspace_id,
        settings,
    };

    super::validate(&state)?;
    Ok(Some(state))
}

/// Workspaces without their tab_order / pinned_tab_ids / essentials filled in.
/// Those derive from the `tabs` and `essentials` tables and get attached in
/// [`assemble_workspaces`].
fn load_workspaces(conn: &Connection) -> StorageResult<Vec<Workspace>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, accent, icon, privacy_mode, active_tab_id, pane_tree, upstream_proxy, tab_folders, background, custom_accent, background_drafts
         FROM workspaces ORDER BY position ASC",
    )?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();

    while let Some(row) = rows.next()? {
        let id_str: String = row.get(0)?;
        let active_tab_id_str: Option<String> = row.get(5)?;
        let pane_tree_json: Option<String> = row.get(6)?;
        let pane_tree = pane_tree_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?;
        let upstream_proxy_json: Option<String> = row.get(7)?;
        let upstream_proxy = upstream_proxy_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?;
        let tab_folders_json: Option<String> = row.get(8)?;
        let tab_folders: Vec<TabFolder> = tab_folders_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?
            .unwrap_or_default();
        let background_json: Option<String> = row.get(9)?;
        let background = background_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?;
        let background_drafts_json: Option<String> = row.get(11)?;
        let background_drafts = background_drafts_json
            .as_deref()
            .map(serde_json::from_str)
            .transpose()?
            .unwrap_or_default();

        out.push(Workspace {
            id: WorkspaceId::from(parse_uuid(&id_str)?),
            name: row.get(1)?,
            accent: enum_from_text::<AccentColor>(&row.get::<_, String>(2)?)?,
            icon: row.get(3)?,
            privacy_mode: enum_from_text::<PrivacyMode>(&row.get::<_, String>(4)?)?,
            active_tab_id: active_tab_id_str
                .as_deref()
                .map(parse_uuid)
                .transpose()?
                .map(TabId::from),
            pane_tree,
            upstream_proxy,
            tab_order: Vec::new(),
            pinned_tab_ids: Vec::new(),
            tab_folders,
            selected_folder_id: None,
            essentials: Vec::new(),
            ephemeral: false,
            background,
            custom_accent: row.get(10)?,
            background_drafts,
        });
    }

    Ok(out)
}

fn load_tabs(conn: &Connection) -> StorageResult<Vec<Tab>> {
    let mut stmt = conn.prepare(
        "SELECT id, workspace_id, url, title, favicon_hash, pinned, muted,
                created_at, last_active_at, zoom, folder_id
         FROM tabs
         ORDER BY workspace_id ASC, position ASC",
    )?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();

    while let Some(row) = rows.next()? {
        let id_str: String = row.get(0)?;
        let workspace_id_str: String = row.get(1)?;
        let favicon_hash: Option<String> = row.get(4)?;
        let zoom: Option<f64> = row.get(9)?;
        let folder_id_str: Option<String> = row.get(10)?;

        out.push(Tab {
            id: TabId::from(parse_uuid(&id_str)?),
            workspace_id: WorkspaceId::from(parse_uuid(&workspace_id_str)?),
            url: row.get::<_, Url>(2)?,
            title: row.get(3)?,
            favicon: favicon_hash.map(|hash| FaviconRef { hash }),
            pinned: row.get(5)?,
            folder_id: folder_id_str
                .as_deref()
                .map(parse_uuid)
                .transpose()?
                .map(TabFolderId::from),
            muted: row.get(6)?,
            loading: false,
            zoom: zoom.unwrap_or_else(wis_core::tab::default_zoom),
            created_at: row.get::<_, DateTime<Utc>>(7)?,
            last_active_at: row.get::<_, DateTime<Utc>>(8)?,
        });
    }

    Ok(out)
}

fn load_essentials(conn: &Connection) -> StorageResult<HashMap<WorkspaceId, Vec<EssentialPin>>> {
    let mut stmt = conn.prepare(
        "SELECT workspace_id, slot, label, url, icon FROM essentials
         ORDER BY workspace_id ASC, slot ASC",
    )?;
    let mut rows = stmt.query([])?;
    let mut out: HashMap<WorkspaceId, Vec<EssentialPin>> = HashMap::new();

    while let Some(row) = rows.next()? {
        let workspace_id_str: String = row.get(0)?;
        let workspace_id = WorkspaceId::from(parse_uuid(&workspace_id_str)?);

        out.entry(workspace_id).or_default().push(EssentialPin {
            slot: row.get::<_, i64>(1)? as u8,
            label: row.get(2)?,
            url: row.get(3)?,
            icon: row.get(4)?,
        });
    }

    Ok(out)
}

fn load_settings(conn: &Connection) -> StorageResult<Option<UserSettings>> {
    let json: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![SETTINGS_KEY],
            |row| row.get(0),
        )
        .map(Some)
        .or_else(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })?;

    Ok(json.map(|s| serde_json::from_str(&s)).transpose()?)
}

fn load_active_workspace(conn: &Connection) -> StorageResult<Option<WorkspaceId>> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = ?1",
            params![META_ACTIVE_WORKSPACE],
            |row| row.get(0),
        )
        .map(Some)
        .or_else(|err| match err {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })?;

    value
        .as_deref()
        .map(parse_uuid)
        .transpose()
        .map(|opt| opt.map(WorkspaceId::from))
}

fn assemble_workspaces(
    workspaces: Vec<Workspace>,
    tabs: &[Tab],
    mut essentials: HashMap<WorkspaceId, Vec<EssentialPin>>,
) -> Vec<Workspace> {
    workspaces
        .into_iter()
        .map(|workspace| {
            let id = workspace.id;
            let tab_order: Vec<TabId> = tabs
                .iter()
                .filter(|t| t.workspace_id == id)
                .map(|t| t.id)
                .collect();
            let pinned_tab_ids: Vec<TabId> = tabs
                .iter()
                .filter(|t| t.workspace_id == id && t.pinned)
                .map(|t| t.id)
                .collect();
            Workspace {
                tab_order,
                pinned_tab_ids,
                essentials: essentials.remove(&id).unwrap_or_default(),
                ..workspace
            }
        })
        .collect()
}

// ===========================================================================
// Helpers
// ===========================================================================

fn uuid_string(id: Uuid) -> String {
    id.to_string()
}

fn parse_uuid(s: &str) -> StorageResult<Uuid> {
    Uuid::parse_str(s).map_err(StorageError::from)
}
