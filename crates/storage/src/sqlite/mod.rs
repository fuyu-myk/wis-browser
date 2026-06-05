//! SQLite-backed [`Storage`] implementation.
//!
//! State is persisted in a small set of normalised tables (see
//! `migrations`). Each save replaces the whole persisted state in a single
//! atomic transaction.
//!
//! Loading is the inverse: reconstruct [`BrowserState`] from the tables, then
//! validate against [`BrowserState::invariant_violations`] before handing it
//! back. A corrupted store returns [`StorageError::DataCorruption`] so the
//! shell can log and fall back to defaults rather than crash.

mod convert;
mod credentials;
mod history;
mod migrations;
mod repo;

use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;
use wis_core::BrowserState;

use crate::error::{StorageError, StorageResult};
use crate::traits::Storage;

pub struct SqliteStorage {
    conn: Mutex<Connection>,
}

impl SqliteStorage {
    /// Open (or create) the SQLite file at `path` and run pending migrations.
    pub fn open(path: impl AsRef<Path>) -> StorageResult<Self> {
        let mut conn = Connection::open(path)?;
        Self::configure(&conn)?;
        migrations::run(&mut conn)?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Open an in-memory database. Used by tests.
    pub fn open_in_memory() -> StorageResult<Self> {
        let mut conn = Connection::open_in_memory()?;
        Self::configure(&conn)?;
        migrations::run(&mut conn)?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn configure(conn: &Connection) -> StorageResult<()> {
        // WAL gives us readers concurrent with writers and survives crashes
        // mid-transaction; FULL synchronous is the safe-by-default fsync mode.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "FULL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;

        Ok(())
    }
}

impl Storage for SqliteStorage {
    fn load_state(&self) -> StorageResult<Option<BrowserState>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        repo::load(&conn)
    }

    fn save_state(&self, state: &BrowserState) -> StorageResult<()> {
        let mut conn = self.conn.lock().expect("storage mutex poisoned");
        let tx = conn.transaction()?;

        repo::save(&tx, state)?;
        tx.commit()?;

        Ok(())
    }

    fn record_visit(
        &self,
        workspace_id: wis_core::WorkspaceId,
        url: &str,
        title: &str,
    ) -> StorageResult<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        history::record_visit(&conn, workspace_id, url, title)
    }

    fn update_visit_title(
        &self,
        workspace_id: wis_core::WorkspaceId,
        url: &str,
        title: &str,
    ) -> StorageResult<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        history::update_title(&conn, workspace_id, url, title)
    }

    fn query_history(
        &self,
        query: &crate::history::HistoryQuery,
    ) -> StorageResult<Vec<crate::history::HistoryEntry>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        history::query_history(&conn, query)
    }

    fn delete_history_entries(&self, ids: &[i64]) -> StorageResult<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        history::delete_entries(&conn, ids)
    }

    fn clear_history(&self, workspace_id: Option<wis_core::WorkspaceId>) -> StorageResult<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        history::clear(&conn, workspace_id)
    }

    fn save_credential(
        &self,
        host: &str,
        username: &str,
        ciphertext: &[u8],
        nonce: &[u8],
    ) -> StorageResult<i64> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        credentials::save(&conn, host, username, ciphertext, nonce)
    }

    fn list_credentials(
        &self,
        host: Option<&str>,
    ) -> StorageResult<Vec<crate::credentials::CredentialRecord>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        credentials::list(&conn, host)
    }

    fn get_credential(
        &self,
        id: i64,
    ) -> StorageResult<Option<crate::credentials::CredentialRecord>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        credentials::get(&conn, id)
    }

    fn delete_credential(&self, id: i64) -> StorageResult<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        credentials::delete(&conn, id)
    }
}

/// Verify the loaded state passes [`BrowserState::invariant_violations`].
pub(crate) fn validate(state: &BrowserState) -> StorageResult<()> {
    let violations = state.invariant_violations();
    if !violations.is_empty() {
        return Err(StorageError::DataCorruption(violations.join("; ")));
    }

    Ok(())
}
