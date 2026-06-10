//! Heuristic link-safety assessment for the "Peek" preview.
//!
//! Pure and dependency-light (only the `url` parser): given a link's href and
//! the visible anchor text, return a [`LinkAssessment`] flagging common phishing
//! and deception signals. No network calls, no allocation beyond the warnings.
//!
//! These are *heuristics*, not a verdict: they surface reasons to be cautious so
//! the preview can warn the user. The Peek webview is also isolated, so a click
//! on a malicious link cannot touch the main browser regardless of the score.

use serde::{Deserialize, Serialize};
use url::{Host, Url};

/// Overall caution level for a link, the maximum severity of its warnings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    /// Nothing notable, or only minor signals.
    Low,
    /// Worth a glance before continuing.
    Medium,
    /// Strong phishing/deception signal.
    High,
}

/// A single reason a link looked suspicious. `code` is a stable identifier
/// (for styling/tests); `message` is user-facing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkWarning {
    pub code: String,
    pub message: String,
}

impl LinkWarning {
    fn new(code: &str, message: &str) -> Self {
        Self {
            code: code.to_string(),
            message: message.to_string(),
        }
    }
}

/// The result of assessing a link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkAssessment {
    pub risk: RiskLevel,
    pub warnings: Vec<LinkWarning>,
}

impl LinkAssessment {
    fn from_warnings(severities: Vec<(RiskLevel, LinkWarning)>) -> Self {
        let risk = severities
            .iter()
            .map(|(level, _)| *level)
            .max_by_key(level_rank)
            .unwrap_or(RiskLevel::Low);
        Self {
            risk,
            warnings: severities.into_iter().map(|(_, w)| w).collect(),
        }
    }
}

fn level_rank(level: &RiskLevel) -> u8 {
    match level {
        RiskLevel::Low => 0,
        RiskLevel::Medium => 1,
        RiskLevel::High => 2,
    }
}

/// TLDs disproportionately abused for phishing/malware. Intentionally short and
/// conservative; presence is only a *low* signal on its own.
const RISKY_TLDS: &[&str] = &[
    "zip", "mov", "tk", "ml", "ga", "cf", "gq", "top", "work", "click", "country", "kim", "loan",
    "xyz", "rest", "fit",
];

fn strip_www(host: &str) -> &str {
    host.strip_prefix("www.").unwrap_or(host)
}

/// Best-effort extraction of a registrable-ish host from arbitrary anchor text,
/// used to detect "the text says one site, the link points to another".
fn host_from_text(text: &str) -> Option<String> {
    let t = text.trim();
    if t.is_empty() || t.len() > 200 {
        return None;
    }

    if let Ok(u) = Url::parse(t) {
        if let Some(h) = u.host_str() {
            return Some(strip_www(h).to_lowercase());
        }
    }

    // Bare domain like "paypal.com/login": no scheme, no spaces, has a dot.
    if !t.contains(char::is_whitespace) && t.contains('.') {
        if let Ok(u) = Url::parse(&format!("https://{t}")) {
            if let Some(h) = u.host_str() {
                let h = strip_www(h).to_lowercase();
                // Require a dot so plain words ("Click here.") don't match.
                if h.contains('.') {
                    return Some(h);
                }
            }
        }
    }

    None
}

