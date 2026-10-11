//! SQLite queries backing the browsing-history store (see
//! [`crate::history`]). Kept separate from `repo` (which round-trips the whole
//! [`wis_core::BrowserState`]) because history is an independent append-style
//! table, not part of the serialised state.
//!
//! History is scoped per workspace: dedup is keyed on `(workspace_id, url)`, so
//! the same page visited in two workspaces keeps independent counts.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use rusqlite::{params, params_from_iter, Connection};
use uuid::Uuid;
use wis_core::WorkspaceId;

use crate::error::{StorageError, StorageResult};
use crate::history::{HistoryEntry, HistoryImport, HistoryImportStats, HistoryQuery};

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

pub fn import(
    conn: &mut Connection,
    workspace_id: WorkspaceId,
    entries: &[HistoryImport],
) -> StorageResult<HistoryImportStats> {
    let ws = workspace_id.as_uuid().to_string();
    let mut stats = HistoryImportStats {
        added: 0,
        merged: 0,
    };
    let tx = conn.transaction()?;
    {
        let mut known = tx.prepare("SELECT 1 FROM history WHERE workspace_id = ?1 AND url = ?2")?;
        let mut upsert = tx.prepare(
            r#"
            INSERT INTO history (workspace_id, url, title, visited_at, visit_count)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(workspace_id, url) DO UPDATE SET
                visit_count = max(history.visit_count, excluded.visit_count),
                visited_at  = CASE WHEN datetime(excluded.visited_at) > datetime(history.visited_at)
                                   THEN excluded.visited_at ELSE history.visited_at END,
                title       = CASE WHEN history.title = '' THEN excluded.title
                                   ELSE history.title END
            "#,
        )?;
        let mut counted = HashSet::new();
        for entry in entries {
            let url = history_key(&entry.url);
            if counted.insert(url) {
                if known.exists(params![ws, url])? {
                    stats.merged += 1;
                } else {
                    stats.added += 1;
                }
            }

            upsert.execute(params![
                ws,
                url,
                entry.title,
                entry.visited_at,
                entry.visit_count.max(1)
            ])?;
        }
    }
    tx.commit()?;

    Ok(stats)
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

// `visited_at` and the bound are both rusqlite's `%F %T%.f+00:00` text, whose
// `+` sorts below the fraction's `.` and digits, so comparing the text orders
// them exactly and lets the scoped clear search its index.
const CLEAR_WORKSPACE_SINCE: &str =
    "DELETE FROM history WHERE workspace_id = ?1 AND visited_at >= ?2";
const CLEAR_SINCE: &str = "DELETE FROM history WHERE visited_at >= ?1";

/// Remove history rows. Scoped to one workspace when `workspace_id` is set,
/// otherwise clears everything.
pub fn clear(
    conn: &Connection,
    workspace_id: Option<WorkspaceId>,
    since: Option<DateTime<Utc>>,
) -> StorageResult<()> {
    match (workspace_id, since) {
        (Some(id), Some(since)) => {
            conn.execute(
                CLEAR_WORKSPACE_SINCE,
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
            conn.execute(CLEAR_SINCE, params![since])?;
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

    fn visit(url: &str, title: &str, days_ago: i64, count: i64) -> HistoryImport {
        HistoryImport {
            url: url.to_string(),
            title: title.to_string(),
            visited_at: Utc::now() - Duration::days(days_ago),
            visit_count: count,
        }
    }

    fn rows(conn: &Connection, ws: WorkspaceId) -> Vec<(String, String, i64)> {
        query_history(
            conn,
            &crate::HistoryQuery {
                workspace_id: Some(ws),
                ..Default::default()
            },
        )
        .expect("query")
        .into_iter()
        .map(|e| (e.url, e.title, e.visit_count))
        .collect()
    }

    #[test]
    fn imported_visits_keep_their_own_time_and_count() {
        let mut conn = conn();
        let ws = WorkspaceId::new();
        let entries = [
            visit("https://a.example/", "A", 1, 7),
            visit("https://b.example/#top", "B", 3, 2),
        ];

        let stats = import(&mut conn, ws, &entries).unwrap();

        assert_eq!(
            stats,
            HistoryImportStats {
                added: 2,
                merged: 0
            }
        );
        assert_eq!(
            rows(&conn, ws),
            vec![
                ("https://a.example/".to_string(), "A".to_string(), 7),
                ("https://b.example/".to_string(), "B".to_string(), 2),
            ]
        );
        let newest = &query_history(&conn, &crate::HistoryQuery::default()).unwrap()[0];
        assert!(newest.visited_at < Utc::now() - Duration::hours(23));
    }

    #[test]
    fn importing_the_same_visits_twice_changes_nothing() {
        let mut conn = conn();
        let ws = WorkspaceId::new();
        let entries = [visit("https://a.example/", "A", 1, 7)];
        import(&mut conn, ws, &entries).unwrap();
        let before = rows(&conn, ws);

        let stats = import(&mut conn, ws, &entries).unwrap();

        assert_eq!(
            stats,
            HistoryImportStats {
                added: 0,
                merged: 1
            }
        );
        assert_eq!(rows(&conn, ws), before);
    }

    #[test]
    fn a_known_url_keeps_its_later_visit_higher_count_and_title() {
        let mut conn = conn();
        let ws = WorkspaceId::new();
        record_visit(&conn, ws, "https://a.example/", "Here").unwrap();

        import(
            &mut conn,
            ws,
            &[visit("https://a.example/", "There", 10, 4)],
        )
        .unwrap();

        assert_eq!(
            rows(&conn, ws),
            vec![("https://a.example/".to_string(), "Here".to_string(), 4)]
        );
        let entry = &query_history(&conn, &crate::HistoryQuery::default()).unwrap()[0];
        assert!(entry.visited_at > Utc::now() - Duration::hours(1));
    }

    #[test]
    fn an_untitled_row_takes_the_imported_title() {
        let mut conn = conn();
        let ws = WorkspaceId::new();
        record_visit(&conn, ws, "https://a.example/", "").unwrap();

        import(&mut conn, ws, &[visit("https://a.example/", "There", 1, 1)]).unwrap();

        assert_eq!(rows(&conn, ws)[0].1, "There");
    }

    #[test]
    fn urls_differing_only_by_fragment_count_once() {
        let mut conn = conn();
        let ws = WorkspaceId::new();
        let entries = [
            visit("https://a.example/#x", "A", 1, 2),
            visit("https://a.example/#y", "A", 2, 3),
        ];

        let stats = import(&mut conn, ws, &entries).unwrap();

        assert_eq!(
            stats,
            HistoryImportStats {
                added: 1,
                merged: 0
            }
        );
        assert_eq!(
            rows(&conn, ws),
            vec![("https://a.example/".to_string(), "A".to_string(), 3)]
        );
    }

    #[test]
    fn imports_stay_in_their_workspace() {
        let mut conn = conn();
        let (a, b) = (WorkspaceId::new(), WorkspaceId::new());
        record_visit(&conn, b, "https://a.example/", "B's").unwrap();

        import(&mut conn, a, &[visit("https://a.example/", "A's", 1, 9)]).unwrap();

        assert_eq!(
            rows(&conn, b)[0],
            ("https://a.example/".to_string(), "B's".to_string(), 1)
        );
        assert_eq!(rows(&conn, a)[0].2, 9);
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

    #[test]
    fn clear_since_orders_stored_times_at_every_fraction_precision() {
        let conn = conn();
        let ws = WorkspaceId::new();
        let bound = DateTime::parse_from_rfc3339("2026-10-08T01:00:00.000500Z")
            .unwrap()
            .with_timezone(&Utc);
        let at = |url: &str, rfc3339: &str| {
            record_visit(&conn, ws, url, "").unwrap();
            let when = DateTime::parse_from_rfc3339(rfc3339)
                .unwrap()
                .with_timezone(&Utc);
            conn.execute(
                "UPDATE history SET visited_at = ?1 WHERE url = ?2",
                params![when, history_key(url)],
            )
            .unwrap();
        };
        at("https://whole.example/", "2026-10-08T01:00:00Z");
        at("https://micro.example/", "2026-10-08T01:00:00.000001Z");
        at("https://milli.example/", "2026-10-08T01:00:00.001Z");
        at("https://nano.example/", "2026-10-08T01:00:00.000500001Z");
        at("https://later.example/", "2026-10-08T01:00:01Z");

        clear(&conn, Some(ws), Some(bound)).unwrap();

        let mut left = urls(&conn);
        left.sort();
        assert_eq!(
            left,
            vec!["https://micro.example/", "https://whole.example/"]
        );
    }

    #[test]
    fn a_scoped_timed_clear_searches_the_visited_at_index() {
        let conn = conn();
        let plan: String = conn
            .query_row(
                &format!("EXPLAIN QUERY PLAN {CLEAR_WORKSPACE_SINCE}"),
                params!["ws", Utc::now()],
                |row| row.get(3),
            )
            .unwrap();
        assert!(
            plan.contains("idx_history_ws_visited_at (workspace_id=? AND visited_at>?)"),
            "{plan}"
        );
    }
}
