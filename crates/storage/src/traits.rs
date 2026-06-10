//! The [`Storage`] trait: the contract the rest of the app codes against.

use crate::bookmarks::{Bookmark, BookmarkFolder};
use crate::credentials::CredentialRecord;
use crate::downloads::DownloadRecord;
use crate::error::StorageResult;
use crate::history::{HistoryEntry, HistoryQuery};
use crate::searches::SearchEntry;
use wis_core::{BrowserState, DownloadId, WorkspaceId};

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

    /// Remove history rows, scoped to `workspace_id` when set, else all of it.
    fn clear_history(&self, workspace_id: Option<WorkspaceId>) -> StorageResult<()>;

    // -- password vault (see [`crate::credentials`]) -----------------------

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
}
