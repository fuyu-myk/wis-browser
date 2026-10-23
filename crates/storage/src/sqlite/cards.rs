//! SQLite queries for saved cards (see [`crate::cards`]).

use chrono::Utc;
use rusqlite::{params, Connection};

use crate::cards::{CardRecord, CardRow};
use crate::error::StorageResult;

/// Insert a card, or replace the one with `id`. Returns its row id.
pub fn save(conn: &Connection, id: Option<i64>, row: &CardRow) -> StorageResult<i64> {
    let now = Utc::now();
    let Some(id) = id else {
        conn.execute(
            "INSERT INTO cards (nickname, brand, last4, exp_month, exp_year, ciphertext, nonce, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
            params![row.nickname, row.brand, row.last4, row.exp_month, row.exp_year, row.ciphertext, row.nonce, now],
        )?;
        return Ok(conn.last_insert_rowid());
    };
    let changed = conn.execute(
        "UPDATE cards SET nickname = ?2, brand = ?3, last4 = ?4, exp_month = ?5, exp_year = ?6,
         ciphertext = ?7, nonce = ?8, updated_at = ?9 WHERE id = ?1",
        params![
            id,
            row.nickname,
            row.brand,
            row.last4,
            row.exp_month,
            row.exp_year,
            row.ciphertext,
            row.nonce,
            now
        ],
    )?;
    if changed == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows.into());
    }
    Ok(id)
}

/// Every card, oldest first.
pub fn list(conn: &Connection) -> StorageResult<Vec<CardRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, nickname, brand, last4, exp_month, exp_year, ciphertext, nonce, created_at, updated_at
         FROM cards ORDER BY id ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(CardRecord {
            id: row.get(0)?,
            nickname: row.get(1)?,
            brand: row.get(2)?,
            last4: row.get(3)?,
            exp_month: row.get(4)?,
            exp_year: row.get(5)?,
            ciphertext: row.get(6)?,
            nonce: row.get(7)?,
            created_at: row.get(8)?,
            updated_at: row.get(9)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn delete(conn: &Connection, id: i64) -> StorageResult<()> {
    conn.execute("DELETE FROM cards WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn clear(conn: &Connection) -> StorageResult<()> {
    conn.execute("DELETE FROM cards", [])?;
    Ok(())
}

pub fn any(conn: &Connection) -> StorageResult<bool> {
    conn.query_row("SELECT EXISTS(SELECT 1 FROM cards)", [], |row| row.get(0))
        .map_err(Into::into)
}
