//! SQLite queries backing the browsing-history store (see
//! [`crate::history`]). Kept separate from `repo` (which round-trips the whole
//! [`wis_core::BrowserState`]) because history is an independent append-style
//! table, not part of the serialised state.
//!
//! History is scoped per workspace: dedup is keyed on `(workspace_id, url)`, so
//! the same page visited in two workspaces keeps independent counts.

use chrono::Utc;
use rusqlite::{params, params_from_iter, Connection};
use uuid::Uuid;
use wis_core::WorkspaceId;

use crate::error::{StorageError, StorageResult};
use crate::history::{HistoryEntry, HistoryQuery};

/// Upsert a visit, deduplicated by `(workspace_id, url)`.
pub fn record_visit(
    conn: &Connection,
    workspace_id: WorkspaceId,
    url: &str,
    title: &str,
) -> StorageResult<()> {
    let now = Utc::now();
    conn.execute(
        r#"
        INSERT INTO history (workspace_id, url, title, visited_at, visit_count)
        VALUES (?1, ?2, ?3, ?4, 1)
        ON CONFLICT(workspace_id, url) DO UPDATE SET
            visit_count = history.visit_count + 1,
            visited_at  = excluded.visited_at,
            title       = CASE WHEN excluded.title <> '' THEN excluded.title
                               ELSE history.title END
        "#,
        params![workspace_id.as_uuid().to_string(), url, title, now],
    )?;

    Ok(())
}

/// Newest-first history, filtered by a case-insensitive substring over
/// url + title (empty matches all), optionally scoped to one workspace, and
/// paginated.
pub fn query_history(conn: &Connection, query: &HistoryQuery) -> StorageResult<Vec<HistoryEntry>> {
    let pattern = format!("%{}%", query.search);
    let ws_filter: Option<String> = query.workspace_id.map(|w| w.as_uuid().to_string());
    let mut stmt = conn.prepare(
        r#"
        SELECT id, workspace_id, url, title, visited_at, visit_count
        FROM history
        WHERE (url LIKE ?1 OR title LIKE ?1)
          AND (?2 IS NULL OR workspace_id = ?2)
        ORDER BY visited_at DESC
        LIMIT ?3 OFFSET ?4
        "#,
    )?;
    let rows = stmt.query_map(
        params![pattern, ws_filter, query.effective_limit(), query.offset],
        |row| {
            let ws_str: String = row.get(1)?;
            let workspace_id = parse_workspace_id(&ws_str).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    1,
                    rusqlite::types::Type::Text,
                    Box::new(err),
                )
            })?;
            Ok(HistoryEntry {
                id: row.get(0)?,
                workspace_id,
                url: row.get(2)?,
                title: row.get(3)?,
                visited_at: row.get(4)?,
                visit_count: row.get(5)?,
            })
        },
    )?;

    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Update the title of an existing `(workspace_id, url)` row without touching
/// its visit count. Used when a page's real title arrives after the visit was
/// first recorded (e.g. a YouTube video whose title loads late). No-op for an
/// empty title or a URL not yet in history.
pub fn update_title(
    conn: &Connection,
    workspace_id: WorkspaceId,
    url: &str,
    title: &str,
) -> StorageResult<()> {
    if title.is_empty() {
        return Ok(());
    }

    conn.execute(
        "UPDATE history SET title = ?3 WHERE workspace_id = ?1 AND url = ?2",
        params![workspace_id.as_uuid().to_string(), url, title],
    )?;

    Ok(())
}

/// Delete rows by id. Unknown ids are silently ignored.
pub fn delete_entries(conn: &Connection, ids: &[i64]) -> StorageResult<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let placeholders = std::iter::repeat("?")
        .take(ids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!("DELETE FROM history WHERE id IN ({placeholders})");
    conn.execute(&sql, params_from_iter(ids.iter()))?;

    Ok(())
}

/// Remove history rows. Scoped to one workspace when `workspace_id` is set,
/// otherwise clears everything.
pub fn clear(conn: &Connection, workspace_id: Option<WorkspaceId>) -> StorageResult<()> {
    match workspace_id {
        Some(id) => {
            conn.execute(
                "DELETE FROM history WHERE workspace_id = ?1",
                params![id.as_uuid().to_string()],
            )?;
        }
        None => {
            conn.execute("DELETE FROM history", [])?;
        }
    }

    Ok(())
}

fn parse_workspace_id(s: &str) -> Result<WorkspaceId, StorageError> {
    Uuid::parse_str(s)
        .map(WorkspaceId::from)
        .map_err(StorageError::from)
}
