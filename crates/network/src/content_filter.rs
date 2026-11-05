//! Adblock filter lists compiled to WebKit content-blocking JSON.
//!
//! The same filter lists that drive the proxy's host-level [`TrackerBlocker`]
//! are converted here into the rule format `WKContentRuleList` consumes, so
//! the engine can filter at the path level inside HTTPS with TLS intact.

use adblock::lists::{parse_filter, FilterSet, ParseOptions, ParsedFilter};
use serde_json::Value;
use wis_core::PrivacyMode;

use crate::error::{NetworkError, NetworkResult};
use crate::site_entities::{entities, own_script_exceptions};

/// Build the content-blocker rule JSON for a privacy mode, or `None` when the
/// mode applies no engine-level filtering.
pub fn content_blocking_json(
    filter_texts: &[&str],
    mode: PrivacyMode,
) -> NetworkResult<Option<String>> {
    if mode == PrivacyMode::Permissive {
        return Ok(None);
    }

    let mut rules = Vec::new();
    if mode == PrivacyMode::Strict {
        rules.push(third_party_script_block());
        rules.extend(own_script_exceptions(&entities()));
    }
    rules.extend(tracker_rules(filter_texts)?);

    serde_json::to_string(&rules)
        .map(Some)
        .map_err(|err| NetworkError::FilterList(format!("serialize content rules: {err}")))
}

/// Content-blocker JSON for lists compiled on their own, without a privacy
/// mode's extra rules.
pub fn list_blocking_json(filter_texts: &[&str]) -> NetworkResult<String> {
    serde_json::to_string(&tracker_rules(filter_texts)?)
        .map_err(|err| NetworkError::FilterList(format!("serialize content rules: {err}")))
}

/// The list without its element hiding and scriptlet rules.
pub fn network_rules(list: &str) -> String {
    let mut kept = String::with_capacity(list.len());
    for line in list.lines().filter(|line| {
        !matches!(
            parse_filter(line, false, ParseOptions::default()),
            Ok(ParsedFilter::Cosmetic(_))
        )
    }) {
        kept.push_str(line);
        kept.push('\n');
    }
    kept
}

fn tracker_rules(filter_texts: &[&str]) -> NetworkResult<Vec<Value>> {
    // into_content_blocking requires the source rules be retained, which only
    // happens when the FilterSet is built in debug mode.
    let mut set = FilterSet::new(true);
    for text in filter_texts {
        set.add_filter_list(text, ParseOptions::default());
    }

    let (cb_rules, _) = set
        .into_content_blocking()
        .map_err(|_| NetworkError::FilterList("content-blocking conversion failed".into()))?;

    cb_rules
        .into_iter()
        .map(|rule| {
            serde_json::to_value(rule)
                .map_err(|err| NetworkError::FilterList(format!("content rule: {err}")))
        })
        .collect()
}

/// Strict-only rule blocking third-party scripts (the tracking vector) while
/// leaving fonts and media loading. A site's owner's other domains are let
/// back in by [`own_script_exceptions`], which follow it.
fn third_party_script_block() -> Value {
    serde_json::json!({
        "trigger": {
            "url-filter": ".*",
            "load-type": ["third-party"],
            "resource-type": ["script"]
        },
        "action": { "type": "block" }
    })
}

/// This machine's loopback as WebKit `url-filter` patterns.
const LOCAL_URL_FILTERS: [&str; 8] = [
    r"^[a-z]+://([^/@]*@)?localhost\.?[:/]",
    r"^[a-z]+://([^/@]*@)?[a-z0-9.-]+\.localhost\.?[:/]",
    r"^[a-z]+://([^/@]*@)?127\.[0-9]+\.[0-9]+\.[0-9]+[:/]",
    r"^[a-z]+://([^/@]*@)?0\.0\.0\.0[:/]",
    r"^[a-z]+://([^/@]*@)?\[::1\][:/]",
    r"^[a-z]+://([^/@]*@)?\[::\][:/]",
    r"^[a-z]+://([^/@]*@)?\[::ffff:7f[0-9a-f][0-9a-f]:[0-9a-f]+\][:/]",
    r"^[a-z]+://([^/@]*@)?\[::ffff:0:0\][:/]",
];

