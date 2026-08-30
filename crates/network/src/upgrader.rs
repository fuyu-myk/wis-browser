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

/// Whether `url` is a public web URL the assistant may fetch on a model's
/// behalf. Model-supplied URLs are an SSRF vector: everything that is not
/// plainly http(s) to a public host is refused. This includes loopback,
/// private and link-local ranges, credentialed URLs, and bare non-ICANN
/// hosts.
pub fn is_public_web_url(url: &Url) -> bool {
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }

    match url.host() {
        None => false,
        Some(Host::Ipv4(addr)) => is_public_ipv4(addr),
        Some(Host::Ipv6(addr)) => {
            if let Some(mapped) = addr.to_ipv4_mapped() {
                return is_public_ipv4(mapped);
            }

            !(addr.is_loopback()
                || addr.is_unspecified()
                // Unique-local fc00::/7 and link-local fe80::/10.
                || (addr.segments()[0] & 0xfe00) == 0xfc00
                || (addr.segments()[0] & 0xffc0) == 0xfe80)
        }
        Some(Host::Domain(d)) => {
            d.contains('.')
                && !d.ends_with(".localhost")
                && !d.ends_with(".local")
                && !d.ends_with(".internal")
                && !d.ends_with(".home.arpa")
                && d != "localhost"
        }
    }
}

fn is_public_ipv4(addr: std::net::Ipv4Addr) -> bool {
    !(addr.is_loopback()
        || addr.is_private()
        || addr.is_link_local()
        || addr.is_broadcast()
        || addr.is_documentation()
        || addr.is_unspecified()
        // CGNAT 100.64.0.0/10.
        || (addr.octets()[0] == 100 && (addr.octets()[1] & 0xc0) == 64))
}

fn upgrade_scheme(url: &Url) -> Option<Url> {
    let mut upgraded = url.clone();
    upgraded.set_scheme("https").ok()?;
    Some(upgraded)
}
