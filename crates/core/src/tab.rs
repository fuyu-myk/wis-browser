//! Tab: a single open page inside a workspace. Immutable: all "mutations"
//! return a new value.

use crate::ids::{TabFolderId, TabId, WorkspaceId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use url::Url;

/// A reference to a cached favicon. Stores only a content-addressed hash;
/// the actual bytes live in the favicon cache.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FaviconRef {
    pub hash: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tab {
    pub id: TabId,
    pub workspace_id: WorkspaceId,
    pub url: Url,
    pub title: String,
    pub favicon: Option<FaviconRef>,
    pub pinned: bool,
    /// Containing tab folder, or `None` when the tab is loose. Pinned tabs are
    /// always loose. Serde-defaults to `None` so existing state loads unchanged.
    #[serde(default)]
    pub folder_id: Option<TabFolderId>,
    pub muted: bool,
    /// Whether the tab's page is currently loading. In-memory only:
    /// not persisted. On cold start no tab can legitimately be
    /// mid-load, so the storage layer always reconstructs `loading: false`.
    #[serde(default)]
    pub loading: bool,
    /// Whether the tab is currently showing reader mode (the in-page article
    /// transform). In-memory only: the transform lives in the live webview and
    /// is lost on reload/restart, so the storage layer reconstructs `false`.
    #[serde(default)]
    pub reader: bool,
    /// Persisted page-zoom factor (1.0 = 100%).
    #[serde(default = "default_zoom")]
    pub zoom: f64,
    pub created_at: DateTime<Utc>,
    pub last_active_at: DateTime<Utc>,
}

/// Default page-zoom factor (100%).
pub fn default_zoom() -> f64 {
    1.0
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
            folder_id: None,
            muted: false,
            loading: false,
            reader: false,
            zoom: default_zoom(),
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

    pub fn with_folder(&self, folder_id: Option<TabFolderId>) -> Self {
        Self {
            folder_id,
            ..self.clone()
        }
    }

    pub fn with_muted(&self, muted: bool) -> Self {
        Self {
            muted,
            ..self.clone()
        }
    }

    pub fn with_zoom(&self, zoom: f64) -> Self {
        Self {
            zoom,
            ..self.clone()
        }
    }

    pub fn with_loading(&self, loading: bool) -> Self {
        Self {
            loading,
            ..self.clone()
        }
    }

    pub fn with_reader(&self, reader: bool) -> Self {
        Self {
            reader,
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
