//! Adblock filter lists compiled to WebKit content-blocking JSON.
//!
//! The same filter lists that drive the proxy's host-level [`TrackerBlocker`]
//! are converted here into the rule format `WKContentRuleList` consumes, so
//! the engine can filter at the path level inside HTTPS with TLS intact.

use adblock::lists::{FilterSet, ParseOptions};
use serde_json::Value;
use wis_core::PrivacyMode;

use crate::error::{NetworkError, NetworkResult};

/// Build the content-blocker rule JSON for a privacy mode, or `None` when the
/// mode applies no engine-level filtering.
pub fn content_blocking_json(
    filter_texts: &[&str],
    mode: PrivacyMode,
) -> NetworkResult<Option<String>> {
    if mode == PrivacyMode::Permissive {
        return Ok(None);
    }

    let mut rules = tracker_rules(filter_texts)?;
    if mode == PrivacyMode::Strict {
        rules.push(third_party_script_block());
    }

    serde_json::to_string(&rules)
        .map(Some)
        .map_err(|err| NetworkError::FilterList(format!("serialize content rules: {err}")))
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
/// leaving first-party CDNs, fonts, and media loading.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter_lists::DEFAULT_TRACKER_LIST;

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
    fn permissive_emits_no_rules() {
        let out = content_blocking_json(&[DEFAULT_TRACKER_LIST], PrivacyMode::Permissive)
            .expect("conversion succeeds");
        assert!(out.is_none(), "permissive applies no engine filter");
    }
}
