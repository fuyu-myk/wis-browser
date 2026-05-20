//! Privacy presets and per-site overrides. These types describe policy only;
//! enforcement lives in the network proxy.

use serde::{Deserialize, Serialize};

/// Browser-wide privacy preset. Strict blocks the most; Permissive blocks the least.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyMode {
    /// HTTPS-only, all third-party blocked, all tracker lists active, DoH mandatory.
    Strict,
    /// HTTPS-First, third-party cookies blocked, tracker blocking on. Recommended default.
    #[default]
    Balanced,
    /// User controls each setting individually; minimal automatic blocking.
    Permissive,
}

/// Per-host overrides on top of the active [`PrivacyMode`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PerSitePolicy {
    pub host: String,
    pub allow_javascript: bool,
    pub allow_cookies: bool,
    pub allow_third_party: bool,
    pub allow_camera: bool,
    pub allow_microphone: bool,
    pub allow_location: bool,
}

impl PerSitePolicy {
    /// Return the default per-site policy implied by a given [`PrivacyMode`].
    pub fn default_for_mode(host: String, mode: PrivacyMode) -> Self {
        match mode {
            PrivacyMode::Strict => Self {
                host,
                allow_javascript: false,
                allow_cookies: false,
                allow_third_party: false,
                allow_camera: false,
                allow_microphone: false,
                allow_location: false,
            },
            PrivacyMode::Balanced => Self {
                host,
                allow_javascript: true,
                allow_cookies: true,
                allow_third_party: false,
                allow_camera: false,
                allow_microphone: false,
                allow_location: false,
            },
            PrivacyMode::Permissive => Self {
                host,
                allow_javascript: true,
                allow_cookies: true,
                allow_third_party: true,
                allow_camera: false,
                allow_microphone: false,
                allow_location: false,
            },
        }
    }
}
