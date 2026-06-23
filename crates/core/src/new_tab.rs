//! New-tab launchpad configuration: where a fresh tab lands and the editable
//! shortcut tiles shown on the launchpad.
//!
//! All validation lives here as pure functions so the IPC boundary can
//! sanitize untrusted input (a tile URL is shown and navigated to) without
//! pulling logic into the shell.

use serde::{Deserialize, Serialize};
use url::Url;

/// Most shortcut tiles the launchpad renders (one row).
pub const MAX_SHORTCUTS: usize = 8;

const MAX_LABEL_LEN: usize = 40;
const MAX_URL_LEN: usize = 2048;

/// What a freshly opened tab lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NewTabTarget {
    /// The built-in launchpad (search + shortcut tiles).
    #[default]
    Launchpad,
    /// The user's homepage URL ([`crate::UserSettings::homepage`]); falls back
    /// to the launchpad when no valid homepage is set.
    Homepage,
}

/// A launchpad shortcut tile: a label and the http(s) URL it opens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShortcutTile {
    pub label: String,
    pub url: String,
}

impl ShortcutTile {
    /// Validate and normalize a tile: the URL must be http(s); the label is
    /// trimmed and, when empty, derived from the host. Returns `None` when the
    /// URL is unusable, so a hostile or malformed tile is dropped at the
    /// boundary rather than rendered.
    pub fn sanitized(&self) -> Option<Self> {
        let url = normalize_web_url(&self.url)?;
        if url.len() > MAX_URL_LEN {
            return None;
        }
        let label = clamp_label(self.label.trim(), &url);

        Some(Self { label, url })
    }
}

/// Default launchpad tiles, used until the user edits them.
pub fn default_shortcuts() -> Vec<ShortcutTile> {
    [
        ("DuckDuckGo", "https://duckduckgo.com/"),
        ("Wikipedia", "https://wikipedia.org/"),
        ("GitHub", "https://github.com/"),
        ("YouTube", "https://youtube.com/"),
        ("Hacker News", "https://news.ycombinator.com/"),
    ]
    .into_iter()
    .map(|(label, url)| ShortcutTile {
        label: label.to_string(),
        url: url.to_string(),
    })
    .collect()
}

/// Sanitize a boundary-supplied tile list: drop invalid tiles, dedupe by URL
/// (first wins), and cap at [`MAX_SHORTCUTS`].
pub fn sanitize_shortcuts(tiles: Vec<ShortcutTile>) -> Vec<ShortcutTile> {
    let mut seen: Vec<String> = Vec::new();
    let mut out: Vec<ShortcutTile> = Vec::new();
    for tile in tiles {
        let Some(tile) = tile.sanitized() else {
            continue;
        };

        if seen.iter().any(|u| u == &tile.url) {
            continue;
        }

        seen.push(tile.url.clone());
        out.push(tile);
        if out.len() >= MAX_SHORTCUTS {
            break;
        }
    }

    out
}

/// Normalize a homepage string to a usable http(s) URL, or `None` when empty
/// or invalid. Empty input means "no homepage" (fall back to the launchpad).
pub fn sanitize_homepage(raw: &str) -> Option<String> {
    normalize_web_url(raw)
}

/// Parse `input` as an http(s) URL, tolerating a missing scheme (assumes
/// https). Returns the canonical string, or `None` when it cannot be a web URL.
fn normalize_web_url(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    // An input that already parses as an absolute URL keeps its scheme: accept
    // only http(s), never re-scheme it. Re-scheming e.g. `mailto:a@b.com` to
    // `https://...` would smuggle a non-web target past the filter (host `b.com`,
    // userinfo `mailto:a`). Only a schemeless input gets an assumed `https://`.
    let parsed = match Url::parse(trimmed) {
        Ok(u) => u,
        Err(_) => Url::parse(&format!("https://{trimmed}")).ok()?,
    };

    if matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some() {
        Some(parsed.to_string())
    } else {
        None
    }
}

/// A non-empty label, capped in length; falls back to the URL's host.
fn clamp_label(label: &str, url: &str) -> String {
    let base = if label.is_empty() {
        host_label(url).unwrap_or_else(|| "Shortcut".to_string())
    } else {
        label.to_string()
    };

    base.chars().take(MAX_LABEL_LEN).collect()
}

fn host_label(url: &str) -> Option<String> {
    let host = Url::parse(url).ok()?.host_str()?.to_string();
    Some(host.strip_prefix("www.").unwrap_or(&host).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_all_valid_and_within_cap() {
        let defaults = default_shortcuts();
        assert!(defaults.len() <= MAX_SHORTCUTS);
        for tile in &defaults {
            assert_eq!(tile.sanitized().as_ref(), Some(tile));
        }
    }

    #[test]
    fn sanitize_drops_invalid_dedupes_and_caps() {
        let mut tiles = vec![
            ShortcutTile {
                label: " Example ".into(),
                url: "example.com".into(),
            },
            // Duplicate URL after normalization: dropped.
            ShortcutTile {
                label: "Dup".into(),
                url: "https://example.com/".into(),
            },
            // Non-web scheme: dropped.
            ShortcutTile {
                label: "Mail".into(),
                url: "mailto:a@b.com".into(),
            },
            // Empty label: derives from host.
            ShortcutTile {
                label: "".into(),
                url: "https://www.github.com/".into(),
            },
        ];
        // Pad past the cap with distinct valid tiles.
        for i in 0..MAX_SHORTCUTS + 4 {
            tiles.push(ShortcutTile {
                label: format!("S{i}"),
                url: format!("https://site{i}.example/"),
            });
        }

        let out = sanitize_shortcuts(tiles);
        assert_eq!(out.len(), MAX_SHORTCUTS, "capped");
        assert_eq!(out[0].label, "Example", "label trimmed");
        assert_eq!(out[0].url, "https://example.com/", "scheme added");
        assert_eq!(
            out[1].label, "github.com",
            "label derived from host, www stripped"
        );
        // No two tiles share a URL.
        let mut urls: Vec<&String> = out.iter().map(|t| &t.url).collect();
        urls.sort();
        urls.dedup();
        assert_eq!(urls.len(), out.len());
    }

    #[test]
    fn homepage_normalizes_and_rejects_non_web() {
        assert_eq!(
            sanitize_homepage("  example.com "),
            Some("https://example.com/".to_string())
        );
        assert_eq!(
            sanitize_homepage("http://x.test/path"),
            Some("http://x.test/path".to_string())
        );
        assert_eq!(sanitize_homepage(""), None);
        assert_eq!(sanitize_homepage("   "), None);
        assert_eq!(sanitize_homepage("javascript:alert(1)"), None);
        assert_eq!(sanitize_homepage("about:blank"), None);
    }
}
