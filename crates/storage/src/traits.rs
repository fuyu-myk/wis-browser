//! The [`Storage`] trait: the contract the rest of the app codes against.

use crate::credentials::CredentialRecord;
use crate::error::StorageResult;
use crate::history::{HistoryEntry, HistoryQuery};
use crate::searches::SearchEntry;
use wis_core::{BrowserState, WorkspaceId};

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
}
