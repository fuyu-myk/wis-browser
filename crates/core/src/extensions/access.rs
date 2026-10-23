//! Where an extension may run, and what it may do beyond what it was
//! installed with: the permissions and sites it asked for while running and
//! was allowed.

use serde::{Deserialize, Serialize};

use crate::extensions::installed::{abilities, is_site_pattern, with_changed, InstalledExtension};
use crate::{WisError, WisResult};

pub const MAX_GRANTED: usize = 64;
const MAX_GRANT: usize = 512;
pub const MAX_SITES: usize = 256;
const MAX_HOST: usize = 253;

/// Whether an extension runs on every site it asks for, or only on the
/// sites the user allowed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SiteAccess {
    #[default]
    All,
    Allowed,
}

pub fn with_site_access(
    list: &[InstalledExtension],
    id: &str,
    access: SiteAccess,
) -> WisResult<Vec<InstalledExtension>> {
    with_changed(list, id, |ext| ext.site_access = access)
}

/// A host as a page's URL gives it, lowercased; `None` for anything else,
/// such as a wildcard or a path.
pub fn plain_host(raw: &str) -> Option<String> {
    let host = raw.trim_end_matches('.').to_ascii_lowercase();
    let label_ok = |label: &str| {
        (1..=63).contains(&label.len())
            && label
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    (host.len() <= MAX_HOST && host.split('.').all(label_ok)).then_some(host)
}

/// Allows or stops an extension on a host, given as a page's host is.
pub fn with_site(
    list: &[InstalledExtension],
    id: &str,
    host: &str,
    allowed: bool,
) -> WisResult<Vec<InstalledExtension>> {
    let refuse = |message: &str| {
        Err(WisError::InvalidInput {
            message: message.into(),
        })
    };
    let Some(host) = plain_host(host) else {
        return refuse("not a site");
    };
    let sites = list
        .iter()
        .find(|e| e.id == id)
        .map(|e| e.allowed_sites.len())
        .unwrap_or_default();
    let listed = list
        .iter()
        .any(|e| e.id == id && e.allowed_sites.contains(&host));
    if allowed && !listed && sites >= MAX_SITES {
        return refuse("that extension is allowed on too many sites already");
    }

    with_changed(list, id, |ext| {
        ext.allowed_sites.retain(|s| *s != host);
        if allowed {
            ext.allowed_sites.push(host);
        }
    })
}

/// The match pattern that allows a host, over http and https.
pub fn site_pattern(host: &str) -> String {
    format!("*://{host}/*")
}

/// A permission or match pattern as WebKit spells it.
fn is_grant(item: &str) -> bool {
    !item.is_empty()
        && item.len() <= MAX_GRANT
        && !item.chars().any(|c| c.is_whitespace() || c.is_control())
}

pub fn with_granted(
    list: &[InstalledExtension],
    id: &str,
    items: &[String],
) -> WisResult<Vec<InstalledExtension>> {
    let current = list
        .iter()
        .find(|e| e.id == id)
        .map(|e| e.granted.clone())
        .unwrap_or_default();
    let mut granted = current;
    for item in items.iter().filter(|i| is_grant(i)) {
        if !granted.contains(item) {
            granted.push(item.clone());
        }
    }
    if granted.len() > MAX_GRANTED {
        return Err(WisError::InvalidInput {
            message: "that extension has been allowed too much already".into(),
        });
    }

    with_changed(list, id, |ext| ext.granted = granted)
}

pub fn without_granted(
    list: &[InstalledExtension],
    id: &str,
    items: &[String],
) -> WisResult<Vec<InstalledExtension>> {
    with_changed(list, id, |ext| ext.granted.retain(|g| !items.contains(g)))
}

/// How a request is put to the user, most far-reaching first. Empty when it
/// asks only for what reaches nothing, which is allowed without asking.
pub fn request_abilities(permissions: &[String], patterns: &[String]) -> Vec<String> {
    abilities(permissions, patterns)
}

/// What an extension was allowed while running, in words.
pub fn granted_abilities(ext: &InstalledExtension) -> Vec<String> {
    let (patterns, permissions): (Vec<String>, Vec<String>) = ext
        .granted
        .iter()
        .cloned()
        .partition(|g| is_site_pattern(g));
    abilities(&permissions, &patterns)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0123456789abcdef0123456789abcdef";

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn installed(granted: &[&str]) -> Vec<InstalledExtension> {
        vec![InstalledExtension {
            id: ID.into(),
            name: "Dark Reader".into(),
            granted: strings(granted),
            ..Default::default()
        }]
    }

    #[test]
    fn extensions_run_on_all_sites_until_narrowed() {
        assert_eq!(installed(&[])[0].site_access, SiteAccess::All);
        let list = with_site_access(&installed(&[]), ID, SiteAccess::Allowed).unwrap();
        assert_eq!(list[0].site_access, SiteAccess::Allowed);
        assert!(with_site_access(&list, "missing", SiteAccess::All).is_err());
    }

    #[test]
    fn sites_are_kept_once_by_their_host() {
        let list = with_site(&installed(&[]), ID, "En.Wikipedia.org.", true).unwrap();
        let list = with_site(&list, ID, "en.wikipedia.org", true).unwrap();
        assert_eq!(list[0].allowed_sites, strings(&["en.wikipedia.org"]));
        let list = with_site(&list, ID, "en.wikipedia.org", false).unwrap();
        assert!(list[0].allowed_sites.is_empty());
    }

    #[test]
    fn only_plain_hosts_are_sites() {
        for bad in [
            "",
            "*.example.com",
            "example.com/path",
            "[::1]",
            "a b.com",
            "exa_mple.com",
            ".com",
        ] {
            assert!(with_site(&installed(&[]), ID, bad, true).is_err(), "{bad}");
        }
        for good in ["localhost", "127.0.0.1", "xn--bcher-kva.example"] {
            assert!(with_site(&installed(&[]), ID, good, true).is_ok(), "{good}");
        }
    }

    #[test]
    fn sites_stop_at_the_cap() {
        let mut list = installed(&[]);
        for i in 0..MAX_SITES {
            list = with_site(&list, ID, &format!("s{i}.test"), true).unwrap();
        }
        assert!(with_site(&list, ID, "one-more.test", true).is_err());
        assert!(with_site(&list, ID, "s0.test", false).is_ok());
    }

    #[test]
    fn a_site_is_allowed_over_both_schemes() {
        assert_eq!(site_pattern("en.wikipedia.org"), "*://en.wikipedia.org/*");
    }

    #[test]
    fn older_settings_read_as_all_sites() {
        let ext: InstalledExtension =
            serde_json::from_str(&format!(r#"{{"id":"{ID}","name":"x","version":"1"}}"#)).unwrap();
        assert_eq!(ext.site_access, SiteAccess::All);
        assert!(ext.allowed_sites.is_empty());
    }

    #[test]
    fn grants_add_once_in_the_order_asked() {
        let list = with_granted(
            &installed(&["tabs"]),
            ID,
            &strings(&["https://a.test/*", "tabs"]),
        )
        .unwrap();
        assert_eq!(list[0].granted, strings(&["tabs", "https://a.test/*"]));
    }

    #[test]
    fn malformed_grants_are_left_out() {
        let long = "x".repeat(MAX_GRANT + 1);
        let asked = strings(&["", "two words", "bad\u{0}", &long, "cookies"]);
        assert_eq!(
            with_granted(&installed(&[]), ID, &asked).unwrap()[0].granted,
            strings(&["cookies"])
        );
    }

    #[test]
    fn grants_stop_at_the_cap() {
        let many: Vec<String> = (0..MAX_GRANTED)
            .map(|i| format!("https://{i}.test/*"))
            .collect();
        let full = with_granted(&installed(&[]), ID, &many).unwrap();
        assert!(with_granted(&full, ID, &strings(&["tabs"])).is_err());
        assert!(with_granted(&full, ID, &strings(&["https://0.test/*"])).is_ok());
    }

    #[test]
    fn grants_refuse_an_unknown_extension() {
        assert!(with_granted(&installed(&[]), "missing", &strings(&["tabs"])).is_err());
        assert!(without_granted(&installed(&[]), "missing", &strings(&["tabs"])).is_err());
    }

    #[test]
    fn removal_takes_only_what_is_named() {
        let list = without_granted(
            &installed(&["tabs", "cookies", "https://a.test/*"]),
            ID,
            &strings(&["cookies", "history"]),
        )
        .unwrap();
        assert_eq!(list[0].granted, strings(&["tabs", "https://a.test/*"]));
    }

    #[test]
    fn requests_for_what_reaches_nothing_need_no_words() {
        assert!(request_abilities(&strings(&["storage", "alarms"]), &[]).is_empty());
    }

    #[test]
    fn requests_name_sites_first_then_permissions() {
        assert_eq!(
            request_abilities(
                &strings(&["storage", "tabs"]),
                &strings(&["https://a.test/*"])
            ),
            strings(&[
                "Read and change your data on a.test",
                "Read the address and title of your tabs"
            ])
        );
        assert_eq!(
            request_abilities(&strings(&["nativeMessaging"]), &[]),
            strings(&["Use “nativeMessaging”"])
        );
    }

    #[test]
    fn granted_words_split_sites_from_permissions() {
        assert_eq!(
            granted_abilities(&installed(&["tabs", "<all_urls>"])[0]),
            strings(&[
                "Read and change everything on every website",
                "Read the address and title of your tabs"
            ])
        );
        assert!(granted_abilities(&installed(&["storage"])[0]).is_empty());
    }

    #[test]
    fn older_settings_read_with_nothing_granted() {
        let ext: InstalledExtension = serde_json::from_str(&format!(
            r#"{{"id":"{ID}","name":"Dark Reader","version":"1"}}"#
        ))
        .unwrap();
        assert!(ext.granted.is_empty());
    }

    #[test]
    fn wrong_errors_are_not_hidden() {
        let err = with_granted(&installed(&[]), "missing", &strings(&["tabs"])).unwrap_err();
        assert!(matches!(err, WisError::InvalidInput { .. }));
    }
}
