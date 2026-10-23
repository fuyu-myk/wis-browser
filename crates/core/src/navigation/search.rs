//! Search engines for non-URL address-bar input.
//!
//! The built-in [`SearchEngine`] enum is the persisted default choice.
//! [`CustomSearchEngine`] adds user-defined engines with an address-bar keyword
//! (`kw query`) and a `{searchTerms}` template; at most one carries `is_default`,
//! which then wins over the built-in choice, and deleting it falls back.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use url::Url;

/// Default search engine for address-bar queries.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchEngine {
    #[default]
    DuckDuckGo,
    Google,
    Bing,
}

impl SearchEngine {
    /// Parse the serde wire string (`"duckduckgo"` etc.), for lenient map keys.
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "duckduckgo" => Some(Self::DuckDuckGo),
            "google" => Some(Self::Google),
            "bing" => Some(Self::Bing),
            _ => None,
        }
    }
}

/// Cap on user-defined engines.
pub const MAX_CUSTOM_ENGINES: usize = 16;
/// The query placeholder a template must contain. A single `%s` (the Chrome
/// convention) is accepted on input and normalised to this.
pub const SEARCH_TERMS_TOKEN: &str = "{searchTerms}";

const MAX_NAME_LEN: usize = 40;
const MAX_KEYWORD_LEN: usize = 32;
const MAX_TEMPLATE_LEN: usize = 2048;

/// A user-defined search engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomSearchEngine {
    pub name: String,
    /// Address-bar trigger (`kw query`): lowercase, no whitespace, unique.
    pub keyword: String,
    /// Query-URL template containing [`SEARCH_TERMS_TOKEN`].
    pub template: String,
    /// At most one engine per list; wins over the built-in default.
    #[serde(default)]
    pub is_default: bool,
}

impl CustomSearchEngine {
    /// Boundary sanitizer: a normalised copy, or `None` when unusable.
    pub fn sanitized(&self) -> Option<Self> {
        let keyword = sanitize_search_keyword(&self.keyword)?;
        let template = sanitize_search_template(&self.template)?;
        let trimmed = self.name.trim();
        let name = if trimmed.is_empty() {
            template_host(&template)?
        } else {
            trimmed.chars().take(MAX_NAME_LEN).collect()
        };

        Some(Self {
            name,
            keyword,
            template,
            is_default: self.is_default,
        })
    }

    /// The template with `query` substituted, percent-encoded.
    pub fn query_url(&self, query: &str) -> String {
        self.template
            .replace(SEARCH_TERMS_TOKEN, &encode_query(query))
    }
}

/// Normalise an address-bar keyword: trimmed, lowercased, capped. `None` when
/// empty, whitespace-bearing, or too long.
pub fn sanitize_search_keyword(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_whitespace) {
        return None;
    }

    let lowered = trimmed.to_lowercase();
    (lowered.chars().count() <= MAX_KEYWORD_LEN).then_some(lowered)
}

/// Sanitize a boundary-supplied engine list: drop invalid engines, dedupe by
/// keyword (first wins), keep at most one default, cap the list.
pub fn sanitize_search_engines(engines: Vec<CustomSearchEngine>) -> Vec<CustomSearchEngine> {
    let mut out: Vec<CustomSearchEngine> = Vec::new();
    let mut has_default = false;
    for engine in engines {
        let Some(mut engine) = engine.sanitized() else {
            continue;
        };
        if out.iter().any(|e| e.keyword == engine.keyword) {
            continue;
        }

        if engine.is_default && has_default {
            engine.is_default = false;
        }

        has_default = has_default || engine.is_default;
        out.push(engine);
        if out.len() >= MAX_CUSTOM_ENGINES {
            break;
        }
    }

    out
}

/// Sanitize boundary-supplied built-in keyword assignments: invalid keywords
/// are dropped, and a duplicate keyword keeps the first engine in enum order.
/// Collisions with custom-engine keywords are resolved at lookup time (custom
/// engines win), so the map stays self-contained.
pub fn sanitize_builtin_search_keywords(
    map: HashMap<SearchEngine, String>,
) -> HashMap<SearchEngine, String> {
    let mut out = HashMap::new();
    let mut used: Vec<String> = Vec::new();
    for engine in [
        SearchEngine::DuckDuckGo,
        SearchEngine::Google,
        SearchEngine::Bing,
    ] {
        let Some(keyword) = map
            .get(&engine)
            .and_then(|raw| sanitize_search_keyword(raw))
        else {
            continue;
        };
        if used.iter().any(|k| k == &keyword) {
            continue;
        }

        used.push(keyword.clone());
        out.insert(engine, keyword);
    }

    out
}

/// Normalise a template: `%s` becomes the canonical token, the substituted
/// URL must be http(s) with a query-independent authority (never re-schemed),
/// capped length.
fn sanitize_search_template(raw: &str) -> Option<String> {
    let mut template = raw.trim().to_string();
    if !template.contains(SEARCH_TERMS_TOKEN) {
        template = template.replacen("%s", SEARCH_TERMS_TOKEN, 1);
    }
    if !template.contains(SEARCH_TERMS_TOKEN) || template.len() > MAX_TEMPLATE_LEN {
        return None;
    }

    let a = Url::parse(&template.replace(SEARCH_TERMS_TOKEN, "wisprobea")).ok()?;
    let b = Url::parse(&template.replace(SEARCH_TERMS_TOKEN, "wisprobeb")).ok()?;
    let authority_stable = a.scheme() == b.scheme()
        && a.host_str() == b.host_str()
        && a.port() == b.port()
        && a.username().is_empty()
        && b.username().is_empty()
        && a.password().is_none()
        && b.password().is_none();

    (authority_stable && matches!(a.scheme(), "http" | "https") && a.host_str().is_some())
        .then_some(template)
}

