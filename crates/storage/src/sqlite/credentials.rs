//! SQLite queries for the password vault (see [`crate::credentials`]).
//! Independent of the serialised `BrowserState`.

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};

use crate::credentials::CredentialRecord;
use crate::error::StorageResult;

/// Upsert a credential, deduplicated by `(host, username)`. Returns the row id.
pub fn save(
    conn: &Connection,
    host: &str,
    username: &str,
    ciphertext: &[u8],
    nonce: &[u8],
) -> StorageResult<i64> {
    let now = Utc::now();
    conn.execute(
        r#"
        INSERT INTO credentials (host, username, ciphertext, nonce, created_at, updated_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?5)
        ON CONFLICT(host, username) DO UPDATE SET
            ciphertext = excluded.ciphertext,
            nonce      = excluded.nonce,
            updated_at = excluded.updated_at
        "#,
        params![host, username, ciphertext, nonce, now],
    )?;
    let id: i64 = conn.query_row(
        "SELECT id FROM credentials WHERE host = ?1 AND username = ?2",
        params![host, username],
        |row| row.get(0),
    )?;

    Ok(id)
}

/// List credentials newest-first, optionally filtered to one host.
pub fn list(conn: &Connection, host: Option<&str>) -> StorageResult<Vec<CredentialRecord>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, host, username, ciphertext, nonce, created_at, updated_at
        FROM credentials
        WHERE (?1 IS NULL OR host = ?1)
        ORDER BY host ASC, username ASC
        "#,
    )?;
    let rows = stmt.query_map(params![host], row_to_record)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Fetch one credential by id.
pub fn get(conn: &Connection, id: i64) -> StorageResult<Option<CredentialRecord>> {
    conn.query_row(
        r#"
        SELECT id, host, username, ciphertext, nonce, created_at, updated_at
        FROM credentials WHERE id = ?1
        "#,
        params![id],
        row_to_record,
    )
    .optional()
    .map_err(Into::into)
}

/// Delete one credential by id.
pub fn delete(conn: &Connection, id: i64) -> StorageResult<()> {
    conn.execute("DELETE FROM credentials WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn clear(conn: &Connection) -> StorageResult<()> {
    conn.execute("DELETE FROM credentials", [])?;
    Ok(())
}

pub fn any(conn: &Connection) -> StorageResult<bool> {
    conn.query_row("SELECT EXISTS(SELECT 1 FROM credentials)", [], |row| {
        row.get(0)
    })
    .map_err(Into::into)
}

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<CredentialRecord> {
    Ok(CredentialRecord {
        id: row.get(0)?,
        host: row.get(1)?,
        username: row.get(2)?,
        ciphertext: row.get(3)?,
        nonce: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}
