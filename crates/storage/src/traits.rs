//! The [`Storage`] trait: the contract the rest of the app codes against.

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::addresses::AddressRecord;
use crate::bookmarks::{Bookmark, BookmarkFolder, BookmarkImport, BookmarkImportStats};
use crate::closed_tabs::ClosedTabRecord;
use crate::credentials::CredentialRecord;
use crate::downloads::DownloadRecord;
use crate::error::StorageResult;
use crate::history::{HistoryEntry, HistoryImport, HistoryImportStats, HistoryQuery};
use crate::searches::SearchEntry;
use wis_core::canvas::{CanvasDocUpdate, CanvasRecord, CanvasSummary};
use wis_core::{BrowserState, CanvasId, DownloadId, TabId, WorkspaceId};

/// A pluggable persistent store for the browser's state.
///
/// Implementations must be safe to share across threads (`Send + Sync`).
/// `load_state` returns `Ok(None)` when there is no persisted state yet
/// (first launch, or after a wipe), so the caller can fall back to
/// [`BrowserState::new_with_defaults`] without conflating "empty" with
/// "broken".
pub trait Storage: Send + Sync {
    /// Read the persisted state. Returns `Ok(None)` when nothing has been
    /// saved yet. The implementation is responsible for verifying invariants
    /// before returning the state.
    fn load_state(&self) -> StorageResult<Option<BrowserState>>;

    /// Persist the full state atomically. Replaces any prior persisted
    /// contents.
    fn save_state(&self, state: &BrowserState) -> StorageResult<()>;

    /// Persist the set of workspaces that each had their own OS window when the
    /// app last exited, so the multi-window layout can be reopened on launch.
    /// Stored separately from [`Storage::save_state`] (it is the runtime window
    /// state, not part of [`BrowserState`]). Default no-op for non-persisting
    /// storages.
    fn save_windowed_workspaces(&self, _ids: &[WorkspaceId]) -> StorageResult<()> {
        Ok(())
    }

    /// Read the workspaces to reopen in their own windows on launch (see
    /// [`Storage::save_windowed_workspaces`]). Empty when nothing was saved.
    fn load_windowed_workspaces(&self) -> StorageResult<Vec<WorkspaceId>> {
        Ok(Vec::new())
    }

    /// Set an app-managed key in the small meta key-value store (survives a full
    /// state save). The caller owns the value encoding (e.g. JSON). Default
    /// no-op for non-persisting storages.
    fn set_meta(&self, _key: &str, _value: &str) -> StorageResult<()> {
        Ok(())
    }

    /// Read a meta key set via [`Storage::set_meta`]; `None` when absent.
    fn get_meta(&self, _key: &str) -> StorageResult<Option<String>> {
        Ok(None)
    }

    // -- browsing history (append-style; see [`crate::history`]) ------------

    /// Record a visit to `url` with `title` in `workspace_id`. Deduplicated by
    /// `(workspace_id, url)`: a repeat visit bumps `visit_count` and refreshes
    /// `visited_at`; a non-empty `title` replaces the stored one. Callers must
    /// skip private/ephemeral workspaces and non-recordable URLs (chrome pages,
    /// non-http(s)).
    fn record_visit(&self, workspace_id: WorkspaceId, url: &str, title: &str) -> StorageResult<()>;

    /// Update the title of an existing visit (matched by workspace + url)
    /// without changing its visit count. No-op for an empty title or a URL not
    /// already recorded. For late-arriving page titles.
    fn update_visit_title(
        &self,
        workspace_id: WorkspaceId,
        url: &str,
        title: &str,
    ) -> StorageResult<()>;

    /// Query history newest-first, filtered/paginated by `query` (and scoped to
    /// one workspace when `query.workspace_id` is set).
    fn query_history(&self, query: &HistoryQuery) -> StorageResult<Vec<HistoryEntry>>;

    /// Delete the history rows with the given ids. Unknown ids are ignored.
    fn delete_history_entries(&self, ids: &[i64]) -> StorageResult<()>;

    /// The URLs of the given history rows.
    fn history_urls(&self, ids: &[i64]) -> StorageResult<Vec<String>>;

