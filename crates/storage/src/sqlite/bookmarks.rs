//! SQLite queries backing the bookmarks store (see [`crate::bookmarks`]).
//! Independent append-style tables, not part of the serialised
//! [`wis_core::BrowserState`]. Scoped per workspace; `folder_id` NULL = unfiled.

use chrono::Utc;
use rusqlite::{params, Connection};
use uuid::Uuid;
use wis_core::WorkspaceId;

use crate::bookmarks::{Bookmark, BookmarkFolder};
use crate::error::{StorageError, StorageResult};

fn parse_workspace_id(s: &str) -> Result<WorkspaceId, StorageError> {
    Uuid::parse_str(s)
        .map(WorkspaceId::from)
        .map_err(StorageError::from)
}

/// Folders in a workspace, ordered by position then creation.
pub fn list_folders(
    conn: &Connection,
    workspace_id: WorkspaceId,
) -> StorageResult<Vec<BookmarkFolder>> {
    let ws = workspace_id.as_uuid().to_string();
    let mut stmt = conn.prepare(
        r#"
        SELECT id, workspace_id, name, position, created_at
        FROM bookmark_folders
        WHERE workspace_id = ?1
        ORDER BY position ASC, id ASC
        "#,
    )?;
    let rows = stmt.query_map(params![ws], |row| {
        let ws_str: String = row.get(1)?;
        let workspace_id = parse_workspace_id(&ws_str).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(err))
        })?;

        Ok(BookmarkFolder {
            id: row.get(0)?,
            workspace_id,
            name: row.get(2)?,
            position: row.get(3)?,
            created_at: row.get(4)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Create a folder, appended after the workspace's existing folders.
pub fn create_folder(
    conn: &Connection,
    workspace_id: WorkspaceId,
    name: &str,
) -> StorageResult<BookmarkFolder> {
    let ws = workspace_id.as_uuid().to_string();
    let now = Utc::now();
    let position: i64 = conn.query_row(
        "SELECT COALESCE(MAX(position), -1) + 1 FROM bookmark_folders WHERE workspace_id = ?1",
        params![ws],
        |row| row.get(0),
    )?;
    conn.execute(
        r#"
        INSERT INTO bookmark_folders (workspace_id, name, position, created_at)
        VALUES (?1, ?2, ?3, ?4)
        "#,
        params![ws, name, position, now],
    )?;

    Ok(BookmarkFolder {
        id: conn.last_insert_rowid(),
        workspace_id,
        name: name.to_string(),
        position,
        created_at: now,
    })
}

/// Reorder folders: set each id's `position` to its index in `ids`.
pub fn reorder_folders(conn: &mut Connection, ids: &[i64]) -> StorageResult<()> {
    let tx = conn.transaction()?;
    for (i, id) in ids.iter().enumerate() {
        tx.execute(
            "UPDATE bookmark_folders SET position = ?2 WHERE id = ?1",
            params![id, i as i64],
        )?;
    }
    tx.commit()?;

    Ok(())
}

/// Rename a folder.
pub fn rename_folder(conn: &Connection, id: i64, name: &str) -> StorageResult<()> {
    conn.execute(
        "UPDATE bookmark_folders SET name = ?2 WHERE id = ?1",
        params![id, name],
    )?;
    Ok(())
}

/// Delete a folder, re-parenting its bookmarks to unfiled (no data loss).
pub fn delete_folder(conn: &Connection, id: i64) -> StorageResult<()> {
    conn.execute(
        "UPDATE bookmarks SET folder_id = NULL WHERE folder_id = ?1",
        params![id],
    )?;
    conn.execute("DELETE FROM bookmark_folders WHERE id = ?1", params![id])?;
    Ok(())
}

/// All bookmarks in a workspace, ordered by folder then position. The UI groups
/// them by `folder_id`.
pub fn list_bookmarks(
    conn: &Connection,
    workspace_id: WorkspaceId,
) -> StorageResult<Vec<Bookmark>> {
    let ws = workspace_id.as_uuid().to_string();
    let mut stmt = conn.prepare(
        r#"
        SELECT id, workspace_id, folder_id, url, title, favicon_hash, position,
               pinned, pin_position, created_at
        FROM bookmarks
        WHERE workspace_id = ?1
        ORDER BY position ASC, id ASC
        "#,
    )?;
    let rows = stmt.query_map(params![ws], |row| {
        let ws_str: String = row.get(1)?;
        let workspace_id = parse_workspace_id(&ws_str).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(err))
        })?;
        let pinned: i64 = row.get(7)?;
        Ok(Bookmark {
            id: row.get(0)?,
            workspace_id,
            folder_id: row.get(2)?,
            url: row.get(3)?,
            title: row.get(4)?,
            favicon_hash: row.get(5)?,
            position: row.get(6)?,
            pinned: pinned != 0,
            pin_position: row.get(8)?,
            created_at: row.get(9)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Add a bookmark (appended within its folder, or appended in the unfiled list).
/// When `pinned`, it is also placed at the front of the pinned row.
pub fn add_bookmark(
    conn: &Connection,
    workspace_id: WorkspaceId,
    folder_id: Option<i64>,
    url: &str,
    title: &str,
    favicon_hash: Option<&str>,
    pinned: bool,
) -> StorageResult<Bookmark> {
    let ws = workspace_id.as_uuid().to_string();
    let now = Utc::now();
    let position: i64 = conn.query_row(
        "SELECT COALESCE(MAX(position), -1) + 1 FROM bookmarks \
         WHERE workspace_id = ?1 AND folder_id IS ?2",
        params![ws, folder_id],
        |row| row.get(0),
    )?;
    // Pinned bookmarks prepend in the pinned row so the newest pins to the front.
    let pin_position: i64 = if pinned {
        conn.query_row(
            "SELECT COALESCE(MIN(pin_position), 0) - 1 FROM bookmarks \
             WHERE workspace_id = ?1 AND pinned = 1",
            params![ws],
            |row| row.get(0),
        )?
    } else {
        0
    };
    conn.execute(
        r#"
        INSERT INTO bookmarks
            (workspace_id, folder_id, url, title, favicon_hash, position, pinned, pin_position, created_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        "#,
        params![ws, folder_id, url, title, favicon_hash, position, pinned as i64, pin_position, now],
    )?;

    Ok(Bookmark {
        id: conn.last_insert_rowid(),
        workspace_id,
        folder_id,
        url: url.to_string(),
        title: title.to_string(),
        favicon_hash: favicon_hash.map(str::to_string),
        position,
        pinned,
        pin_position,
        created_at: now,
    })
}

/// Set the pinned flag (keeping the folder). Pinning places it at the front of
/// the pinned row.
pub fn set_pinned(conn: &Connection, id: i64, pinned: bool) -> StorageResult<()> {
    if pinned {
        let pin_position: i64 = conn.query_row(
            "SELECT COALESCE(MIN(pin_position), 0) - 1 FROM bookmarks \
             WHERE pinned = 1 AND workspace_id = (SELECT workspace_id FROM bookmarks WHERE id = ?1)",
            params![id],
            |row| row.get(0),
        )?;
        conn.execute(
            "UPDATE bookmarks SET pinned = 1, pin_position = ?2 WHERE id = ?1",
            params![id, pin_position],
        )?;
    } else {
        conn.execute("UPDATE bookmarks SET pinned = 0 WHERE id = ?1", params![id])?;
    }

    Ok(())
}

/// Reorder the pinned row: set `pin_position` to the index in `ids` (and ensure
/// each is pinned). Does not touch `folder_id`.
pub fn reorder_pinned(conn: &mut Connection, ids: &[i64]) -> StorageResult<()> {
    let tx = conn.transaction()?;
    for (i, id) in ids.iter().enumerate() {
        tx.execute(
            "UPDATE bookmarks SET pinned = 1, pin_position = ?2 WHERE id = ?1",
            params![id, i as i64],
        )?;
    }
    tx.commit()?;

    Ok(())
}

/// Reorder bookmarks within a group (a folder, or the unfiled list): set each
/// id's `position` to its index in `ids`. The caller passes ids that share a
/// `folder_id`; `folder_id` itself is untouched.
pub fn reorder_bookmarks(conn: &mut Connection, ids: &[i64]) -> StorageResult<()> {
    let tx = conn.transaction()?;
    for (i, id) in ids.iter().enumerate() {
        tx.execute(
            "UPDATE bookmarks SET position = ?2 WHERE id = ?1",
            params![id, i as i64],
        )?;
    }
    tx.commit()?;

    Ok(())
}

/// Move a bookmark to a different folder (or unfiled), appended at the end.
pub fn move_bookmark(conn: &Connection, id: i64, folder_id: Option<i64>) -> StorageResult<()> {
    // Place at the end of the destination folder for this bookmark's workspace.
    let ws: Option<String> = conn
        .query_row(
            "SELECT workspace_id FROM bookmarks WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .ok();
    let Some(ws) = ws else {
        return Ok(());
    };
    let position: i64 = conn.query_row(
        r#"
        SELECT COALESCE(MAX(position), -1) + 1 FROM bookmarks
        WHERE workspace_id = ?1 AND folder_id IS ?2
        "#,
        params![ws, folder_id],
        |row| row.get(0),
    )?;
    conn.execute(
        "UPDATE bookmarks SET folder_id = ?2, position = ?3 WHERE id = ?1",
        params![id, folder_id, position],
    )?;

    Ok(())
}

/// Delete a bookmark. Unknown ids are silently ignored.
pub fn delete_bookmark(conn: &Connection, id: i64) -> StorageResult<()> {
    conn.execute("DELETE FROM bookmarks WHERE id = ?1", params![id])?;
    Ok(())
}
