//! SQLite queries for saved addresses (see [`crate::addresses`]).

use chrono::Utc;
use rusqlite::{params, Connection};

use crate::addresses::AddressRecord;
use crate::error::StorageResult;

/// Insert an address, or replace the one with `id`. Returns its row id.
pub fn save(
    conn: &Connection,
    id: Option<i64>,
    ciphertext: &[u8],
    nonce: &[u8],
) -> StorageResult<i64> {
    let now = Utc::now();
    let Some(id) = id else {
        conn.execute(
            "INSERT INTO addresses (ciphertext, nonce, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
            params![ciphertext, nonce, now],
        )?;
        return Ok(conn.last_insert_rowid());
    };
    let changed = conn.execute(
        "UPDATE addresses SET ciphertext = ?2, nonce = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, ciphertext, nonce, now],
    )?;
    if changed == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows.into());
    }
    Ok(id)
}

/// Every address, oldest first.
pub fn list(conn: &Connection) -> StorageResult<Vec<AddressRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, ciphertext, nonce, created_at, updated_at FROM addresses ORDER BY id ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(AddressRecord {
            id: row.get(0)?,
            ciphertext: row.get(1)?,
            nonce: row.get(2)?,
            created_at: row.get(3)?,
            updated_at: row.get(4)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn delete(conn: &Connection, id: i64) -> StorageResult<()> {
    conn.execute("DELETE FROM addresses WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn clear(conn: &Connection) -> StorageResult<()> {
    conn.execute("DELETE FROM addresses", [])?;
    Ok(())
}

pub fn any(conn: &Connection) -> StorageResult<bool> {
    conn.query_row("SELECT EXISTS(SELECT 1 FROM addresses)", [], |row| {
        row.get(0)
    })
    .map_err(Into::into)
}