    /// Remove history rows, scoped to `workspace_id` when set, else all of it.
    /// With `since`, only visits at or after that instant go, while older ones stay.
    fn clear_history(
        &self,
        workspace_id: Option<WorkspaceId>,
        since: Option<DateTime<Utc>>,
    ) -> StorageResult<()>;

    /// Merge visits read from another browser in one transaction. A URL already
    /// recorded keeps the later visit, the higher count and its own title, so
    /// importing the same data twice changes nothing.
    fn import_history(
        &self,
        workspace_id: WorkspaceId,
        entries: &[HistoryImport],
    ) -> StorageResult<HistoryImportStats>;

    // -- canvases (see `wis_core::canvas`) ------------------------------------

    /// A workspace's canvases, most recently changed first.
    fn list_canvases(&self, workspace_id: WorkspaceId) -> StorageResult<Vec<CanvasSummary>>;

    fn load_canvas(
        &self,
        workspace_id: WorkspaceId,
        id: CanvasId,
    ) -> StorageResult<Option<CanvasRecord>>;

    fn insert_canvas(&self, canvas: &CanvasRecord) -> StorageResult<()>;

    /// True when the canvas exists in that workspace.
    fn rename_canvas(
        &self,
        workspace_id: WorkspaceId,
        id: CanvasId,
        name: &str,
        updated_at: i64,
    ) -> StorageResult<bool>;

    /// Write newer documents in one transaction; returns the canvases that still
    /// existed to take them.
    fn save_canvas_docs(&self, docs: &[CanvasDocUpdate]) -> StorageResult<Vec<CanvasId>>;

    /// True when the canvas existed in that workspace.
    fn delete_canvas(&self, workspace_id: WorkspaceId, id: CanvasId) -> StorageResult<bool>;

    fn delete_workspace_canvases(&self, workspace_id: WorkspaceId) -> StorageResult<()>;

    // -- site usage (see `crate::usage`) ------------------------------------

    /// Add active seconds; credits for the same workspace, day, hour and site sum.
    fn add_usage(&self, credits: &[wis_core::usage::Credit]) -> StorageResult<()>;

    /// Per day, hour and site within `from..=to`, summed across workspaces
    /// when `workspace_id` is `None`.
    fn usage(
        &self,
        workspace_id: Option<WorkspaceId>,
        from: chrono::NaiveDate,
        to: chrono::NaiveDate,
    ) -> StorageResult<Vec<wis_core::usage::UsageRow>>;

    /// Scoped to `workspace_id` when set; with `from`, only that local day
    /// and hour onward go.
    fn clear_usage(
        &self,
        workspace_id: Option<WorkspaceId>,
        from: Option<(chrono::NaiveDate, u8)>,
    ) -> StorageResult<()>;

    // -- password vault (see `crate::credentials`) -------------------------

    /// Upsert a credential (dedup by host+username); returns its row id. The
    /// `ciphertext`/`nonce` are produced by `wis-secrets`; this layer never
    /// sees the plaintext or the key.
    fn save_credential(
        &self,
        host: &str,
        username: &str,
        ciphertext: &[u8],
        nonce: &[u8],
    ) -> StorageResult<i64>;

    /// List credentials, optionally filtered to one host.
    fn list_credentials(&self, host: Option<&str>) -> StorageResult<Vec<CredentialRecord>>;

    /// Fetch one credential by id.
    fn get_credential(&self, id: i64) -> StorageResult<Option<CredentialRecord>>;

    /// Delete one credential by id.
    fn delete_credential(&self, id: i64) -> StorageResult<()>;

    /// Delete every credential.
    fn clear_credentials(&self) -> StorageResult<()>;

    fn has_credentials(&self) -> StorageResult<bool>;

    // -- saved addresses (see `crate::addresses`) ---------------------------

    /// Insert an address, or replace the one with `id`; returns its row id.
    fn save_address(&self, id: Option<i64>, ciphertext: &[u8], nonce: &[u8]) -> StorageResult<i64>;

    fn list_addresses(&self) -> StorageResult<Vec<AddressRecord>>;

    fn delete_address(&self, id: i64) -> StorageResult<()>;

    fn clear_addresses(&self) -> StorageResult<()>;

    fn has_addresses(&self) -> StorageResult<bool>;

    /// Delete every credential and address in one transaction.
    fn clear_vault(&self) -> StorageResult<()>;

    // -- URL-bar suggestions ------------------------------------------------

