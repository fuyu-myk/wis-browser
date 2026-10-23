//! Autoplay policy: what media may start without a user gesture, globally
//! and per host.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::sites::user_agent::normalize_host;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoplayPolicy {
    /// Media starts on its own, sound included.
    Allow,
    /// Muted media may start on its own; sound waits for a gesture.
    #[default]
    BlockAudible,
    /// Nothing starts without a gesture.
    Block,
}

/// The policy for `host`: its override when one is set, else `base`.
pub fn autoplay_policy_for_host(
    host: &str,
    overrides: &HashMap<String, AutoplayPolicy>,
    base: AutoplayPolicy,
) -> AutoplayPolicy {
    overrides
        .get(&normalize_host(host))
        .copied()
        .unwrap_or(base)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocking_sound_is_the_default() {
        assert_eq!(AutoplayPolicy::default(), AutoplayPolicy::BlockAudible);
    }

    #[test]
    fn a_host_without_an_override_gets_the_base_policy() {
        let overrides = HashMap::from([("video.example".to_string(), AutoplayPolicy::Allow)]);
        assert_eq!(
            autoplay_policy_for_host("news.example", &overrides, AutoplayPolicy::Block),
            AutoplayPolicy::Block
        );
    }

    #[test]
    fn an_override_matches_through_host_normalisation() {
        let overrides = HashMap::from([("video.example".to_string(), AutoplayPolicy::Allow)]);
        assert_eq!(
            autoplay_policy_for_host("WWW.Video.Example", &overrides, AutoplayPolicy::Block),
            AutoplayPolicy::Allow
        );
    }

    #[test]
    fn policies_serialise_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&AutoplayPolicy::BlockAudible).unwrap(),
            "\"block_audible\""
        );
        assert_eq!(
            serde_json::from_str::<AutoplayPolicy>("\"allow\"").unwrap(),
            AutoplayPolicy::Allow
        );
    }
}
