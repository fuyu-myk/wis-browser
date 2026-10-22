//! Tracker blocking using Brave's `adblock` engine.
//!
//! A [`TrackerBlocker`] is built once from a set of Adblock-Plus-syntax
//! filter lists (the bundled default list, optionally supplemented at
//! runtime). Parsing an EasyPrivacy-size list into an [`Engine`] costs real
//! startup CPU, so [`TrackerBlocker::from_filter_texts_with_cache`] memoizes
//! the compiled engine on disk (`engine-<hash>.bin` beside the filter-list
//! cache, keyed by a hash of the input texts) and deserializes on a hit:
//! launch becomes a file read. The DAT format self-versions; any mismatch
//! (crate upgrade, corruption) silently falls back to a fresh parse.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use adblock::lists::{FilterSet, ParseOptions};
use adblock::request::Request;
use adblock::Engine;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::{NetworkError, NetworkResult};
use crate::filter_lists::DEFAULT_TRACKER_LIST;

/// Cache file name for the serialized engine compiled from the lists.
fn engine_cache_file(texts: &[&str], trusted: &[&str]) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    texts.hash(&mut hasher);
    trusted.hash(&mut hasher);
    format!("engine-{:016x}.bin", hasher.finish())
}

/// The kind of subresource being requested. Lifted from the standard
/// Adblock Plus `$type` options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestType {
    /// Top-level navigation (a tab's main URL).
    Document,
    /// Iframe or other embedded document.
    Subdocument,
    Stylesheet,
    Script,
    Image,
    Font,
    Media,
    Xhr,
    WebSocket,
    Other,
}

impl RequestType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::Subdocument => "subdocument",
            Self::Stylesheet => "stylesheet",
            Self::Script => "script",
            Self::Image => "image",
            Self::Font => "font",
            Self::Media => "media",
            Self::Xhr => "xhr",
            Self::WebSocket => "websocket",
            Self::Other => "other",
        }
    }
}

/// Result of a single blocker decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockerVerdict {
    /// Request did not match any blocking rule.
    Allow,
    /// Request matched a blocking rule. `reason` is the filter rule text.
    Block { reason: String },
}

pub struct TrackerBlocker {
    engine: Engine,
}

/// Per-page cosmetic filtering payload (see [`TrackerBlocker::cosmetic_for_page`]).
pub struct CosmeticPage {
    /// `display:none` CSS for the host-specific hide selectors.
    pub hide_css: String,
    /// Selectors exempted on this host, consumed by the generic query.
    pub exceptions: std::collections::HashSet<String>,
    /// True when the page disables generic hiding (`#@#`/`$generichide`).
    pub generichide: bool,
}

/// Batch selectors into `display:none` rules (chunked so one bad selector
/// cannot invalidate the whole sheet).
fn selectors_to_css(selectors: &[String]) -> String {
    let mut css = String::new();
    for chunk in selectors.chunks(200) {
        if chunk.is_empty() {
            continue;
        }
        css.push_str(&chunk.join(",\n"));
        css.push_str("{display:none!important;}\n");
    }

    css
}

impl TrackerBlocker {
    /// Build a blocker preloaded with the bundled default tracker list.
    pub fn with_default_list() -> NetworkResult<Self> {
        Self::from_filter_texts(&[DEFAULT_TRACKER_LIST])
    }

    /// Build from one or more Adblock-Plus-syntax filter list texts.
    pub fn from_filter_texts(texts: &[&str]) -> NetworkResult<Self> {
        Self::from_lists(texts, &[])
    }

    /// The `trusted` lists may also run scriptlets that need trust.
    pub fn from_lists(texts: &[&str], trusted: &[&str]) -> NetworkResult<Self> {
        let mut set = FilterSet::new(false);
        for text in texts {
            set.add_filter_list(text, ParseOptions::default());
        }
        let trusted_options = ParseOptions {
            permissions: crate::scriptlet_library::TRUSTED,
            ..ParseOptions::default()
        };
        for text in trusted {
            set.add_filter_list(text, trusted_options);
        }

        let engine = Engine::from_filter_set(set, true);
        Ok(Self { engine })
    }

