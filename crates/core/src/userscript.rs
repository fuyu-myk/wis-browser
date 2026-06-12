//! Tampermonkey-style user scripts: user-supplied JavaScript injected into
//! pages whose URL matches the script's `@match`/`@include` patterns.
//!
//! This module is the pure core: the metadata-block parser
//! ([`parse_metadata`]) and the URL matcher ([`Userscript::matches_url`]).
//! The shell injects matching scripts at the script's `@run-at` time.
//!
//! Scope note: GM_* APIs are not provided (no privileged bridge), so scripts
//! run in the page world like a normal injected `<script>`. DOM/CSS tweaks
//! and self-contained logic work; scripts depending on `GM_xmlhttpRequest`
//! et al. do not.

use serde::{Deserialize, Serialize};

use crate::error::{WisError, WisResult};
use crate::ids::UserscriptId;

pub const MAX_NAME_BYTES: usize = 120;
pub const MAX_SOURCE_BYTES: usize = 512 * 1024;
pub const MAX_PATTERNS: usize = 100;

/// When a script is injected relative to document load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum RunAt {
    /// Before the page's own scripts (`@run-at document-start`).
    DocumentStart,
    /// After the DOM is parsed (`@run-at document-end`, the default).
    #[default]
    DocumentEnd,
    /// After the page finishes loading (`@run-at document-idle`); treated as
    /// document-end here.
    DocumentIdle,
}

/// A stored user script.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Userscript {
    pub id: UserscriptId,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// `@match` + `@include` URL patterns (glob: `*` is any run of chars).
    /// Empty means the script never runs (no implicit match-all).
    #[serde(default)]
    pub matches: Vec<String>,
    /// `@exclude` patterns; a URL matching any of these is skipped.
    #[serde(default)]
    pub excludes: Vec<String>,
    #[serde(default)]
    pub run_at: RunAt,
    /// The full script source (including its metadata block).
    pub source: String,
}

fn default_true() -> bool {
    true
}

impl Userscript {
    /// Build a script from raw `.user.js` source, parsing its metadata block.
    pub fn from_source(source: &str) -> WisResult<Self> {
        if source.len() > MAX_SOURCE_BYTES {
            return Err(WisError::InvalidInput {
                message: "user script exceeds size limit".to_string(),
            });
        }

        let meta = parse_metadata(source);
        let name = meta
            .name
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "Untitled script".to_string());
        let name = name.chars().take(MAX_NAME_BYTES).collect::<String>();
        let matches: Vec<String> = meta.matches.into_iter().take(MAX_PATTERNS).collect();
        let excludes: Vec<String> = meta.excludes.into_iter().take(MAX_PATTERNS).collect();

        Ok(Self {
            id: UserscriptId::new(),
            name,
            enabled: true,
            matches,
            excludes,
            run_at: meta.run_at,
            source: source.to_string(),
        })
    }

    pub fn validate(&self) -> WisResult<()> {
        if self.name.trim().is_empty() || self.name.len() > MAX_NAME_BYTES {
            return Err(WisError::InvalidInput {
                message: "user script name must be 1..=120 bytes".to_string(),
            });
        }
        if self.source.len() > MAX_SOURCE_BYTES {
            return Err(WisError::InvalidInput {
                message: "user script source exceeds size limit".to_string(),
            });
        }
        if self.matches.len() > MAX_PATTERNS || self.excludes.len() > MAX_PATTERNS {
            return Err(WisError::InvalidInput {
                message: "too many match/exclude patterns".to_string(),
            });
        }

        Ok(())
    }

    /// Whether this (enabled) script should run on `url`.
    pub fn matches_url(&self, url: &str) -> bool {
        if !self.enabled || self.matches.is_empty() {
            return false;
        }
        if self.excludes.iter().any(|p| glob_match(p, url)) {
            return false;
        }

        self.matches.iter().any(|p| glob_match(p, url))
    }
}

/// Parsed contents of a `==UserScript==` metadata block.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ParsedMetadata {
    pub name: Option<String>,
    pub matches: Vec<String>,
    pub excludes: Vec<String>,
    pub run_at: RunAt,
}

