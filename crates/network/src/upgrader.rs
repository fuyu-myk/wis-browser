//! HTTPS-First / HTTPS-Only policy.
//!
//! The upgrader rewrites or blocks plaintext-HTTP requests according to the
//! active [`PrivacyMode`]:
//!
//! | Mode       | Behaviour                                  |
//! |------------|--------------------------------------------|
//! | Strict     | HTTPS only: block HTTP outright.          |
//! | Balanced   | HTTPS-First: try HTTPS, fall back to HTTP only if explicitly allowed by the caller. |
//! | Permissive | Leave HTTP alone.                          |
//!
//! Loopback (`localhost`, `127.0.0.1`, `::1`) is always exempt regardless of
//! mode, because dev servers need to work.

use url::{Host, Url};
use wis_core::PrivacyMode;

/// Outcome of running a URL through the upgrader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpgradeAction {
    /// Use the URL unchanged.
    Keep,
    /// Replace the URL's scheme with `https`.
    UpgradeToHttps(Url),
    /// Reject the request entirely.
    Block,
}

pub struct HttpsUpgrader {
    mode: PrivacyMode,
}

impl HttpsUpgrader {
    pub fn new(mode: PrivacyMode) -> Self {
        Self { mode }
    }

    pub fn decide(&self, url: &Url) -> UpgradeAction {
        if url.scheme() != "http" {
            return UpgradeAction::Keep;
        }
        if is_loopback(url) {
            return UpgradeAction::Keep;
        }

        match self.mode {
            PrivacyMode::Strict => match upgrade_scheme(url) {
                Some(upgraded) => UpgradeAction::UpgradeToHttps(upgraded),
                None => UpgradeAction::Block,
            },
            PrivacyMode::Balanced => match upgrade_scheme(url) {
                Some(upgraded) => UpgradeAction::UpgradeToHttps(upgraded),
                None => UpgradeAction::Keep,
            },
            PrivacyMode::Permissive => UpgradeAction::Keep,
        }
    }
}

/// Whether `url` points at this machine.
pub fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(d)) => d == "localhost" || d.ends_with(".localhost"),
        Some(Host::Ipv4(addr)) => addr.is_loopback(),
        Some(Host::Ipv6(addr)) => addr.is_loopback(),
        None => false,
    }
}

fn upgrade_scheme(url: &Url) -> Option<Url> {
    let mut upgraded = url.clone();
    upgraded.set_scheme("https").ok()?;
    Some(upgraded)
}
