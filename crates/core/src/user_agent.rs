//! User agent strings for remote (page) webviews.
//!
//! WKWebView's default user agent omits the `Version/<n>` and `Safari/<build>`
//! tokens, so many sites flag the embedded engine as unknown or out of date
//! ("unsupported browser"). Presenting a modern Safari-class string restores
//! compatibility, since the engine genuinely is WebKit. A small per-host
//! override map handles the rare site that special-cases Chrome.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Frozen WebKit build token, as real Safari reports it.
const WEBKIT_BUILD: &str = "605.1.15";
/// Safari marketing version token. Bump to track current Safari.
const SAFARI_VERSION: &str = "18.5";
/// Chrome version token for the Chrome profile.
const CHROME_VERSION: &str = "126.0.0.0";

/// macOS version token Safari reports. Apple froze this at `10_15_7`
/// regardless of the real OS or CPU, so matching it is the safe default.
pub const DEFAULT_MAC_OS_VERSION: &str = "10_15_7";

/// Which user agent a remote webview should present.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UaProfile {
    /// Leave the engine default untouched (kept as an explicit escape hatch).
    Default,
    /// A modern desktop Safari string. The recommended default.
    #[default]
    Safari,
    /// A desktop Chrome string for sites that special-case Chrome.
    Chrome,
    /// A caller-supplied verbatim string.
    Custom(String),
}

/// A modern desktop Safari user agent for the given macOS version token.
pub fn compatibility_user_agent(os_version: &str) -> String {
    format!(
        "Mozilla/5.0 (Macintosh; Intel Mac OS X {os}) \
         AppleWebKit/{wk} (KHTML, like Gecko) Version/{ver} Safari/{wk}",
        os = os_version,
        wk = WEBKIT_BUILD,
        ver = SAFARI_VERSION,
    )
}

/// A desktop Chrome user agent for the given macOS version token.
pub fn chrome_user_agent(os_version: &str) -> String {
    format!(
        "Mozilla/5.0 (Macintosh; Intel Mac OS X {os}) \
         AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{chrome} Safari/537.36",
        os = os_version,
        chrome = CHROME_VERSION,
    )
}

/// Resolve a profile to a concrete user agent string. `None` means "do not
/// override" (leave the engine default), which is the `Default` profile and an
/// empty `Custom` string.
pub fn resolve_profile(profile: &UaProfile, os_version: &str) -> Option<String> {
    match profile {
        UaProfile::Default => None,
        UaProfile::Safari => Some(compatibility_user_agent(os_version)),
        UaProfile::Chrome => Some(chrome_user_agent(os_version)),
        UaProfile::Custom(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
    }
}

/// `www.`-stripped, lowercased host, matching the favicon/vault host
/// normalisation convention so overrides apply across `www`/bare hosts.
pub fn normalize_host(host: &str) -> String {
    let lower = host.trim().to_ascii_lowercase();
    lower.strip_prefix("www.").unwrap_or(&lower).to_string()
}

/// User agent for a request to `host`: a per-host override if present,
/// otherwise the base profile. `None` leaves the engine default in place.
pub fn ua_for_host(
    host: &str,
    overrides: &HashMap<String, UaProfile>,
    base: &UaProfile,
    os_version: &str,
) -> Option<String> {
    let normalized = normalize_host(host);
    let profile = overrides.get(&normalized).unwrap_or(base);
    resolve_profile(profile, os_version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safari_ua_has_required_tokens_and_no_chrome() {
        let ua = compatibility_user_agent(DEFAULT_MAC_OS_VERSION);
        assert!(ua.contains("Macintosh"));
        assert!(ua.contains("AppleWebKit/605.1.15"));
        assert!(ua.contains("Version/"));
        assert!(ua.contains("Safari/605.1.15"));
        assert!(!ua.contains("Chrome"));
        assert!(!ua.contains("Chromium"));
    }

    #[test]
    fn chrome_ua_has_chrome_token() {
        let ua = chrome_user_agent(DEFAULT_MAC_OS_VERSION);
        assert!(ua.contains("Chrome/"));
        assert!(ua.contains("Safari/537.36"));
    }

    #[test]
    fn default_profile_does_not_override() {
        assert_eq!(resolve_profile(&UaProfile::Default, DEFAULT_MAC_OS_VERSION), None);
    }

    #[test]
    fn profile_defaults_to_safari() {
        assert_eq!(UaProfile::default(), UaProfile::Safari);
        assert!(resolve_profile(&UaProfile::default(), DEFAULT_MAC_OS_VERSION).is_some());
    }

    #[test]
    fn empty_custom_does_not_override() {
        assert_eq!(resolve_profile(&UaProfile::Custom("   ".into()), DEFAULT_MAC_OS_VERSION), None);
        assert_eq!(
            resolve_profile(&UaProfile::Custom(" my-agent ".into()), DEFAULT_MAC_OS_VERSION),
            Some("my-agent".to_string())
        );
    }

    #[test]
    fn host_override_beats_base_and_normalizes_www() {
        let mut overrides = HashMap::new();
        overrides.insert("example.com".to_string(), UaProfile::Chrome);

        let chosen = ua_for_host(
            "www.example.com",
            &overrides,
            &UaProfile::Safari,
            DEFAULT_MAC_OS_VERSION,
        )
        .unwrap();
        assert!(chosen.contains("Chrome/"));

        let base = ua_for_host(
            "other.com",
            &overrides,
            &UaProfile::Safari,
            DEFAULT_MAC_OS_VERSION,
        )
        .unwrap();
        assert!(!base.contains("Chrome"));
    }
}