    /// Record an address-bar search query (deduped by text; bumps its count).
    fn record_search(&self, query: &str) -> StorageResult<()>;

    /// Past searches matching `query`, most-used/recent first.
    fn suggest_searches(&self, query: &str, limit: u32) -> StorageResult<Vec<SearchEntry>>;

    /// The most recent searches, newest first (empty address-bar dropdown).
    fn recent_searches(&self, limit: u32) -> StorageResult<Vec<SearchEntry>>;

    /// Forget a remembered search query.
    fn delete_search(&self, query: &str) -> StorageResult<()>;

    /// Revisit-worthy sites for `query` in a workspace, ranked by visit count
    /// then recency (the "Wis Recommendations" section).
    fn recommend_sites(
        &self,
        workspace_id: WorkspaceId,
        query: &str,
        limit: u32,
    ) -> StorageResult<Vec<HistoryEntry>>;

    // -- bookmarks (per-workspace, foldered; see [`crate::bookmarks`]) -------

    /// Folders in a workspace, ordered by position.
    fn list_bookmark_folders(
        &self,
        workspace_id: WorkspaceId,
    ) -> StorageResult<Vec<BookmarkFolder>>;

    /// Create a folder appended after the workspace's existing folders.
    fn create_bookmark_folder(
        &self,
        workspace_id: WorkspaceId,
        name: &str,
    ) -> StorageResult<BookmarkFolder>;

    /// Rename a folder.
    fn rename_bookmark_folder(&self, id: i64, name: &str) -> StorageResult<()>;

    /// Reorder folders within a workspace (each id's `position` = its index).
    fn reorder_bookmark_folders(&self, ids: &[i64]) -> StorageResult<()>;

    /// Delete a folder, re-parenting its bookmarks to unfiled.
    fn delete_bookmark_folder(&self, id: i64) -> StorageResult<()>;

    /// All bookmarks in a workspace (UI groups them by `folder_id`).
    fn list_bookmarks(&self, workspace_id: WorkspaceId) -> StorageResult<Vec<Bookmark>>;

    /// Add a bookmark to `folder_id` (or unfiled when `None`); returns it.
    /// `pinned` also places it in the quick-access row.
    fn add_bookmark(
        &self,
        workspace_id: WorkspaceId,
        folder_id: Option<i64>,
        url: &str,
        title: &str,
        favicon_hash: Option<&str>,
        pinned: bool,
    ) -> StorageResult<Bookmark>;

    /// Bulk-insert imported bookmarks in one transaction (HTML import).
    /// Folder names are reused when the workspace already has a folder of
    /// that name, created otherwise; each entry's `folder` indexes into
    /// `folders` (an out-of-range index stores the entry unfiled). Entries
    /// whose (folder, url, title) already exist in the workspace are
    /// skipped. Imported rows append after existing ones, never pinned.
    fn import_bookmarks(
        &self,
        workspace_id: WorkspaceId,
        folders: &[String],
        entries: &[BookmarkImport],
    ) -> StorageResult<BookmarkImportStats>;

    /// Move a bookmark to a different folder (or unfiled). Keeps the pin flag.
    fn move_bookmark(&self, id: i64, folder_id: Option<i64>) -> StorageResult<()>;

    /// Pin/unpin a bookmark (independent of its folder). Pinning places it at
    /// the front of the quick-access row.
    fn set_bookmark_pinned(&self, id: i64, pinned: bool) -> StorageResult<()>;

    /// Reorder the pinned quick-access row (each id's `pin_position` = its index).
    fn reorder_pinned_bookmarks(&self, ids: &[i64]) -> StorageResult<()>;

    /// Reorder bookmarks within a group sharing a `folder_id` (a folder, or the
    /// unfiled list): each id's `position` = its index.
    fn reorder_bookmarks(&self, ids: &[i64]) -> StorageResult<()>;

    /// Delete a bookmark by id.
    fn delete_bookmark(&self, id: i64) -> StorageResult<()>;

    // -- recently-closed tabs (see `crate::closed_tabs`) -------------------

    /// Record a closed tab and return its row id. Callers must skip private
    /// (ephemeral) workspaces; `record.id` is ignored.
    fn record_closed_tab(&self, record: &ClosedTabRecord) -> StorageResult<i64>;