/// Parse the `// ==UserScript== ... // ==/UserScript==` header. Unknown keys
/// are ignored; a missing block yields defaults (no matches, document-end).
pub fn parse_metadata(source: &str) -> ParsedMetadata {
    let mut meta = ParsedMetadata::default();
    let mut in_block = false;
    for line in source.lines() {
        let line = line.trim();
        if line.contains("==UserScript==") {
            in_block = true;
            continue;
        }
        if line.contains("==/UserScript==") {
            break;
        }
        if !in_block {
            continue;
        }
        // Lines look like `// @key   value`.
        let Some(rest) = line.strip_prefix("//") else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix('@') else {
            continue;
        };
        let mut parts = rest.splitn(2, char::is_whitespace);
        let key = parts.next().unwrap_or("").trim();
        let value = parts.next().unwrap_or("").trim().to_string();

        match key {
            "name" if meta.name.is_none() => meta.name = Some(value),
            "match" | "include" if !value.is_empty() => meta.matches.push(value),
            "exclude" if !value.is_empty() => meta.excludes.push(value),
            "run-at" => {
                meta.run_at = match value.as_str() {
                    "document-start" => RunAt::DocumentStart,
                    "document-idle" => RunAt::DocumentIdle,
                    _ => RunAt::DocumentEnd,
                }
            }
            _ => {}
        }
    }

    meta
}

/// Glob match where `*` matches any (possibly empty) run of characters and
/// everything else is literal. `*` alone matches anything. Pragmatic
/// stand-in for Chrome match patterns + `@include` globs: imprecise on
/// host/path boundaries but adequate for the common cases, and this is a
/// power-user surface.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    if pattern == "*" || pattern == "<all_urls>" {
        return true;
    }

    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == text; // no wildcard: exact
    }

    let mut pos = 0usize;
    // First segment must be a prefix.
    let first = parts[0];
    if !text[pos..].starts_with(first) {
        return false;
    }

    pos += first.len();
    // Last segment must be a suffix (checked after the middle scan).
    let last = parts[parts.len() - 1];
    // Middle segments must appear in order.
    for seg in &parts[1..parts.len() - 1] {
        if seg.is_empty() {
            continue;
        }
        match text[pos..].find(seg) {
            Some(idx) => pos += idx + seg.len(),
            None => return false,
        }
    }

    text[pos..].len() >= last.len() && text.ends_with(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_matches_expected() {
        assert!(glob_match("*", "https://anything/x"));
        assert!(glob_match(
            "*://*.youtube.com/*",
            "https://www.youtube.com/watch?v=1"
        ));
        assert!(glob_match(
            "https://example.com/*",
            "https://example.com/page"
        ));
        assert!(!glob_match(
            "https://example.com/*",
            "https://other.com/page"
        ));
        assert!(glob_match("*reddit.com*", "https://old.reddit.com/r/x"));
        assert!(!glob_match("https://a.com/", "https://a.com/b"));
    }

    #[test]
    fn parses_metadata_block() {
        let src = "// ==UserScript==\n// @name  My Script\n// @match *://*.example.com/*\n// @exclude *://example.com/admin*\n// @run-at document-start\n// ==/UserScript==\nconsole.log(1);";
        let meta = parse_metadata(src);
        assert_eq!(meta.name.as_deref(), Some("My Script"));
        assert_eq!(meta.matches, vec!["*://*.example.com/*".to_string()]);
        assert_eq!(meta.excludes, vec!["*://example.com/admin*".to_string()]);
        assert_eq!(meta.run_at, RunAt::DocumentStart);
    }

    #[test]
    fn from_source_defaults_and_matching() {
        let src = "// ==UserScript==\n// @match https://site.test/*\n// ==/UserScript==\n";
        let us = Userscript::from_source(src).unwrap();
        assert_eq!(us.name, "Untitled script");
        assert_eq!(us.run_at, RunAt::DocumentEnd);
        assert!(us.enabled);
        assert!(us.matches_url("https://site.test/page"));
        assert!(!us.matches_url("https://other.test/page"));
    }

    #[test]
    fn no_matches_never_runs() {
        let mut us = Userscript::from_source("// ==UserScript==\n// ==/UserScript==\n").unwrap();
        us.matches.clear();
        assert!(!us.matches_url("https://anywhere/"));
    }

    #[test]
    fn disabled_never_runs() {
        let mut us =
            Userscript::from_source("// ==UserScript==\n// @match *\n// ==/UserScript==\n")
                .unwrap();
        us.enabled = false;
        assert!(!us.matches_url("https://anywhere/"));
    }

    #[test]
    fn validate_rejects_empty_name() {
        let mut us =
            Userscript::from_source("// ==UserScript==\n// @match *\n// ==/UserScript==\n")
                .unwrap();
        us.name = "   ".to_string();
        assert!(us.validate().is_err());
    }
}
