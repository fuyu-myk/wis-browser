//! Records of recently-closed tabs, kept so they can be reopened after a restart.
//!
//! An independent table like browsing history, never folded into
//! [`wis_core::BrowserState`]. One row per closed tab, newest first, capped by
//! the caller. Tabs closed in a private (ephemeral) workspace are never
//! written here, and clearing browsing history clears these too.
//!
//! The live session blob a tab carries at close (WebKit's `interactionState`)
//! is deliberately **not** stored.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use wis_core::{TabFolderId, WorkspaceId};

/// Metadata of a closed tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClosedTabRecord {
    /// Row id. Ignored when recording.
    pub id: i64,
    pub workspace_id: WorkspaceId,
    pub url: String,
    /// Title at close. Empty when the page never reported one;
    /// callers fall back to the URL.
    pub title: String,
    /// The folder the tab lived in at close (`None` = loose).
    pub folder_id: Option<TabFolderId>,
    /// Position in the workspace's tab order at close. Clamped
    /// by the caller if the workspace has since shrunk.
    pub position: i64,
    pub pinned: bool,
    /// When the tab was archived for being idle, the last time it was used.
    pub closed_at: DateTime<Utc>,
    /// Closed by archiving rather than by the user.
    #[serde(default)]
    pub archived: bool,
}
