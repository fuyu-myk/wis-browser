//! Tab — a single open page inside a workspace. Immutable: all "mutations"
//! return a new value.

use crate::ids::{TabId, WorkspaceId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use url::Url;

/// A reference to a cached favicon. Stores only a content-addressed hash;
/// the actual bytes live in the favicon cache.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FaviconRef {
    pub hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tab {
    pub id: TabId,
    pub workspace_id: WorkspaceId,
    pub url: Url,
    pub title: String,
    pub favicon: Option<FaviconRef>,
    pub pinned: bool,
    pub muted: bool,
    pub created_at: DateTime<Utc>,
    pub last_active_at: DateTime<Utc>,
}

impl Tab {
    pub fn new(workspace_id: WorkspaceId, url: Url) -> Self {
        let now = Utc::now();
        Self {
            id: TabId::new(),
            workspace_id,
            title: url.as_str().to_string(),
            url,
            favicon: None,
            pinned: false,
            muted: false,
            created_at: now,
            last_active_at: now,
        }
    }

    pub fn with_url(&self, url: Url) -> Self {
        Self {
            url,
            ..self.clone()
        }
    }

    pub fn with_title(&self, title: String) -> Self {
        Self {
            title,
            ..self.clone()
        }
    }

    pub fn with_favicon(&self, favicon: Option<FaviconRef>) -> Self {
        Self {
            favicon,
            ..self.clone()
        }
    }

    pub fn with_pinned(&self, pinned: bool) -> Self {
        Self {
            pinned,
            ..self.clone()
        }
    }

    pub fn with_muted(&self, muted: bool) -> Self {
        Self {
            muted,
            ..self.clone()
        }
    }

    /// Refresh `last_active_at` to "now". Used when a tab is focused.
    pub fn touched(&self) -> Self {
        Self {
            last_active_at: Utc::now(),
            ..self.clone()
        }
    }
}
