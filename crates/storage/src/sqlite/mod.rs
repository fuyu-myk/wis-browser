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

mod addresses;
mod bookmarks;
mod canvases;
mod closed_tabs;
mod convert;
mod credentials;
mod downloads;
mod history;
mod migrations;
mod repo;
mod searches;
mod usage;

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, OpenFlags};
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
        // mid-transaction. NORMAL synchronous is the recommended pairing with
        // WAL: it can never corrupt the database (the WAL is synced at
        // checkpoints), it only risks losing the most recent commit on power
        // loss.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
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

/// Report whether the database file at `path` passes SQLite's own integrity
/// check.
///
/// A file that is `ok` still holds every row it ever held, so callers must
/// never discard it. Anything else counts as not intact.
pub fn is_intact(path: &Path) -> bool {
    let Ok(conn) = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY) else {
        return false;
    };

    matches!(
        conn.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0)),
        Ok(result) if result == "ok"
    )
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

    fn save_windowed_workspaces(&self, ids: &[wis_core::WorkspaceId]) -> StorageResult<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        repo::save_windowed_workspaces(&conn, ids)
    }

    fn load_windowed_workspaces(&self) -> StorageResult<Vec<wis_core::WorkspaceId>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        repo::load_windowed_workspaces(&conn)
    }

    fn set_meta(&self, key: &str, value: &str) -> StorageResult<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        repo::set_meta(&conn, key, value)
    }

    fn get_meta(&self, key: &str) -> StorageResult<Option<String>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        repo::get_meta(&conn, key)
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

    fn add_usage(&self, credits: &[wis_core::library::usage::Credit]) -> StorageResult<()> {
        let mut conn = self.conn.lock().expect("storage mutex poisoned");
        usage::add(&mut conn, credits)
    }

    fn usage(
        &self,
        workspace_id: Option<wis_core::WorkspaceId>,
        from: chrono::NaiveDate,
        to: chrono::NaiveDate,
    ) -> StorageResult<Vec<wis_core::library::usage::UsageRow>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        usage::query(&conn, workspace_id, from, to)
    }

    fn clear_usage(
        &self,
        workspace_id: Option<wis_core::WorkspaceId>,
        from: Option<(chrono::NaiveDate, u8)>,
    ) -> StorageResult<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        usage::clear(&conn, workspace_id, from)
    }

    fn list_canvases(
        &self,
        workspace_id: wis_core::WorkspaceId,
    ) -> StorageResult<Vec<wis_core::canvas::board::CanvasSummary>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        canvases::list(&conn, workspace_id)
    }

    fn load_canvas(
        &self,
        workspace_id: wis_core::WorkspaceId,
        id: wis_core::CanvasId,
    ) -> StorageResult<Option<wis_core::canvas::board::CanvasRecord>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        canvases::load(&conn, workspace_id, id)
    }

    fn insert_canvas(&self, canvas: &wis_core::canvas::board::CanvasRecord) -> StorageResult<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        canvases::insert(&conn, canvas)
    }

    fn rename_canvas(
        &self,
        workspace_id: wis_core::WorkspaceId,
        id: wis_core::CanvasId,
        name: &str,
        updated_at: i64,
    ) -> StorageResult<bool> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        canvases::rename(&conn, workspace_id, id, name, updated_at)
    }

    fn save_canvas_docs(
        &self,
        docs: &[wis_core::canvas::board::CanvasDocUpdate],
    ) -> StorageResult<Vec<wis_core::CanvasId>> {
        let mut conn = self.conn.lock().expect("storage mutex poisoned");
        canvases::save_docs(&mut conn, docs)
    }

    fn delete_canvas(
        &self,
        workspace_id: wis_core::WorkspaceId,
        id: wis_core::CanvasId,
    ) -> StorageResult<bool> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        canvases::delete(&conn, workspace_id, id)
    }

    fn delete_workspace_canvases(&self, workspace_id: wis_core::WorkspaceId) -> StorageResult<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        canvases::delete_workspace(&conn, workspace_id)
    }

    fn import_history(
        &self,
        workspace_id: wis_core::WorkspaceId,
        entries: &[crate::history::HistoryImport],
    ) -> StorageResult<crate::history::HistoryImportStats> {
        let mut conn = self.conn.lock().expect("storage mutex poisoned");
        history::import(&mut conn, workspace_id, entries)
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

    fn clear_history(
        &self,
        workspace_id: Option<wis_core::WorkspaceId>,
        since: Option<chrono::DateTime<chrono::Utc>>,
    ) -> StorageResult<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        history::clear(&conn, workspace_id, since)
    }

    fn history_urls(&self, ids: &[i64]) -> StorageResult<Vec<String>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        history::urls_for(&conn, ids)
    }

    fn delete_closed_tabs_by_urls(&self, urls: &[String]) -> StorageResult<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        closed_tabs::delete_by_urls(&conn, urls)
    }

    fn record_closed_tab(
        &self,
        record: &crate::closed_tabs::ClosedTabRecord,
    ) -> StorageResult<i64> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        closed_tabs::record(&conn, record)
    }

    fn record_closed_tabs(
        &self,
        records: &[crate::closed_tabs::ClosedTabRecord],
    ) -> StorageResult<Vec<i64>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        closed_tabs::record_all(&conn, records)
    }

    fn list_closed_tabs(
        &self,
        limit: u32,
    ) -> StorageResult<Vec<crate::closed_tabs::ClosedTabRecord>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        closed_tabs::list(&conn, limit)
    }

    fn take_closed_tab(
        &self,
        id: i64,
    ) -> StorageResult<Option<crate::closed_tabs::ClosedTabRecord>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        closed_tabs::take(&conn, id)
    }

    fn take_latest_closed_tab(&self) -> StorageResult<Option<crate::closed_tabs::ClosedTabRecord>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        closed_tabs::take_latest(&conn)
    }

    fn prune_closed_tabs(&self, keep: u32) -> StorageResult<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        closed_tabs::prune(&conn, keep)
    }

    fn clear_closed_tabs(
        &self,
        workspace_id: Option<wis_core::WorkspaceId>,
        since: Option<chrono::DateTime<chrono::Utc>>,
    ) -> StorageResult<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        closed_tabs::clear(&conn, workspace_id, since)
    }

    fn record_download(&self, record: &crate::downloads::DownloadRecord) -> StorageResult<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        downloads::record(&conn, record)
    }

    fn list_downloads(&self, limit: u32) -> StorageResult<Vec<crate::downloads::DownloadRecord>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        downloads::list(&conn, limit)
    }

    fn get_download(
        &self,
        id: wis_core::DownloadId,
    ) -> StorageResult<Option<crate::downloads::DownloadRecord>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        downloads::get(&conn, id)
    }

    fn delete_download(&self, id: wis_core::DownloadId) -> StorageResult<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        downloads::delete(&conn, id)
    }

    fn clear_downloads(&self) -> StorageResult<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        downloads::clear(&conn)
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

    fn clear_credentials(&self) -> StorageResult<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        credentials::clear(&conn)
    }

    fn has_credentials(&self) -> StorageResult<bool> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        credentials::any(&conn)
    }

    fn save_address(&self, id: Option<i64>, ciphertext: &[u8], nonce: &[u8]) -> StorageResult<i64> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        addresses::save(&conn, id, ciphertext, nonce)
    }

    fn list_addresses(&self) -> StorageResult<Vec<crate::addresses::AddressRecord>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        addresses::list(&conn)
    }

    fn delete_address(&self, id: i64) -> StorageResult<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        addresses::delete(&conn, id)
    }

    fn clear_addresses(&self) -> StorageResult<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        addresses::clear(&conn)
    }

    fn has_addresses(&self) -> StorageResult<bool> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        addresses::any(&conn)
    }

    fn clear_vault(&self) -> StorageResult<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        let tx = conn.unchecked_transaction()?;
        credentials::clear(&tx)?;
        addresses::clear(&tx)?;
        tx.commit()?;
        Ok(())
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
        bookmarks::add_bookmark(
            &conn,
            workspace_id,
            folder_id,
            url,
            title,
            favicon_hash,
            pinned,
        )
    }

    fn import_bookmarks(
        &self,
        workspace_id: wis_core::WorkspaceId,
        folders: &[String],
        entries: &[crate::bookmarks::BookmarkImport],
    ) -> StorageResult<crate::bookmarks::BookmarkImportStats> {
        let mut conn = self.conn.lock().expect("storage mutex poisoned");
        bookmarks::import(&mut conn, workspace_id, folders, entries)
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

    fn save_tab_session(&self, tab_id: wis_core::TabId, data: &str) -> StorageResult<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        let id = tab_id.as_uuid().to_string();
        let now = chrono::Utc::now();
        conn.execute(
            "INSERT INTO tab_sessions (tab_id, data, updated_at) VALUES (?1, ?2, ?3) \
             ON CONFLICT(tab_id) DO UPDATE SET data = ?2, updated_at = ?3",
            rusqlite::params![id, data, now],
        )?;

        Ok(())
    }

    fn load_tab_sessions(
        &self,
    ) -> StorageResult<std::collections::HashMap<wis_core::TabId, String>> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        let mut stmt = conn.prepare("SELECT tab_id, data FROM tab_sessions")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut out = std::collections::HashMap::new();
        for row in rows {
            let (id_str, data) = row?;
            // Drop any row whose id no longer parses.
            if let Ok(uuid) = uuid::Uuid::parse_str(&id_str) {
                out.insert(wis_core::TabId::from(uuid), data);
            }
        }

        Ok(out)
    }

    fn delete_tab_session(&self, tab_id: wis_core::TabId) -> StorageResult<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        conn.execute(
            "DELETE FROM tab_sessions WHERE tab_id = ?1",
            rusqlite::params![tab_id.as_uuid().to_string()],
        )?;

        Ok(())
    }

    fn purge_tab_sessions_except(&self, keep: &[wis_core::TabId]) -> StorageResult<()> {
        let conn = self.conn.lock().expect("storage mutex poisoned");
        if keep.is_empty() {
            conn.execute("DELETE FROM tab_sessions", [])?;
            return Ok(());
        }

        let placeholders = vec!["?"; keep.len()].join(",");
        let sql = format!("DELETE FROM tab_sessions WHERE tab_id NOT IN ({placeholders})");
        let ids: Vec<String> = keep.iter().map(|id| id.as_uuid().to_string()).collect();
        conn.execute(&sql, rusqlite::params_from_iter(ids.iter()))?;

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
