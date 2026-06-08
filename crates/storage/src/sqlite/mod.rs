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

mod bookmarks;
mod convert;
mod credentials;
mod history;
mod migrations;
mod repo;
mod searches;

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

    /// Write a consistent standalone copy of the database to `dst`. Uses
    /// `VACUUM INTO`, which snapshots the committed state correctly even with
    /// WAL active and produces a single self-contained file (no `-wal`/`-shm` sidecars).
    /// `dst` is replaced if it already exists.
    pub fn backup_to(&self, dst: &Path) -> StorageResult<()> {
        if dst.exists() {
            let _ = std::fs::remove_file(dst);
        }
        let conn = self.conn.lock().expect("storage mutex poisoned");
        let target = dst.to_string_lossy().replace('\'', "''");

        conn.execute_batch(&format!("VACUUM INTO '{target}'"))?;
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

    fn record_search(&self, query: &str) -> StorageResult<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        searches::record(&conn, query)
    }

    fn suggest_searches(
        &self,
        query: &str,
        limit: u32,
    ) -> StorageResult<Vec<crate::searches::SearchEntry>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        searches::suggest(&conn, query, limit)
    }

    fn recent_searches(&self, limit: u32) -> StorageResult<Vec<crate::searches::SearchEntry>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        searches::recent(&conn, limit)
    }

    fn delete_search(&self, query: &str) -> StorageResult<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        searches::delete(&conn, query)
    }

    fn recommend_sites(
        &self,
        workspace_id: wis_core::WorkspaceId,
        query: &str,
        limit: u32,
    ) -> StorageResult<Vec<crate::history::HistoryEntry>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        history::recommend(&conn, workspace_id, query, limit)
    }

    fn list_bookmark_folders(
        &self,
        workspace_id: wis_core::WorkspaceId,
    ) -> StorageResult<Vec<crate::bookmarks::BookmarkFolder>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        bookmarks::list_folders(&conn, workspace_id)
    }

    fn create_bookmark_folder(
        &self,
        workspace_id: wis_core::WorkspaceId,
        name: &str,
    ) -> StorageResult<crate::bookmarks::BookmarkFolder> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        bookmarks::create_folder(&conn, workspace_id, name)
    }

    fn rename_bookmark_folder(&self, id: i64, name: &str) -> StorageResult<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        bookmarks::rename_folder(&conn, id, name)
    }

    fn reorder_bookmark_folders(&self, ids: &[i64]) -> StorageResult<()> {
        let mut conn = self.conn.lock().expect("storage mutex poisoned");
        bookmarks::reorder_folders(&mut conn, ids)
    }

    fn delete_bookmark_folder(&self, id: i64) -> StorageResult<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        bookmarks::delete_folder(&conn, id)
    }

    fn list_bookmarks(
        &self,
        workspace_id: wis_core::WorkspaceId,
    ) -> StorageResult<Vec<crate::bookmarks::Bookmark>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        bookmarks::list_bookmarks(&conn, workspace_id)
    }

    fn add_bookmark(
        &self,
        workspace_id: wis_core::WorkspaceId,
        folder_id: Option<i64>,
        url: &str,
        title: &str,
        favicon_hash: Option<&str>,
        pinned: bool,
    ) -> StorageResult<crate::bookmarks::Bookmark> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        bookmarks::add_bookmark(&conn, workspace_id, folder_id, url, title, favicon_hash, pinned)
    }

    fn move_bookmark(&self, id: i64, folder_id: Option<i64>) -> StorageResult<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        bookmarks::move_bookmark(&conn, id, folder_id)
    }

    fn set_bookmark_pinned(&self, id: i64, pinned: bool) -> StorageResult<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        bookmarks::set_pinned(&conn, id, pinned)
    }

    fn reorder_pinned_bookmarks(&self, ids: &[i64]) -> StorageResult<()> {
        let mut conn = self.conn.lock().expect("storage mutex poisoned");
        bookmarks::reorder_pinned(&mut conn, ids)
    }

    fn reorder_bookmarks(&self, ids: &[i64]) -> StorageResult<()> {
        let mut conn = self.conn.lock().expect("storage mutex poisoned");
        bookmarks::reorder_bookmarks(&mut conn, ids)
    }

    fn delete_bookmark(&self, id: i64) -> StorageResult<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        bookmarks::delete_bookmark(&conn, id)
    }

    fn save_session_cookies(
        &self,
        workspace_id: wis_core::WorkspaceId,
        data: &str,
    ) -> StorageResult<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        let ws = workspace_id.as_uuid().to_string();
        let now = chrono::Utc::now();
        // `created_at` is set only on first insert; later snapshots refresh the
        // data + `updated_at` but keep `created_at`, so the absolute TTL holds.
        conn.execute(
            "INSERT INTO session_cookies (workspace_id, data, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?3) \
             ON CONFLICT(workspace_id) DO UPDATE SET data = ?2, updated_at = ?3",
            rusqlite::params![ws, data, now],
        )?;

        Ok(())
    }

    fn load_session_cookies(
        &self,
        workspace_id: wis_core::WorkspaceId,
        max_age_days: i64,
    ) -> StorageResult<Option<String>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        let ws = workspace_id.as_uuid().to_string();
        let cutoff = chrono::Utc::now() - chrono::Duration::days(max_age_days);
        let data = conn
            .query_row(
                "SELECT data FROM session_cookies WHERE workspace_id = ?1 AND created_at >= ?2",
                rusqlite::params![ws, cutoff],
                |row| row.get::<_, String>(0),
            )
            .ok();

        Ok(data)
    }

    fn purge_stale_session_cookies(&self, max_age_days: i64) -> StorageResult<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        let cutoff = chrono::Utc::now() - chrono::Duration::days(max_age_days);
        conn.execute(
            "DELETE FROM session_cookies WHERE created_at < ?1",
            rusqlite::params![cutoff],
        )?;

        Ok(())
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
