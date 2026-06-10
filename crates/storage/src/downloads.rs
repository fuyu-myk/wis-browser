//! Completed-download history records.
//!
//! An *append-only* store kept entirely in SQLite and never folded into
//! [`wis_core::BrowserState`], like browsing history. One row per finished
//! download; in-flight, failed, and cancelled downloads live only in the
//! shell's in-memory tracking. Global rather than per workspace (the files
//! all land in the same downloads directory). Downloads from private
//! (ephemeral) contexts are never written here.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use wis_core::DownloadId;

/// One completed download.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadRecord {
    pub id: DownloadId,
    pub url: String,
    pub filename: String,
    /// Absolute destination path at completion time. The file may have been
    /// moved or deleted since; consumers must validate before use.
    pub path: String,
    pub bytes: i64,
    pub completed_at: DateTime<Utc>,
}