    /// [`Storage::record_closed_tab`] for several, in one transaction.
    fn record_closed_tabs(&self, records: &[ClosedTabRecord]) -> StorageResult<Vec<i64>>;

    /// Closed tabs, newest first, capped at `limit`.
    fn list_closed_tabs(&self, limit: u32) -> StorageResult<Vec<ClosedTabRecord>>;

    /// Remove one closed tab and return it. `None` when it is already gone.
    fn take_closed_tab(&self, id: i64) -> StorageResult<Option<ClosedTabRecord>>;

    /// Remove the most recently closed tab and return it.
    fn take_latest_closed_tab(&self) -> StorageResult<Option<ClosedTabRecord>>;

    /// Remove closed tabs whose URL is one of `urls`.
    fn delete_closed_tabs_by_urls(&self, urls: &[String]) -> StorageResult<()>;

    /// Drop all but the last `keep` closed tabs recorded.
    fn prune_closed_tabs(&self, keep: u32) -> StorageResult<()>;

    /// Remove closed tabs, scoped like [`Storage::clear_history`]: to one
    /// workspace when set, and to tabs closed at or after `since` when set.
    fn clear_closed_tabs(
        &self,
        workspace_id: Option<WorkspaceId>,
        since: Option<DateTime<Utc>>,
    ) -> StorageResult<()>;

    // -- downloads (append-only completed-download history; see `crate::downloads`)

    /// Record a completed download. Idempotent per id. Callers must skip
    /// downloads from private/ephemeral contexts.
    fn record_download(&self, record: &DownloadRecord) -> StorageResult<()>;

    /// Completed downloads, newest first, capped at `limit`.
    fn list_downloads(&self, limit: u32) -> StorageResult<Vec<DownloadRecord>>;

    /// Fetch one completed download by id.
    fn get_download(&self, id: DownloadId) -> StorageResult<Option<DownloadRecord>>;

    /// Remove one download from history (never touches the file). Unknown ids
    /// are ignored.
    fn delete_download(&self, id: DownloadId) -> StorageResult<()>;

    /// Remove the entire download history (never touches the files).
    fn clear_downloads(&self) -> StorageResult<()>;

    // -- session cookies (per-workspace; see migration M0011) --------------

    /// Persist a workspace's session-cookie snapshot (opaque blob). Replaces any
    /// prior snapshot's data but keeps its original `created_at` (so the absolute
    /// lifetime cap isn't reset by ongoing snapshots).
    fn save_session_cookies(&self, workspace_id: WorkspaceId, data: &str) -> StorageResult<()>;

    /// Load a workspace's session-cookie snapshot, or `None` if none saved or it
    /// was first persisted more than `max_age_days` ago (absolute TTL).
    fn load_session_cookies(
        &self,
        workspace_id: WorkspaceId,
        max_age_days: i64,
    ) -> StorageResult<Option<String>>;

    /// Delete every session-cookie snapshot first persisted more than
    /// `max_age_days` ago (`0` deletes all). Hygiene + blast-radius bound.
    fn purge_stale_session_cookies(&self, max_age_days: i64) -> StorageResult<()>;

    // -- per-tab session restore (interactionState; see migration M0018) ----

    /// Persist a tab's serialized WKWebView `interactionState` (base64), so its
    /// back/forward history + scroll restore across a full restart. Replaces any
    /// prior blob for the tab. Callers must skip ephemeral (private) tabs.
    /// Default no-op for non-persisting storages.
    fn save_tab_session(&self, _tab_id: TabId, _data: &str) -> StorageResult<()> {
        Ok(())
    }

    /// Load every persisted tab session, keyed by tab id. Empty when none.
    fn load_tab_sessions(&self) -> StorageResult<HashMap<TabId, String>> {
        Ok(HashMap::new())
    }

    /// Delete a tab's persisted session (e.g. when the tab is closed).
    fn delete_tab_session(&self, _tab_id: TabId) -> StorageResult<()> {
        Ok(())
    }

    /// Drop persisted sessions for tabs no longer present (orphan hygiene at
    /// startup). `keep` is the set of still-existing tab ids; an empty `keep`
    /// clears the table.
    fn purge_tab_sessions_except(&self, _keep: &[TabId]) -> StorageResult<()> {
        Ok(())
    }
}
