//! The [`Storage`] trait: the contract the rest of the app codes against.

use crate::error::StorageResult;
use crate::history::{HistoryEntry, HistoryQuery};
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
    fn record_visit(
        &self,
        workspace_id: WorkspaceId,
        url: &str,
        title: &str,
    ) -> StorageResult<()>;

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
}
