//! Address-bar search-history records (the non-URL queries the user typed).
//! Deduplicated by query text; powers the URL-bar suggestions' search section.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// One remembered search query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchEntry {
    pub id: i64,
    pub query: String,
    pub searched_at: DateTime<Utc>,
    pub count: i64,
}
