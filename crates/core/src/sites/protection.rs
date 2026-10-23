//! Per-site protection: whether in-page filtering, the page's JavaScript and
//! WebKit's fingerprinting protection apply on a host.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::sites::user_agent::normalize_host;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SiteProtection {
    pub blocking: bool,
    pub javascript: bool,
    pub fingerprinting: bool,
}

impl Default for SiteProtection {
    fn default() -> Self {
        Self {
            blocking: true,
            javascript: true,
            fingerprinting: true,
        }
    }
}

pub fn site_protection_for_host(
    host: &str,
    overrides: &HashMap<String, SiteProtection>,
) -> SiteProtection {
    overrides
        .get(&normalize_host(host))
        .copied()
        .unwrap_or_default()
}

pub fn with_site_protection(
    overrides: &HashMap<String, SiteProtection>,
    host: &str,
    protection: SiteProtection,
) -> HashMap<String, SiteProtection> {
    let host = normalize_host(host);
    let mut next = overrides.clone();
    if protection == SiteProtection::default() || host.is_empty() {
        next.remove(&host);
    } else {
        next.insert(host, protection);
    }
    next
}

/// Hosts that normalise alike keep the entry whose original key sorts last.
pub fn sanitized_site_protection(
    overrides: HashMap<String, SiteProtection>,
) -> HashMap<String, SiteProtection> {
    let mut entries: Vec<(String, SiteProtection)> = overrides.into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
        .into_iter()
        .map(|(host, protection)| (normalize_host(&host), protection))
        .filter(|(host, protection)| !host.is_empty() && *protection != SiteProtection::default())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::user_settings::{UserSettings, UserSettingsPatch};

    const OFF: SiteProtection = SiteProtection {
        blocking: false,
        javascript: true,
        fingerprinting: true,
    };
    const NO_SCRIPT: SiteProtection = SiteProtection {
        blocking: true,
        javascript: false,
        fingerprinting: true,
    };

    #[test]
    fn sites_are_protected_until_switched_off() {
        assert_eq!(
            site_protection_for_host("news.example", &HashMap::new()),
            SiteProtection {
                blocking: true,
                javascript: true,
                fingerprinting: true,
            }
        );
    }

    #[test]
    fn an_override_matches_through_host_normalisation() {
        let overrides = HashMap::from([("news.example".to_string(), OFF)]);
        assert_eq!(
            site_protection_for_host("WWW.News.Example", &overrides),
            OFF
        );
        assert_eq!(
            site_protection_for_host("other.example", &overrides),
            SiteProtection::default()
        );
    }

    #[test]
    fn switching_a_site_back_on_forgets_it() {
        let off = with_site_protection(&HashMap::new(), "www.News.example", OFF);
        assert_eq!(off, HashMap::from([("news.example".to_string(), OFF)]));

        let on = with_site_protection(&off, "news.example", SiteProtection::default());
        assert!(on.is_empty());
        assert_eq!(off.len(), 1);
    }

    #[test]
    fn a_switched_host_is_found_under_the_key_it_was_saved_with() {
        let saved = with_site_protection(&HashMap::new(), "www.www.example", OFF);
        assert_eq!(site_protection_for_host("www.www.example", &saved), OFF);
    }

    #[test]
    fn saved_overrides_are_normalised_and_defaults_dropped() {
        let saved = HashMap::from([
            ("WWW.A.example".to_string(), OFF),
            ("b.example".to_string(), SiteProtection::default()),
            ("".to_string(), OFF),
        ]);
        assert_eq!(
            sanitized_site_protection(saved),
            HashMap::from([("a.example".to_string(), OFF)])
        );
    }

    #[test]
    fn hosts_that_normalise_alike_resolve_the_same_way_every_time() {
        for _ in 0..16 {
            let saved = HashMap::from([
                ("a.example".to_string(), OFF),
                ("www.a.example".to_string(), NO_SCRIPT),
            ]);
            assert_eq!(
                sanitized_site_protection(saved),
                HashMap::from([("a.example".to_string(), NO_SCRIPT)])
            );
        }
    }

    #[test]
    fn a_saved_entry_missing_a_switch_reads_it_as_on() {
        let read: SiteProtection = serde_json::from_str(r#"{"blocking":false}"#).unwrap();
        assert_eq!(read, OFF);
        let saved_before_fingerprinting: SiteProtection =
            serde_json::from_str(r#"{"blocking":true,"javascript":false}"#).unwrap();
        assert_eq!(saved_before_fingerprinting, NO_SCRIPT);
    }

    #[test]
    fn fingerprinting_protection_off_alone_is_remembered() {
        let off = SiteProtection {
            fingerprinting: false,
            ..SiteProtection::default()
        };
        let saved = with_site_protection(&HashMap::new(), "draw.example", off);
        assert_eq!(site_protection_for_host("draw.example", &saved), off);
    }

    #[test]
    fn settings_start_with_no_overrides_and_patch_them_whole() {
        assert!(UserSettings::default().site_protection_overrides.is_empty());
        let patched = UserSettings::default().with_patch(UserSettingsPatch {
            site_protection_overrides: Some(HashMap::from([("a.example".to_string(), OFF)])),
            ..Default::default()
        });
        assert_eq!(
            patched.site_protection_overrides.get("a.example"),
            Some(&OFF)
        );
    }
}