/// WebKit rules that block every load of this machine's loopback, documents
/// included, for workspaces behind Tor or SOCKS.
pub fn local_blocking_json() -> String {
    let rules = LOCAL_URL_FILTERS
        .iter()
        .map(|filter| {
            serde_json::json!({
                "trigger": { "url-filter": filter },
                "action": { "type": "block" },
            })
        })
        .collect();
    serde_json::Value::Array(rules).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter_lists::DEFAULT_TRACKER_LIST;

    #[test]
    fn a_list_on_its_own_gets_no_privacy_mode_extras() {
        let json = list_blocking_json(&["||consent.test^\n@@||consent.test/ok.js\n"]).unwrap();
        assert!(json.contains("consent"));
        assert!(json.contains("ignore-previous-rules"));
        let rules: Vec<Value> = serde_json::from_str(&json).unwrap();
        assert!(!rules.contains(&third_party_script_block()));
    }

    #[test]
    fn a_list_s_hiding_and_scriptlet_rules_stay_out_of_webkit() {
        let list = "[Adblock Plus 2.0]\n! Title: Notices\n||consent.example^$third-party\n\
            /cookie-banner.js$script\n@@||consent.example/allowed.js\n##.cookie-banner\n\
            example.com##.consent\nexample.com##+js(set-cookie, consent, no)\n\
            example.com#@#.consent\n";
        let kept = network_rules(list);
        for rule in [
            "||consent.example^$third-party",
            "/cookie-banner.js$script",
            "@@||consent.example/allowed.js",
        ] {
            assert!(kept.contains(rule), "{rule}");
        }
        assert!(!kept.contains('#'), "{kept}");

        let json = content_blocking_json(&[&kept], PrivacyMode::Balanced)
            .unwrap()
            .unwrap();
        assert!(json.contains("cookie-banner"));
        assert!(!json.contains("css-display-none"));
    }

    #[test]
    fn balanced_emits_blocking_rules_for_known_tracker() {
        let json = content_blocking_json(&[DEFAULT_TRACKER_LIST], PrivacyMode::Balanced)
            .expect("conversion succeeds")
            .expect("balanced produces rules");

        let rules: Value = serde_json::from_str(&json).expect("valid JSON");
        let array = rules.as_array().expect("rule list is an array");
        assert!(!array.is_empty(), "tracker list must yield rules");

        for rule in array {
            assert!(rule
                .get("trigger")
                .and_then(|t| t.get("url-filter"))
                .is_some());
            assert!(rule.get("action").and_then(|a| a.get("type")).is_some());
        }
        assert!(json.contains("doubleclick"), "bundled tracker must convert");
    }

    #[test]
    fn strict_adds_third_party_script_block() {
        let balanced = content_blocking_json(&[DEFAULT_TRACKER_LIST], PrivacyMode::Balanced)
            .unwrap()
            .unwrap();
        let strict = content_blocking_json(&[DEFAULT_TRACKER_LIST], PrivacyMode::Strict)
            .unwrap()
            .unwrap();

        assert!(strict.contains("third-party"));
        assert!(
            strict.len() > balanced.len(),
            "strict layers an extra rule on top of the tracker list"
        );
    }

    #[test]
    fn strict_lets_a_site_run_its_owner_s_scripts_before_the_lists_apply() {
        let rules = |mode| -> Vec<Value> {
            serde_json::from_str(
                &content_blocking_json(&[DEFAULT_TRACKER_LIST], mode)
                    .unwrap()
                    .unwrap(),
            )
            .unwrap()
        };
        let strict = rules(PrivacyMode::Strict);
        assert_eq!(strict[0], third_party_script_block());
        let own_quora_cdn = |rule: &Value| {
            rule["action"]["type"] == "ignore-previous-rules"
                && rule["trigger"]["url-filter"]
                    .as_str()
                    .is_some_and(|f| f.contains("quoracdn"))
        };
        let exception = strict
            .iter()
            .position(own_quora_cdn)
            .expect("quora's cdn is its own");
        let first_list_rule = strict
            .iter()
            .position(|rule| rule.to_string().contains("doubleclick"))
            .unwrap();
        assert!(exception < first_list_rule);
        assert!(!rules(PrivacyMode::Balanced).iter().any(own_quora_cdn));
    }

    #[test]
    fn permissive_emits_no_rules() {
        let out = content_blocking_json(&[DEFAULT_TRACKER_LIST], PrivacyMode::Permissive)
            .expect("conversion succeeds");
        assert!(out.is_none(), "permissive applies no engine filter");
    }

    fn local_filters() -> Vec<regex::Regex> {
        let rules: serde_json::Value = serde_json::from_str(&local_blocking_json()).unwrap();
        rules
            .as_array()
            .unwrap()
            .iter()
            .map(|rule| {
                assert_eq!(rule["action"]["type"], "block");
                assert!(rule["trigger"].get("resource-type").is_none(), "every load");
                let filter = rule["trigger"]["url-filter"].as_str().unwrap();
                regex::RegexBuilder::new(filter)
                    .case_insensitive(true)
                    .build()
                    .unwrap()
            })
            .collect()
    }

    #[test]
    fn the_local_list_blocks_every_load_of_this_machine() {
        let filters = local_filters();
        for url in [
            "http://localhost/",
            "https://localhost:3000/a",
            "http://localhost.:8080/",
            "ws://localhost/socket",
            "HTTP://LOCALHOST/",
            "http://app.localhost/",
            "http://user:pass@127.0.0.1/",
            "http://127.0.0.1/",
            "http://127.8.9.10:631/",
            "http://0.0.0.0:11434/v1",
            "http://[::1]:8080/",
            "http://[::]/",
            "http://[::ffff:7f00:1]/",
            "http://[::ffff:0:0]/",
        ] {
            assert!(filters.iter().any(|f| f.is_match(url)), "{url}");
        }
    }

    #[test]
    fn the_local_list_leaves_other_hosts_alone() {
        let filters = local_filters();
        for url in [
            "https://localhost.example.com/",
            "https://notlocalhost/",
            "https://example.com/localhost/",
            "https://127.example.com/",
            "https://example.com/?u=http://127.0.0.1/",
            "http://192.168.1.20/",
            "http://[fd00::1]/",
            "http://[::ffff:7f:1]/",
            "http://[::ffff:c000:207]/",
        ] {
            assert!(!filters.iter().any(|f| f.is_match(url)), "{url}");
        }
    }
}