/// Assess a link for display in the Peek preview.
///
/// `href` is the link target (ideally already absolute); `anchor_text` is the
/// link's visible text, used for deceptive-text detection.
pub fn assess_link(href: &str, anchor_text: Option<&str>) -> LinkAssessment {
    let mut out: Vec<(RiskLevel, LinkWarning)> = Vec::new();

    let parsed = match Url::parse(href) {
        Ok(u) => u,
        Err(_) => {
            return LinkAssessment::from_warnings(vec![(
                RiskLevel::High,
                LinkWarning::new(
                    "malformed_url",
                    "This link is malformed and may be deceptive.",
                ),
            )]);
        }
    };

    let scheme = parsed.scheme().to_ascii_lowercase();
    match scheme.as_str() {
        "http" => out.push((
            RiskLevel::Medium,
            LinkWarning::new(
                "insecure_http",
                "This site is not secure (HTTP); traffic can be read or altered.",
            ),
        )),
        "https" => {}
        "javascript" | "data" | "vbscript" | "file" | "blob" => out.push((
            RiskLevel::High,
            LinkWarning::new(
                "dangerous_scheme",
                "This link runs code or opens local content instead of a normal web page.",
            ),
        )),
        _ => out.push((
            RiskLevel::Medium,
            LinkWarning::new(
                "unusual_scheme",
                "This link uses an unusual address scheme.",
            ),
        )),
    }

    // Embedded credentials: "https://paypal.com@evil.example".
    if !parsed.username().is_empty() || parsed.password().is_some() {
        out.push((
            RiskLevel::High,
            LinkWarning::new(
                "credentials_in_url",
                "The address embeds a login before an '@', a common way to hide the real site.",
            ),
        ));
    }

    let host_str = parsed.host_str().map(|h| strip_www(h).to_lowercase());

    match parsed.host() {
        Some(Host::Ipv4(_)) | Some(Host::Ipv6(_)) => out.push((
            RiskLevel::Medium,
            LinkWarning::new(
                "ip_address_host",
                "This link points to a raw IP address rather than a named site.",
            ),
        )),
        _ => {}
    }

    if let Some(host) = host_str.as_deref() {
        // Punycode label: the `url` crate normalises Unicode hosts to ASCII
        // "xn--" labels, which is the homograph-attack signal.
        if host.split('.').any(|label| label.starts_with("xn--")) {
            out.push((
                RiskLevel::Medium,
                LinkWarning::new(
                    "idn_homograph",
                    "This domain uses international characters that can imitate another site.",
                ),
            ));
        }

        let labels: Vec<&str> = host.split('.').filter(|l| !l.is_empty()).collect();
        if labels.len() > 5 {
            out.push((
                RiskLevel::Medium,
                LinkWarning::new(
                    "many_subdomains",
                    "This address has unusually many subdomains, often used to look legitimate.",
                ),
            ));
        }

        if host.len() > 60 {
            out.push((
                RiskLevel::Low,
                LinkWarning::new("long_host", "This address is unusually long."),
            ));
        }

        if let Some(tld) = labels.last() {
            if RISKY_TLDS.contains(tld) {
                out.push((
                    RiskLevel::Low,
                    LinkWarning::new(
                        "uncommon_tld",
                        "This site uses a domain ending often associated with abuse.",
                    ),
                ));
            }
        }

        // Deceptive anchor text: visible text names a different site.
        if let Some(text) = anchor_text {
            if let Some(text_host) = host_from_text(text) {
                if text_host != host && !host.ends_with(&format!(".{text_host}")) {
                    out.push((
                        RiskLevel::High,
                        LinkWarning::new(
                            "deceptive_text",
                            "The link text names a different site than where it actually goes.",
                        ),
                    ));
                }
            }
        }
    }

    LinkAssessment::from_warnings(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(a: &LinkAssessment) -> Vec<String> {
        a.warnings.iter().map(|w| w.code.clone()).collect()
    }

    #[test]
    fn plain_https_is_low_risk_with_no_warnings() {
        let a = assess_link("https://example.com/page", Some("Example"));
        assert_eq!(a.risk, RiskLevel::Low);
        assert!(a.warnings.is_empty());
    }

    #[test]
    fn http_is_medium() {
        let a = assess_link("http://example.com/", None);
        assert_eq!(a.risk, RiskLevel::Medium);
        assert!(codes(&a).contains(&"insecure_http".to_string()));
    }

    #[test]
    fn embedded_credentials_are_high() {
        let a = assess_link("https://paypal.com@evil.example/login", None);
        assert_eq!(a.risk, RiskLevel::High);
        assert!(codes(&a).contains(&"credentials_in_url".to_string()));
    }

    #[test]
    fn javascript_scheme_is_high() {
        let a = assess_link("javascript:alert(1)", None);
        assert_eq!(a.risk, RiskLevel::High);
        assert!(codes(&a).contains(&"dangerous_scheme".to_string()));
    }

    #[test]
    fn punycode_host_flags_homograph() {
        // "xn--pple-43d.com" is a homograph of apple.com.
        let a = assess_link("https://xn--pple-43d.com/", None);
        assert!(codes(&a).contains(&"idn_homograph".to_string()));
    }

    #[test]
    fn ip_host_is_flagged() {
        let a = assess_link("https://192.168.1.1/admin", None);
        assert!(codes(&a).contains(&"ip_address_host".to_string()));
    }

    #[test]
    fn deceptive_anchor_text_is_high() {
        let a = assess_link("https://evil.example/login", Some("paypal.com"));
        assert_eq!(a.risk, RiskLevel::High);
        assert!(codes(&a).contains(&"deceptive_text".to_string()));
    }

    #[test]
    fn matching_anchor_text_is_not_deceptive() {
        let a = assess_link("https://paypal.com/login", Some("paypal.com"));
        assert!(!codes(&a).contains(&"deceptive_text".to_string()));
    }

    #[test]
    fn subdomain_of_text_host_is_not_deceptive() {
        let a = assess_link("https://login.paypal.com/", Some("paypal.com"));
        assert!(!codes(&a).contains(&"deceptive_text".to_string()));
    }

    #[test]
    fn non_domain_anchor_text_is_ignored() {
        let a = assess_link("https://example.com/", Some("Click here to continue"));
        assert!(!codes(&a).contains(&"deceptive_text".to_string()));
    }

    #[test]
    fn malformed_url_is_high() {
        let a = assess_link("ht!tp://nonsense", None);
        assert_eq!(a.risk, RiskLevel::High);
    }

    #[test]
    fn assessment_round_trips_as_snake_case() {
        let a = assess_link("http://example.com/", None);
        let json = serde_json::to_string(&a).unwrap();
        assert!(json.contains("\"risk\":\"medium\""));
        let back: LinkAssessment = serde_json::from_str(&json).unwrap();
        assert_eq!(a, back);
    }
}