    /// Like [`Self::from_lists`], but memoizing the compiled engine in
    /// `cache_dir`: a hit deserializes instead of re-parsing the lists, and a
    /// miss parses then writes the cache (best-effort) for the next launch.
    /// Stale `engine-*.bin` files from older list versions are pruned.
    pub fn from_filter_texts_with_cache(
        texts: &[&str],
        trusted: &[&str],
        cache_dir: &Path,
    ) -> NetworkResult<Self> {
        let file_name = engine_cache_file(texts, trusted);
        let path = cache_dir.join(&file_name);

        if let Some(blocker) = Self::from_engine_cache(&path) {
            return Ok(blocker);
        }

        let blocker = Self::from_lists(texts, trusted)?;
        blocker.write_engine_cache(cache_dir, &file_name);
        Ok(blocker)
    }

    /// Deserialize a cached engine. Any failure (missing file, version
    /// mismatch after a crate upgrade, corruption) returns `None` so the
    /// caller falls back to a fresh parse.
    fn from_engine_cache(path: &Path) -> Option<Self> {
        let bytes = std::fs::read(path).ok()?;
        let mut engine = Engine::default();
        match engine.deserialize(&bytes) {
            Ok(()) => {
                tracing::info!(
                    "blocker: loaded compiled engine cache ({} bytes) from {}",
                    bytes.len(),
                    path.display()
                );
                Some(Self { engine })
            }
            Err(err) => {
                tracing::warn!(
                    "blocker: engine cache at {} unusable ({err:?}); reparsing filter lists",
                    path.display()
                );
                None
            }
        }
    }

    /// Write the compiled engine beside the filter-list cache and prune
    /// engine caches from other list versions. Best-effort: failures only
    /// log (the next launch just parses again).
    fn write_engine_cache(&self, cache_dir: &Path, file_name: &str) {
        if let Err(err) = std::fs::create_dir_all(cache_dir) {
            tracing::warn!("blocker: create cache dir failed: {err}");
            return;
        }

        let stale: Vec<PathBuf> = std::fs::read_dir(cache_dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("engine-") && n.ends_with(".bin"))
                    && p.file_name().and_then(|n| n.to_str()) != Some(file_name)
            })
            .collect();
        for path in stale {
            let _ = std::fs::remove_file(path);
        }

        let path = cache_dir.join(file_name);
        match std::fs::write(&path, self.engine.serialize()) {
            Ok(()) => tracing::info!("blocker: wrote engine cache to {}", path.display()),
            Err(err) => tracing::warn!("blocker: engine cache write failed: {err}"),
        }
    }

    /// An empty blocker. Used when `PrivacyMode::Permissive` disables
    /// tracker blocking entirely, but the policy code still wants something
    /// to call.
    pub fn empty() -> Self {
        let engine = Engine::from_filter_set(FilterSet::new(false), true);
        Self { engine }
    }

    /// Load uBO's scriptlet library (see [`crate::scriptlet_library`]) into
    /// the engine, enabling `+js(...)` scriptlet rules to expand via
    /// [`Self::scriptlets_for_page`]. Returns the number of resources.
    pub fn load_resources(&mut self, json: &str) -> NetworkResult<usize> {
        let resources = crate::scriptlet_library::resources(json)?;
        let count = resources.len();
        self.engine.use_resources(resources);

        Ok(count)
    }

    /// Assembled scriptlet JS for a page (empty when no rules match or no
    /// resource library is loaded). Must run at document start, before the
    /// page's own scripts.
    pub fn scriptlets_for_page(&self, url: &str) -> String {
        let scriptlets = self.engine.url_cosmetic_resources(url).injected_script;
        if scriptlets.is_empty() {
            return scriptlets;
        }
        format!("(function(){{\nconst scriptletGlobals = {{}};\n{scriptlets}\n}})();")
    }

    /// Host-specific cosmetic resources for a page: CSS hiding elements the
    /// filter lists declare against this exact host, plus the exception set
    /// and `generichide` flag consumed by [`Self::cosmetic_generic_css`].
    pub fn cosmetic_for_page(&self, url: &str) -> CosmeticPage {
        let res = self.engine.url_cosmetic_resources(url);
        let selectors: Vec<String> = res.hide_selectors.into_iter().collect();
        let mut hide_css = selectors_to_css(&selectors);
        // Procedural rules whose selector is expressible in plain CSS (most
        // importantly `:has(...)`) are appended as their own styled rules.
        for json in &res.procedural_actions {
            let Ok(filter) = serde_json::from_str::<
                adblock::cosmetic_filter_cache::ProceduralOrActionFilter,
            >(json) else {
                continue;
            };

            if let Some((selector, style)) = filter.as_css() {
                hide_css.push_str(&selector);
                hide_css.push('{');
                hide_css.push_str(&style);
                hide_css.push_str(";}\n");
            }
        }

        CosmeticPage {
            hide_css,
            exceptions: res.exceptions,
            generichide: res.generichide,
        }
    }

    /// Generic element-hiding CSS for the classes/ids actually present on a page.
    pub fn cosmetic_generic_css(
        &self,
        classes: &[String],
        ids: &[String],
        exceptions: &std::collections::HashSet<String>,
    ) -> String {
        let selectors =
            self.engine
                .hidden_class_id_selectors(classes.iter(), ids.iter(), exceptions);
        selectors_to_css(&selectors)
    }

    /// Check a request. `source_url` is the URL of the document making the
    /// request (used to determine first-party vs third-party).
    pub fn check_request(
        &self,
        url: &Url,
        source_url: &Url,
        request_type: RequestType,
    ) -> NetworkResult<BlockerVerdict> {
        let req = Request::new(url.as_str(), source_url.as_str(), request_type.as_str())
            .map_err(|e| NetworkError::FilterList(format!("invalid request: {e:?}")))?;
        let result = self.engine.check_network_request(&req);

        if result.matched {
            Ok(BlockerVerdict::Block {
                reason: result.filter.unwrap_or_else(|| "tracker".into()),
            })
        } else {
            Ok(BlockerVerdict::Allow)
        }
    }

    /// A list blocks the host and an `@@` rule then lets it through.
    pub fn excepts_tunnel(&self, host: &str) -> bool {
        tunnel_url(host)
            .and_then(|url| Request::new(url.as_str(), url.as_str(), "document").ok())
            .is_some_and(|request| {
                self.engine
                    .check_network_request(&request)
                    .exception
                    .is_some()
            })
    }

    /// Judged from the host it hides behind, so `$third-party` rules apply.
    pub fn blocks_alias(&self, alias: &str, host: &str) -> bool {
        let (Some(alias), Some(host)) = (tunnel_url(alias), tunnel_url(host)) else {
            return false;
        };
        matches!(
            self.check_request(&alias, &host, RequestType::Document),
            Ok(BlockerVerdict::Block { .. })
        )
    }

    pub fn blocks_tunnel(&self, host: &str) -> bool {
        tunnel_url(host).is_some_and(|url| {
            matches!(
                self.check_request(&url, &url, RequestType::Document),
                Ok(BlockerVerdict::Block { .. })
            )
        })
    }
}

