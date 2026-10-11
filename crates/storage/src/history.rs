//! Browsing-history records.
//!
//! History is an *append-style* store kept entirely in SQLite and never folded
//! into [`wis_core::BrowserState`]. Rows are deduplicated by URL: revisiting a
//! page bumps its `visit_count` and refreshes `visited_at`/`title`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use wis_core::WorkspaceId;

/// One deduplicated history row (one per URL *per workspace*).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: i64,
    pub workspace_id: WorkspaceId,
    pub url: String,
    pub title: String,
    pub visited_at: DateTime<Utc>,
    pub visit_count: i64,
}

/// A visited URL read from another browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryImport {
    pub url: String,
    pub title: String,
    pub visited_at: DateTime<Utc>,
    pub visit_count: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryImportStats {
    pub added: usize,
    pub merged: usize,
}

/// Query parameters for [`crate::Storage::query_history`]: a case-insensitive
/// substring match over url + title (empty matches everything), newest first,
/// with pagination, optionally scoped to a single workspace.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HistoryQuery {
    #[serde(default)]
    pub search: String,
    /// Restrict to one workspace; `None` returns history across all workspaces.
    #[serde(default)]
    pub workspace_id: Option<WorkspaceId>,
    /// Maximum rows to return. `0` applies [`HistoryQuery::DEFAULT_LIMIT`].
    #[serde(default)]
    pub limit: u32,
    /// Rows to skip, for paging.
    #[serde(default)]
    pub offset: u32,
}

impl HistoryQuery {
    /// Cap applied when `limit == 0`, so an unbounded query can't load an
    /// unbounded result set into memory.
    pub const DEFAULT_LIMIT: u32 = 200;

    /// The effective row cap for this query.
    pub fn effective_limit(&self) -> u32 {
        if self.limit == 0 {
            Self::DEFAULT_LIMIT
        } else {
            self.limit
        }
    }
}
