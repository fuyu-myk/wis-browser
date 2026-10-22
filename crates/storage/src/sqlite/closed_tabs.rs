//! SQLite queries backing the recently-closed-tab store (see
//! [`crate::closed_tabs`]).

use chrono::{DateTime, Utc};
use rusqlite::{params, params_from_iter, Connection, Row};
use uuid::Uuid;
use wis_core::{TabFolderId, WorkspaceId};

use crate::closed_tabs::ClosedTabRecord;
use crate::error::{StorageError, StorageResult};

/// Insert a closed tab and return its new row id.
pub fn record(conn: &Connection, entry: &ClosedTabRecord) -> StorageResult<i64> {
    conn.execute(
        r#"
        INSERT INTO closed_tabs
            (workspace_id, url, title, folder_id, position, pinned, closed_at, archived)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        "#,
        params![
            entry.workspace_id.as_uuid().to_string(),
            entry.url,
            entry.title,
            entry.folder_id.map(|id| id.as_uuid().to_string()),
            entry.position,
            entry.pinned,
            entry.closed_at,
            entry.archived,
        ],
    )?;

    Ok(conn.last_insert_rowid())
}

/// In one transaction.
pub fn record_all(conn: &Connection, entries: &[ClosedTabRecord]) -> StorageResult<Vec<i64>> {
    let tx = conn.unchecked_transaction()?;
    let ids = entries
        .iter()
        .map(|entry| record(&tx, entry))
        .collect::<StorageResult<Vec<i64>>>()?;
    tx.commit()?;
    Ok(ids)
}

/// Closed tabs, newest first, capped at `limit`.
pub fn list(conn: &Connection, limit: u32) -> StorageResult<Vec<ClosedTabRecord>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, workspace_id, url, title, folder_id, position, pinned, closed_at, archived
        FROM closed_tabs
        ORDER BY closed_at DESC, id DESC
        LIMIT ?1
        "#,
    )?;
    let rows = stmt.query_map(params![limit], read_row)?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }

    Ok(out)
}

/// Remove one closed tab and return it, or `None` when it is already gone.
pub fn take(conn: &Connection, id: i64) -> StorageResult<Option<ClosedTabRecord>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, workspace_id, url, title, folder_id, position, pinned, closed_at, archived
        FROM closed_tabs
        WHERE id = ?1
        "#,
    )?;
    let mut rows = stmt.query_map(params![id], read_row)?;
    let found = rows.next().transpose()?;
    drop(rows);
    drop(stmt);

    if found.is_some() {
        conn.execute("DELETE FROM closed_tabs WHERE id = ?1", params![id])?;
    }

    Ok(found)
}

/// Remove the most recently closed tab and return it.
pub fn take_latest(conn: &Connection) -> StorageResult<Option<ClosedTabRecord>> {
    let newest = list(conn, 1)?.into_iter().next();
    match newest {
        Some(entry) => take(conn, entry.id),
        None => Ok(None),
    }
}

/// Drop all but the last `keep` entries recorded. Not the newest closed: an
/// archived tab carries when it was last used.
pub fn prune(conn: &Connection, keep: u32) -> StorageResult<()> {
    conn.execute(
        r#"
        DELETE FROM closed_tabs
        WHERE id NOT IN (
            SELECT id FROM closed_tabs ORDER BY id DESC LIMIT ?1
        )
        "#,
        params![keep],
    )?;

    Ok(())
}

/// Remove closed tabs whose URL is one of `urls`.
pub fn delete_by_urls(conn: &Connection, urls: &[String]) -> StorageResult<()> {
    if urls.is_empty() {
        return Ok(());
    }
    let placeholders = std::iter::repeat_n("?", urls.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!("DELETE FROM closed_tabs WHERE url IN ({placeholders})");
    conn.execute(&sql, params_from_iter(urls.iter()))?;

    Ok(())
}

/// Remove closed tabs, scoped to `workspace_id` when set and to tabs closed at
/// or after `since` when set. Mirrors `history::clear`.
pub fn clear(
    conn: &Connection,
    workspace_id: Option<WorkspaceId>,
    since: Option<DateTime<Utc>>,
) -> StorageResult<()> {
    match (workspace_id, since) {
        (Some(id), Some(since)) => {
            conn.execute(
                "DELETE FROM closed_tabs \
                 WHERE workspace_id = ?1 AND datetime(closed_at) >= datetime(?2)",
                params![id.as_uuid().to_string(), since],
            )?;
        }
        (Some(id), None) => {
            conn.execute(
                "DELETE FROM closed_tabs WHERE workspace_id = ?1",
                params![id.as_uuid().to_string()],
            )?;
        }
        (None, Some(since)) => {
            conn.execute(
                "DELETE FROM closed_tabs WHERE datetime(closed_at) >= datetime(?1)",
                params![since],
            )?;
        }
        (None, None) => {
            conn.execute("DELETE FROM closed_tabs", [])?;
        }
    }

    Ok(())
}

fn read_row(row: &Row<'_>) -> rusqlite::Result<ClosedTabRecord> {
    let workspace_str: String = row.get(1)?;
    let folder_str: Option<String> = row.get(4)?;

    let workspace_id = parse_uuid(&workspace_str).map_err(to_sql_error)?;
    let folder_id = folder_str
        .as_deref()
        .map(parse_uuid)
        .transpose()
        .map_err(to_sql_error)?;

    Ok(ClosedTabRecord {
        id: row.get(0)?,
        workspace_id: WorkspaceId::from(workspace_id),
        url: row.get(2)?,
        title: row.get(3)?,
        folder_id: folder_id.map(TabFolderId::from),
        position: row.get(5)?,
        pinned: row.get(6)?,
        closed_at: row.get(7)?,
        archived: row.get(8)?,
    })
}

fn parse_uuid(value: &str) -> Result<Uuid, StorageError> {
    Uuid::parse_str(value)
        .map_err(|err| StorageError::DataCorruption(format!("invalid uuid {value}: {err}")))
}

fn to_sql_error(err: StorageError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(err))
}