/// A tunnel hides its path, so the proxy judges it as a document request for
/// the host's root.
pub fn tunnel_url(host: &str) -> Option<Url> {
    Url::parse(&format!("https://{host}/")).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_cache_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("wis-blocker-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp cache dir");
        dir
    }

    const SCRIPTLET_RULES: &str =
        "a.test##+js(set-constant, foo, 1)\na.test##+js(trusted-click-element, #reject)\n";

    fn with_library(mut blocker: TrackerBlocker) -> TrackerBlocker {
        blocker
            .load_resources(crate::scriptlet_library::tests::LIBRARY)
            .expect("library");
        blocker
    }

    #[test]
    fn only_trusted_lists_run_trusted_scriptlets() {
        let untrusted =
            with_library(TrackerBlocker::from_filter_texts(&[SCRIPTLET_RULES]).unwrap());
        let page = untrusted.scriptlets_for_page("https://a.test/");
        assert!(page.contains("setConstant("), "{page}");
        assert!(page.contains("safeSelf"), "{page}");
        assert!(!page.contains("trustedClickElement("), "{page}");

        let trusted = with_library(TrackerBlocker::from_lists(&[], &[SCRIPTLET_RULES]).unwrap());
        let page = trusted.scriptlets_for_page("https://a.test/");
        assert!(page.contains("trustedClickElement("), "{page}");
    }

    /// uBO's helpers read a `scriptletGlobals` binding, and the page should
    /// not see or replace the scriptlets' functions.
    #[test]
    fn a_page_s_scriptlets_share_one_private_scope() {
        let blocker = with_library(TrackerBlocker::from_filter_texts(&[SCRIPTLET_RULES]).unwrap());
        let page = blocker.scriptlets_for_page("https://a.test/");
        assert!(
            page.starts_with("(function(){\nconst scriptletGlobals = {};\n"),
            "{page}"
        );
        assert!(page.ends_with("\n})();"), "{page}");
        assert_eq!(blocker.scriptlets_for_page("https://elsewhere.test/"), "");
    }

    #[test]
    fn trusting_a_list_misses_the_engine_cache() {
        let dir = temp_cache_dir("trust");
        let _ = TrackerBlocker::from_filter_texts_with_cache(&[SCRIPTLET_RULES], &[], &dir)
            .expect("untrusted");
        let trusted = with_library(
            TrackerBlocker::from_filter_texts_with_cache(&[], &[SCRIPTLET_RULES], &dir)
                .expect("trusted"),
        );
        assert!(trusted
            .scriptlets_for_page("https://a.test/")
            .contains("trustedClickElement("));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_tunnel_is_judged_by_its_host_alone() {
        let blocker = TrackerBlocker::from_filter_texts(&["||tracker.test^\n/ads/banner.js\n"])
            .expect("list");
        assert!(blocker.blocks_tunnel("tracker.test"));
        assert!(blocker.blocks_tunnel("cdn.tracker.test"));
        assert!(!blocker.blocks_tunnel("site.test"));
        assert!(!blocker.blocks_tunnel(""));
    }

    fn assert_blocks_tracker(blocker: &TrackerBlocker) {
        let url = Url::parse("https://tracker.test/pixel.js").expect("url");
        let source = Url::parse("https://site.test/").expect("url");
        let verdict = blocker
            .check_request(&url, &source, RequestType::Script)
            .expect("check");
        assert!(matches!(verdict, BlockerVerdict::Block { .. }));
    }

    #[test]
    fn cosmetic_resources_surface_specific_and_generic_hides() {
        let list = "##.ad-banner\nexample.test###sponsored\n";
        let blocker = TrackerBlocker::from_filter_texts(&[list]).expect("build");

        let page = blocker.cosmetic_for_page("https://example.test/page");
        assert!(page.hide_css.contains("#sponsored"), "{}", page.hide_css);

        let proc_list = "example.test##div:has(> .ad-inner)\n";
        let proc_blocker = TrackerBlocker::from_filter_texts(&[proc_list]).expect("build");
        let proc_page = proc_blocker.cosmetic_for_page("https://example.test/x");
        assert!(
            proc_page.hide_css.contains(":has(> .ad-inner)"),
            "{}",
            proc_page.hide_css
        );

        let generic =
            blocker.cosmetic_generic_css(&["ad-banner".to_string()], &[], &page.exceptions);
        assert!(generic.contains(".ad-banner"), "{generic}");
    }

    #[test]
    fn cache_roundtrip_preserves_blocking_decisions() {
        let dir = temp_cache_dir("roundtrip");
        let list = "||tracker.test^\n";

        // Miss: parses and writes the cache.
        let first =
            TrackerBlocker::from_filter_texts_with_cache(&[list], &[], &dir).expect("build");
        assert_blocks_tracker(&first);
        let cached = dir.join(engine_cache_file(&[list], &[]));
        assert!(cached.exists(), "engine cache must be written on miss");

        // Hit: deserializes, with identical decisions.
        let second =
            TrackerBlocker::from_filter_texts_with_cache(&[list], &[], &dir).expect("reload");
        assert_blocks_tracker(&second);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn changed_list_replaces_stale_engine_cache() {
        let dir = temp_cache_dir("stale");
        let old_list = "||old.test^\n";
        let new_list = "||tracker.test^\n";

        let _ = TrackerBlocker::from_filter_texts_with_cache(&[old_list], &[], &dir).expect("v1");
        let blocker =
            TrackerBlocker::from_filter_texts_with_cache(&[new_list], &[], &dir).expect("v2");
        assert_blocks_tracker(&blocker);

        assert!(!dir.join(engine_cache_file(&[old_list], &[])).exists());
        assert!(dir.join(engine_cache_file(&[new_list], &[])).exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_cache_falls_back_to_parsing() {
        let dir = temp_cache_dir("corrupt");
        let list = "||tracker.test^\n";
        std::fs::write(dir.join(engine_cache_file(&[list], &[])), b"not a dat file")
            .expect("write garbage");

        let blocker =
            TrackerBlocker::from_filter_texts_with_cache(&[list], &[], &dir).expect("fallback");
        assert_blocks_tracker(&blocker);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