fn template_host(template: &str) -> Option<String> {
    let probe = template.replace(SEARCH_TERMS_TOKEN, "probe");
    Url::parse(&probe).ok()?.host_str().map(str::to_string)
}

/// Percent-encode a query for URL embedding (spaces as `%20`, so templates
/// with the token in the path work too).
fn encode_query(query: &str) -> String {
    url::form_urlencoded::byte_serialize(query.as_bytes())
        .collect::<String>()
        .replace('+', "%20")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine(name: &str, keyword: &str, template: &str, is_default: bool) -> CustomSearchEngine {
        CustomSearchEngine {
            name: name.to_string(),
            keyword: keyword.to_string(),
            template: template.to_string(),
            is_default,
        }
    }

    #[test]
    fn sanitize_drops_invalid_dedupes_and_caps() {
        let mut input = vec![
            engine("Kagi", "K", "https://kagi.com/search?q=%s", true),
            engine(
                "",
                "wiki",
                "https://en.wikipedia.org/wiki/{searchTerms}",
                true,
            ),
            engine("Dup", "kagi", "https://dup.example/?q={searchTerms}", false),
            engine("NoToken", "nt", "https://no-token.example/", false),
            engine(
                "BadScheme",
                "js",
                "javascript:alert('{searchTerms}')",
                false,
            ),
            engine(
                "Spaced",
                "two words",
                "https://ok.example/?q={searchTerms}",
                false,
            ),
        ];
        input[2].keyword = "k".to_string();
        for i in 0..MAX_CUSTOM_ENGINES + 4 {
            input.push(engine(
                "Filler",
                &format!("f{i}"),
                "https://filler.example/?q={searchTerms}",
                false,
            ));
        }

        let out = sanitize_search_engines(input);
        assert_eq!(out.len(), MAX_CUSTOM_ENGINES);
        assert_eq!(out[0].keyword, "k");
        assert_eq!(out[0].template, "https://kagi.com/search?q={searchTerms}");
        assert!(out[0].is_default);
        // The empty name derives from the template host; its default flag is
        // dropped because an earlier engine already claimed it.
        assert_eq!(out[1].name, "en.wikipedia.org");
        assert!(!out[1].is_default);
        assert_eq!(out.iter().filter(|e| e.is_default).count(), 1);
        assert!(out.iter().all(|e| e.keyword != "nt" && e.keyword != "js"));
    }

    #[test]
    fn rejects_templates_whose_authority_depends_on_the_query() {
        for template in [
            "https://{searchTerms}/x",
            "https://{searchTerms}.evil.example/",
            "https://google.com@evil.example/?q={searchTerms}",
            "https://{searchTerms}@site.example/",
            "https://x.example:{searchTerms}/",
        ] {
            let out = engine("E", "e", template, false).sanitized();
            assert!(out.is_none(), "accepted {template}");
        }
    }

    #[test]
    fn substitutes_every_token_occurrence() {
        let e = engine(
            "D",
            "d",
            "https://x.example/{searchTerms}?q={searchTerms}",
            false,
        );
        assert_eq!(e.query_url("a b"), "https://x.example/a%20b?q=a%20b");
    }

    #[test]
    fn from_id_matches_the_wire_format() {
        for engine in [
            SearchEngine::DuckDuckGo,
            SearchEngine::Google,
            SearchEngine::Bing,
        ] {
            let wire = serde_json::to_string(&engine).unwrap();
            assert_eq!(SearchEngine::from_id(wire.trim_matches('"')), Some(engine));
        }
        assert_eq!(SearchEngine::from_id("altavista"), None);
    }

    #[test]
    fn builtin_keywords_sanitize_drops_invalid_and_duplicates() {
        let map = HashMap::from([
            (SearchEngine::DuckDuckGo, " DDG ".to_string()),
            (SearchEngine::Google, "two words".to_string()),
            (SearchEngine::Bing, "ddg".to_string()),
        ]);
        let out = sanitize_builtin_search_keywords(map);
        assert_eq!(out.len(), 1);
        assert_eq!(out[&SearchEngine::DuckDuckGo], "ddg");
    }

    #[test]
    fn keyword_normalises_and_rejects_bad_input() {
        assert_eq!(sanitize_search_keyword("  GH "), Some("gh".to_string()));
        assert_eq!(sanitize_search_keyword("!b"), Some("!b".to_string()));
        for bad in ["", "   ", "two words", &"x".repeat(40)] {
            assert_eq!(sanitize_search_keyword(bad), None, "accepted {bad:?}");
        }
    }

    #[test]
    fn query_url_percent_encodes() {
        let e = engine("W", "w", "https://x.example/find/{searchTerms}", false);
        assert_eq!(
            e.query_url("a b&c+d"),
            "https://x.example/find/a%20b%26c%2Bd"
        );
    }
}
