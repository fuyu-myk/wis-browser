//! SQLite queries backing the completed-download history store (see
//! [`crate::downloads`]). Kept separate from `repo` because downloads are an
//! independent append-only table, not part of the serialised state.

use rusqlite::{params, Connection};
use uuid::Uuid;
use wis_core::DownloadId;

use crate::downloads::DownloadRecord;
use crate::error::{StorageError, StorageResult};

/// Insert a completed download. Idempotent per id (a replayed completion
/// overwrites its own row rather than duplicating).
pub fn record(conn: &Connection, record: &DownloadRecord) -> StorageResult<()> {
    conn.execute(
        r#"
        INSERT OR REPLACE INTO downloads (id, url, filename, path, bytes, completed_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        "#,
        params![
            record.id.as_uuid().to_string(),
            record.url,
            record.filename,
            record.path,
            record.bytes,
            record.completed_at,
        ],
    )?;

    Ok(())
}

/// Newest-first completed downloads, capped at `limit`.
pub fn list(conn: &Connection, limit: u32) -> StorageResult<Vec<DownloadRecord>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, url, filename, path, bytes, completed_at
        FROM downloads
        ORDER BY completed_at DESC
        LIMIT ?1
        "#,
    )?;
    let rows = stmt.query_map(params![limit], |row| {
        let id_str: String = row.get(0)?;
        let id = parse_download_id(&id_str).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(err))
        })?;

        Ok(DownloadRecord {
            id,
            url: row.get(1)?,
            filename: row.get(2)?,
            path: row.get(3)?,
            bytes: row.get(4)?,
            completed_at: row.get(5)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Fetch one completed download by id.
pub fn get(conn: &Connection, id: DownloadId) -> StorageResult<Option<DownloadRecord>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, url, filename, path, bytes, completed_at
        FROM downloads
        WHERE id = ?1
        "#,
    )?;
    let mut rows = stmt.query_map(params![id.as_uuid().to_string()], |row| {
        let id_str: String = row.get(0)?;
        let id = parse_download_id(&id_str).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(err))
        })?;

        Ok(DownloadRecord {
            id,
            url: row.get(1)?,
            filename: row.get(2)?,
            path: row.get(3)?,
            bytes: row.get(4)?,
            completed_at: row.get(5)?,
        })
    })?;

    rows.next().transpose().map_err(Into::into)
}

/// Remove one download from history (the file itself is untouched). Unknown
/// ids are silently ignored.
pub fn delete(conn: &Connection, id: DownloadId) -> StorageResult<()> {
    conn.execute(
        "DELETE FROM downloads WHERE id = ?1",
        params![id.as_uuid().to_string()],
    )?;

    Ok(())
}

/// Remove the entire download history (files are untouched).
pub fn clear(conn: &Connection) -> StorageResult<()> {
    conn.execute("DELETE FROM downloads", [])?;

    Ok(())
}

fn parse_download_id(s: &str) -> Result<DownloadId, StorageError> {
    Uuid::parse_str(s)
        .map(DownloadId::from)
        .map_err(StorageError::from)
}
