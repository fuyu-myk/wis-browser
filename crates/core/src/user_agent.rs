//! User agent strings for remote (page) webviews.
//!
//! WKWebView's default user agent omits the `Version/<n>` and `Safari/<build>`
//! tokens, so many sites flag the embedded engine as unknown or out of date
//! ("unsupported browser"). Presenting a modern Safari-class string restores
//! compatibility, since the engine genuinely is WebKit. A small per-host
//! override map handles the rare site that special-cases Chrome.
//!
//! The parenthesised platform token is chosen from the running OS so a page
//! sees the real platform: sites key OS-specific downloads, styles, and feature
//! gates off it. The engine tokens stay as the chosen profile dictates.

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

/// Host platform a user agent presents itself as. Selected at the boundary
/// from [`std::env::consts::OS`] (a compile-time target constant. Kept out
/// of `serde` on purpose: it is derived from the running build, never persisted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum UaPlatform {
    /// Apple desktop: the frozen `Macintosh; Intel Mac OS X 10_15_7` token.
    #[default]
    Mac,
    /// Windows desktop, as modern browsers report it.
    Windows,
    /// Linux desktop (X11), as modern browsers report it.
    Linux,
}

impl UaPlatform {
    /// The platform of the running build, read from the compile-time
    /// [`std::env::consts::OS`] constant. Imperative-shell entry point.
    pub fn host() -> Self {
        Self::from_os(std::env::consts::OS)
    }

    /// Map a [`std::env::consts::OS`]-style token to a family. Pure, so every
    /// branch is testable from any host; unknown targets fall back to `Mac`
    /// so the produced string is always well-formed.
    pub fn from_os(os: &str) -> Self {
        match os {
            "windows" => Self::Windows,
            "linux" => Self::Linux,
            _ => Self::Mac,
        }
    }

    /// The parenthesised platform token, e.g. `Macintosh; Intel Mac OS X 10_15_7`.
    fn token(self) -> String {
        match self {
            Self::Mac => format!("Macintosh; Intel Mac OS X {DEFAULT_MAC_OS_VERSION}"),
            Self::Windows => "Windows NT 10.0; Win64; x64".to_string(),
            Self::Linux => "X11; Linux x86_64".to_string(),
        }
    }
}

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

/// A modern desktop Safari user agent for the given platform.
pub fn compatibility_user_agent(platform: UaPlatform) -> String {
    format!(
        "Mozilla/5.0 ({plat}) \
         AppleWebKit/{wk} (KHTML, like Gecko) Version/{ver} Safari/{wk}",
        plat = platform.token(),
        wk = WEBKIT_BUILD,
        ver = SAFARI_VERSION,
    )
}

/// A desktop Chrome user agent for the given platform.
pub fn chrome_user_agent(platform: UaPlatform) -> String {
    format!(
        "Mozilla/5.0 ({plat}) \
         AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{chrome} Safari/537.36",
        plat = platform.token(),
        chrome = CHROME_VERSION,
    )
}

/// Resolve a profile to a concrete user agent string. `None` means "do not
/// override" (leave the engine default), which is the `Default` profile and an
/// empty `Custom` string.
pub fn resolve_profile(profile: &UaProfile, platform: UaPlatform) -> Option<String> {
    match profile {
        UaProfile::Default => None,
        UaProfile::Safari => Some(compatibility_user_agent(platform)),
        UaProfile::Chrome => Some(chrome_user_agent(platform)),
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
    platform: UaPlatform,
) -> Option<String> {
    let normalized = normalize_host(host);
    let profile = overrides.get(&normalized).unwrap_or(base);
    resolve_profile(profile, platform)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safari_ua_has_required_tokens_and_no_chrome() {
        let ua = compatibility_user_agent(UaPlatform::Mac);
        assert!(ua.contains("Macintosh"));
        assert!(ua.contains("AppleWebKit/605.1.15"));
        assert!(ua.contains("Version/"));
        assert!(ua.contains("Safari/605.1.15"));
        assert!(!ua.contains("Chrome"));
        assert!(!ua.contains("Chromium"));
    }

    #[test]
    fn chrome_ua_has_chrome_token() {
        let ua = chrome_user_agent(UaPlatform::Mac);
        assert!(ua.contains("Chrome/"));
        assert!(ua.contains("Safari/537.36"));
    }

    // Locks the macOS strings byte-for-byte so the per-OS refactor cannot
    // regress the established (and field-tested) Mac user agents.
    #[test]
    fn mac_strings_are_unchanged() {
        assert_eq!(
            compatibility_user_agent(UaPlatform::Mac),
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 \
             (KHTML, like Gecko) Version/18.5 Safari/605.1.15",
        );
        assert_eq!(
            chrome_user_agent(UaPlatform::Mac),
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36",
        );
    }

    #[test]
    fn platform_token_follows_os() {
        let win = compatibility_user_agent(UaPlatform::Windows);
        assert!(win.contains("Windows NT 10.0; Win64; x64"));
        assert!(!win.contains("Macintosh"));

        let linux = compatibility_user_agent(UaPlatform::Linux);
        assert!(linux.contains("X11; Linux x86_64"));
        assert!(!linux.contains("Macintosh"));
    }

    #[test]
    fn from_os_maps_known_targets_and_falls_back_to_mac() {
        assert_eq!(UaPlatform::from_os("windows"), UaPlatform::Windows);
        assert_eq!(UaPlatform::from_os("linux"), UaPlatform::Linux);
        assert_eq!(UaPlatform::from_os("macos"), UaPlatform::Mac);
        assert_eq!(UaPlatform::from_os("fogos"), UaPlatform::Mac);
    }

    #[test]
    fn default_profile_does_not_override() {
        assert_eq!(resolve_profile(&UaProfile::Default, UaPlatform::Mac), None);
    }

    #[test]
    fn profile_defaults_to_safari() {
        assert_eq!(UaProfile::default(), UaProfile::Safari);
        assert!(resolve_profile(&UaProfile::default(), UaPlatform::Mac).is_some());
    }

    #[test]
    fn empty_custom_does_not_override() {
        assert_eq!(
            resolve_profile(&UaProfile::Custom("   ".into()), UaPlatform::Mac),
            None
        );
        assert_eq!(
            resolve_profile(&UaProfile::Custom(" my-agent ".into()), UaPlatform::Mac),
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
            UaPlatform::Mac,
        )
        .unwrap();
        assert!(chosen.contains("Chrome/"));

        let base =
            ua_for_host("other.com", &overrides, &UaProfile::Safari, UaPlatform::Mac).unwrap();
        assert!(!base.contains("Chrome"));
    }
}
