//! Round-trip tests for the engine-level content-blocker JSON. Guards against
//! silent regressions where rule generation produces zero rules or drops the
//! Strict-only third-party-script layer.

use serde_json::Value;
use wis_core::PrivacyMode;
use wis_network::content_blocking_json;
use wis_network::filter_lists::DEFAULT_TRACKER_LIST;

fn rules_for(mode: PrivacyMode) -> Vec<Value> {
    let json = content_blocking_json(&[DEFAULT_TRACKER_LIST], mode)
        .expect("content-blocker JSON generation must succeed")
        .expect("non-Permissive modes must emit rules");
    serde_json::from_str(&json).expect("emitted JSON must round-trip")
}

fn url_filter(rule: &Value) -> Option<&str> {
    rule.get("trigger")?.get("url-filter")?.as_str()
}

fn is_third_party_script(rule: &Value) -> bool {
    let trigger = match rule.get("trigger") {
        Some(t) => t,
        None => return false,
    };
    let load_types = trigger
        .get("load-type")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    let resource_types = trigger
        .get("resource-type")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    load_types.contains(&"third-party") && resource_types.contains(&"script")
}

#[test]
fn balanced_emits_host_rule_for_bundled_tracker() {
    let rules = rules_for(PrivacyMode::Balanced);
    assert!(
        rules.iter().any(|r| url_filter(r)
            .map(|f| f.contains("doubleclick"))
            .unwrap_or(false)),
        "Balanced output must contain a rule targeting doubleclick"
    );
}

#[test]
fn strict_adds_third_party_script_block() {
    let rules = rules_for(PrivacyMode::Strict);
    assert!(
        rules.iter().any(is_third_party_script),
        "Strict output must contain a third-party-script blocking rule"
    );
}

#[test]
fn balanced_omits_third_party_script_block() {
    let rules = rules_for(PrivacyMode::Balanced);
    assert!(
        !rules.iter().any(is_third_party_script),
        "Balanced output must NOT contain the Strict-only third-party-script rule"
    );
}

#[test]
fn permissive_emits_no_rules() {
    let output = content_blocking_json(&[DEFAULT_TRACKER_LIST], PrivacyMode::Permissive)
        .expect("Permissive call must succeed");
    assert!(output.is_none(), "Permissive applies no engine filter");
}
