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
    /// Whether the page has a previous/next history entry. In-memory only:
    /// history lives in the live webview, so both reconstruct `false` until
    /// the restored page's first load reports the real values.
    #[serde(default)]
    pub can_go_back: bool,
    #[serde(default)]
    pub can_go_forward: bool,
    /// The page's load failure when the last navigation failed (DNS, refused,
    /// timeout, TLS). In-memory only: the chrome shows an error page in the
    /// tab's place, cleared on the next load. The storage layer reconstructs
    /// `None`.
    #[serde(default)]
    pub load_error: Option<crate::navigation::load_error::LoadError>,
    pub created_at: DateTime<Utc>,
    pub last_active_at: DateTime<Utc>,
}

/// Placeholder title for a page that has not reported one: the bare site name
/// (`www.`-stripped host), so an untitled page reads as "example.com" in the
/// tab strip, tooltips, and history. Non-host URLs (about:) fall back to the
/// URL itself.
pub fn placeholder_title(url: &Url) -> String {
    url.host_str()
        .map(|h| h.strip_prefix("www.").unwrap_or(h).to_string())
        .unwrap_or_else(|| url.as_str().to_string())
}

/// Whether a closed tab at `url` goes to Recently closed: web pages, Wis pages
/// and canvases, but not a blank new tab.
pub fn reopenable_after_close(url: &Url) -> bool {
    match url.scheme() {
        "http" | "https" => true,
        "about" => url.path() != "blank",
        scheme => scheme == crate::canvas::board::SCHEME,
    }
}

impl Tab {
    pub fn new(workspace_id: WorkspaceId, url: Url) -> Self {
        let now = Utc::now();
        Self {
            id: TabId::new(),
            workspace_id,
            title: placeholder_title(&url),
            url,
            favicon: None,
            pinned: false,
            folder_id: None,
            muted: false,
            loading: false,
            reader: false,
            load_error: None,
            can_go_back: false,
            can_go_forward: false,
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

    pub fn with_loading(&self, loading: bool) -> Self {
        Self {
            loading,
            ..self.clone()
        }
    }

    pub fn with_load_error(
        &self,
        load_error: Option<crate::navigation::load_error::LoadError>,
    ) -> Self {
        Self {
            load_error,
            ..self.clone()
        }
    }

    pub fn with_reader(&self, reader: bool) -> Self {
        Self {
            reader,
            ..self.clone()
        }
    }

    pub fn with_nav_state(&self, can_go_back: bool, can_go_forward: bool) -> Self {
        Self {
            can_go_back,
            can_go_forward,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_title_is_the_bare_site_name() {
        let url = Url::parse("https://www.example.com/some/long/path?q=1#frag").unwrap();
        assert_eq!(placeholder_title(&url), "example.com");

        let no_www = Url::parse("https://docs.rs/tokio/latest").unwrap();
        assert_eq!(placeholder_title(&no_www), "docs.rs");
    }

    #[test]
    fn placeholder_title_falls_back_to_the_url_without_a_host() {
        let url = Url::parse("about:blank").unwrap();
        assert_eq!(placeholder_title(&url), "about:blank");
    }

    #[test]
    fn web_pages_wis_pages_and_canvases_can_be_reopened() {
        for url in [
            "https://example.com/",
            "http://localhost:8080/",
            "about:settings",
            "about:history",
            "wis-canvas://localhost/?canvas=0b9f5d6e-8f43-4a5e-9d8c-3c2f1d7a6b5e",
        ] {
            assert!(reopenable_after_close(&Url::parse(url).unwrap()), "{url}");
        }
    }

    #[test]
    fn blank_tabs_and_other_schemes_are_not_reopened() {
        for url in [
            "about:blank",
            "about:blank#top",
            "about:blank?x",
            "file:///etc/hosts",
            "data:text/plain,hi",
        ] {
            assert!(!reopenable_after_close(&Url::parse(url).unwrap()), "{url}");
        }
    }

    #[test]
    fn new_tab_seeds_the_placeholder_title() {
        let url = Url::parse("https://www.example.com/deep/path").unwrap();
        let tab = Tab::new(crate::WorkspaceId::new(), url);
        assert_eq!(tab.title, "example.com");
    }
}
