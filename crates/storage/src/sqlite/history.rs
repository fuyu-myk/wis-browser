//! SQLite queries backing the browsing-history store (see
//! [`crate::history`]). Kept separate from `repo` (which round-trips the whole
//! [`wis_core::BrowserState`]) because history is an independent append-style
//! table, not part of the serialised state.
//!
//! History is scoped per workspace: dedup is keyed on `(workspace_id, url)`, so
//! the same page visited in two workspaces keeps independent counts.

use chrono::{DateTime, Utc};
use rusqlite::{params, params_from_iter, Connection};
use uuid::Uuid;
use wis_core::WorkspaceId;

use crate::error::{StorageError, StorageResult};
use crate::history::{HistoryEntry, HistoryQuery};

/// The identity a visit is stored under: the URL without its fragment. A `#`
/// only ever delimits the fragment in a valid URL (it is percent-encoded
/// anywhere else), so in-page anchor jumps (`page#a`, `page#b`) update the one
/// `page` row instead of piling up near-duplicate history entries.
fn history_key(url: &str) -> &str {
    url.split('#').next().unwrap_or(url)
}

/// Upsert a visit, deduplicated by `(workspace_id, url)`, fragment ignored.
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
        params![
            workspace_id.as_uuid().to_string(),
            history_key(url),
            title,
            now
        ],
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

/// Recommend revisit-worthy sites for the URL-bar suggestions: rows in one
/// workspace matching `query` (substring over url+title; empty matches all),
/// ranked by visit count then recency.
pub fn recommend(
    conn: &Connection,
    workspace_id: WorkspaceId,
    query: &str,
    limit: u32,
) -> StorageResult<Vec<HistoryEntry>> {
    let pattern = format!("%{query}%");
    let ws = workspace_id.as_uuid().to_string();
    let mut stmt = conn.prepare(
        r#"
        SELECT id, workspace_id, url, title, visited_at, visit_count
        FROM history
        WHERE workspace_id = ?1 AND (url LIKE ?2 OR title LIKE ?2)
        ORDER BY visit_count DESC, visited_at DESC
        LIMIT ?3
        "#,
    )?;
    let rows = stmt.query_map(params![ws, pattern, limit], |row| {
        let ws_str: String = row.get(1)?;
        let workspace_id = parse_workspace_id(&ws_str).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(err))
        })?;
        Ok(HistoryEntry {
            id: row.get(0)?,
            workspace_id,
            url: row.get(2)?,
            title: row.get(3)?,
            visited_at: row.get(4)?,
            visit_count: row.get(5)?,
        })
    })?;

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
        params![workspace_id.as_uuid().to_string(), history_key(url), title],
    )?;

    Ok(())
}

/// Delete rows by id. Unknown ids are silently ignored.
pub fn delete_entries(conn: &Connection, ids: &[i64]) -> StorageResult<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let placeholders = std::iter::repeat_n("?", ids.len())
        .take(ids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!("DELETE FROM history WHERE id IN ({placeholders})");
    conn.execute(&sql, params_from_iter(ids.iter()))?;

    Ok(())
}

/// The URLs of the given history rows. Unknown ids are skipped.
pub fn urls_for(conn: &Connection, ids: &[i64]) -> StorageResult<Vec<String>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders = std::iter::repeat_n("?", ids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!("SELECT url FROM history WHERE id IN ({placeholders})");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(ids.iter()), |row| row.get::<_, String>(0))?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }

    Ok(out)
}

/// Remove history rows. Scoped to one workspace when `workspace_id` is set,
/// otherwise clears everything.
pub fn clear(
    conn: &Connection,
    workspace_id: Option<WorkspaceId>,
    since: Option<DateTime<Utc>>,
) -> StorageResult<()> {
    // `visited_at` is stored as text, so they are comparable as instants
    // via `datetime()`.
    match (workspace_id, since) {
        (Some(id), Some(since)) => {
            conn.execute(
                "DELETE FROM history \
                 WHERE workspace_id = ?1 AND datetime(visited_at) >= datetime(?2)",
                params![id.as_uuid().to_string(), since],
            )?;
        }
        (Some(id), None) => {
            conn.execute(
                "DELETE FROM history WHERE workspace_id = ?1",
                params![id.as_uuid().to_string()],
            )?;
        }
        (None, Some(since)) => {
            conn.execute(
                "DELETE FROM history WHERE datetime(visited_at) >= datetime(?1)",
                params![since],
            )?;
        }
        (None, None) => {
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("in-memory DB");
        super::super::migrations::run(&mut conn).expect("migrations");
        conn
    }

    fn backdate(conn: &Connection, url: &str, by: Duration) {
        conn.execute(
            "UPDATE history SET visited_at = ?1 WHERE url = ?2",
            params![Utc::now() - by, history_key(url)],
        )
        .expect("backdate");
    }

    fn urls(conn: &Connection) -> Vec<String> {
        query_history(conn, &crate::HistoryQuery::default())
            .expect("query")
            .into_iter()
            .map(|e| e.url)
            .collect()
    }

    #[test]
    fn clear_since_keeps_visits_older_than_the_instant() {
        let conn = conn();
        let ws = WorkspaceId::new();
        record_visit(&conn, ws, "https://old.example/", "Old").unwrap();
        record_visit(&conn, ws, "https://new.example/", "New").unwrap();
        backdate(&conn, "https://old.example/", Duration::days(2));

        clear(&conn, None, Some(Utc::now() - Duration::hours(1))).unwrap();

        assert_eq!(urls(&conn), vec!["https://old.example/"]);
    }

    #[test]
    fn clear_since_honours_the_workspace_scope() {
        let conn = conn();
        let a = WorkspaceId::new();
        let b = WorkspaceId::new();
        record_visit(&conn, a, "https://a.example/", "A").unwrap();
        record_visit(&conn, a, "https://a-old.example/", "A old").unwrap();
        record_visit(&conn, b, "https://b.example/", "B").unwrap();
        backdate(&conn, "https://a-old.example/", Duration::days(2));

        clear(&conn, Some(a), Some(Utc::now() - Duration::hours(1))).unwrap();

        let mut left = urls(&conn);
        left.sort();
        assert_eq!(left, vec!["https://a-old.example/", "https://b.example/"]);
    }
}
