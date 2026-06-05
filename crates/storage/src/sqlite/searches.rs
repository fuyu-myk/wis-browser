//! SQLite queries for address-bar search history (see [`crate::searches`]).

use chrono::Utc;
use rusqlite::{params, Connection};

use crate::error::StorageResult;
use crate::searches::SearchEntry;

/// Record a search query, deduplicated by text (bumps `count`, refreshes time).
pub fn record(conn: &Connection, query: &str) -> StorageResult<()> {
    let now = Utc::now();
    conn.execute(
        r#"
        INSERT INTO searches (query, searched_at, count)
        VALUES (?1, ?2, 1)
        ON CONFLICT(query) DO UPDATE SET
            count = searches.count + 1,
            searched_at = excluded.searched_at
        "#,
        params![query, now],
    )?;

    Ok(())
}

/// Suggest past searches matching `query` (substring, case-insensitive),
/// most-used and most-recent first. Empty `query` returns the top searches.
pub fn suggest(conn: &Connection, query: &str, limit: u32) -> StorageResult<Vec<SearchEntry>> {
    let pattern = format!("%{query}%");
    let mut stmt = conn.prepare(
        r#"
        SELECT id, query, searched_at, count
        FROM searches
        WHERE query LIKE ?1
        ORDER BY count DESC, searched_at DESC
        LIMIT ?2
        "#,
    )?;
    let rows = stmt.query_map(params![pattern, limit], |row| {
        Ok(SearchEntry {
            id: row.get(0)?,
            query: row.get(1)?,
            searched_at: row.get(2)?,
            count: row.get(3)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// The most recently performed searches, newest first. Used for the empty
/// address-bar dropdown (before the user types anything).
pub fn recent(conn: &Connection, limit: u32) -> StorageResult<Vec<SearchEntry>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, query, searched_at, count
        FROM searches
        ORDER BY searched_at DESC
        LIMIT ?1
        "#,
    )?;
    let rows = stmt.query_map(params![limit], |row| {
        Ok(SearchEntry {
            id: row.get(0)?,
            query: row.get(1)?,
            searched_at: row.get(2)?,
            count: row.get(3)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Forget a search query so it's no longer suggested.
pub fn delete(conn: &Connection, query: &str) -> StorageResult<()> {
    conn.execute("DELETE FROM searches WHERE query = ?1", params![query])?;
    Ok(())
}
