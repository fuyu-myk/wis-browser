//! Bookmark records.
//!
//! Bookmarks are an *append-style* per-workspace store kept entirely in SQLite,
//! never folded into [`wis_core::BrowserState`]. They are organised into
//! optional folders; a bookmark with `folder_id == None` is "unfiled".

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use wis_core::WorkspaceId;

/// A bookmark folder within a workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookmarkFolder {
    pub id: i64,
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub position: i64,
    pub created_at: DateTime<Utc>,
}

/// A saved bookmark. `favicon_hash` mirrors the favicon-cache key so the UI can
/// render the icon without re-fetching.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: i64,
    pub workspace_id: WorkspaceId,
    /// Containing folder, or `None` when unfiled.
    pub folder_id: Option<i64>,
    pub url: String,
    pub title: String,
    pub favicon_hash: Option<String>,
    pub position: i64,
    /// Pinned to the quick-access row, independent of `folder_id`.
    pub pinned: bool,
    /// Order within the pinned row (separate from `position`).
    pub pin_position: i64,
    pub created_at: DateTime<Utc>,
}
