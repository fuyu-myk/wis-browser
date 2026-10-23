//! Updating extensions from the Chrome Web Store: whether a version is newer
//! and whether it requires more permissions. An update awaits user approval
//! in the latter case.

use serde::{Deserialize, Serialize};

use crate::extensions::installed::{
    abilities, tidy, tidy_name, with_changed, InstalledExtension, MAX_VERSION,
};
use crate::WisResult;

/// An update that asks for more than the installed version, until approved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingUpdate {
    pub version: String,
    /// What it would add, in words.
    pub abilities: Vec<String>,
    /// Additional permissions it would require.
    #[serde(default)]
    pub reach: Reach,
}

/// What a version of an extension may reach: the permissions WebKit
/// recognised and the match patterns it asks for.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reach {
    pub permissions: Vec<String>,
    pub patterns: Vec<String>,
}

/// Whether `candidate` is a later Chrome extension version than `current`.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    let Some(candidate) = parts(candidate) else {
        return false;
    };
    candidate > parts(current).unwrap_or_default()
}

/// A version's four numbers, missing ones as zero.
fn parts(version: &str) -> Option<[u16; 4]> {
    let numbers = version
        .split('.')
        .map(|part| {
            part.bytes()
                .all(|b| b.is_ascii_digit())
                .then(|| part.parse::<u16>().ok())
                .flatten()
        })
        .collect::<Option<Vec<u16>>>()?;
    if !(1..=4).contains(&numbers.len()) {
        return None;
    }

    let mut padded = [0; 4];
    padded[..numbers.len()].copy_from_slice(&numbers);
    Some(padded)
}

/// Whether `next` reaches anything `installed` does not.
pub fn wants_more(installed: &Reach, next: &Reach) -> bool {
    next.permissions
        .iter()
        .any(|p| !installed.permissions.contains(p))
        || next
            .patterns
            .iter()
            .any(|p| !installed.patterns.contains(p))
}

/// What `next` could do that `installed` could not, in words.
pub fn added_abilities(installed: &Reach, next: &Reach) -> Vec<String> {
    let before = abilities(&installed.permissions, &installed.patterns);
    abilities(&next.permissions, &next.patterns)
        .into_iter()
        .filter(|a| !before.contains(a))
        .collect()
}

/// Records an installed update.
pub fn with_update(
    list: &[InstalledExtension],
    id: &str,
    name: &str,
    version: &str,
) -> WisResult<Vec<InstalledExtension>> {
    with_changed(list, id, |ext| {
        ext.name = tidy_name(name);
        ext.version = tidy(version, MAX_VERSION);
        ext.pending_update = None;
    })
}

pub fn with_pending_update(
    list: &[InstalledExtension],
    id: &str,
    pending: Option<PendingUpdate>,
) -> WisResult<Vec<InstalledExtension>> {
    with_changed(list, id, |ext| ext.pending_update = pending)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0123456789abcdef0123456789abcdef";

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn reach(permissions: &[&str], patterns: &[&str]) -> Reach {
        Reach {
            permissions: strings(permissions),
            patterns: strings(patterns),
        }
    }

    fn installed() -> Vec<InstalledExtension> {
        vec![InstalledExtension {
            id: ID.into(),
            name: "Dark Reader".into(),
            version: "4.9.133".into(),
            ..Default::default()
        }]
    }

    #[test]
    fn versions_compare_part_by_part() {
        assert!(is_newer("4.9.140", "4.9.133"));
        assert!(is_newer("4.10", "4.9.200"));
        assert!(is_newer("1.0.0.1", "1.0"));
        assert!(!is_newer("1.0.0", "1.0"));
        assert!(!is_newer("4.9.133", "4.9.133"));
        assert!(!is_newer("4.9.100", "4.9.133"));
    }

    #[test]
    fn odd_versions_are_never_newer() {
        for odd in ["", "5.0-beta", "5..0", "1.2.3.4.5", "99999999999", " 5.0"] {
            assert!(!is_newer(odd, "1.0"), "{odd}");
        }
        assert!(is_newer("1.1", "not a version"));
    }

    #[test]
    fn an_update_that_drops_or_keeps_reach_wants_nothing_more() {
        let old = reach(&["storage", "tabs"], &["<all_urls>"]);
        assert!(!wants_more(&old, &reach(&["storage"], &["<all_urls>"])));
        assert!(!wants_more(&old, &old));
    }

    #[test]
    fn an_update_with_a_new_permission_or_site_wants_more() {
        let old = reach(&["storage"], &["https://a.test/*"]);
        assert!(wants_more(
            &old,
            &reach(&["storage", "cookies"], &["https://a.test/*"])
        ));
        assert!(wants_more(
            &old,
            &reach(&["storage"], &["https://a.test/*", "https://b.test/*"])
        ));
    }

    #[test]
    fn what_an_update_adds_is_put_in_words() {
        let old = reach(&["storage"], &["https://a.test/*"]);
        let next = reach(&["storage", "tabs"], &["<all_urls>"]);
        assert_eq!(
            added_abilities(&old, &next),
            strings(&[
                "Read and change everything on every website",
                "Read the address and title of your tabs"
            ])
        );
    }

    #[test]
    fn an_installed_update_takes_its_version_and_clears_what_waited() {
        let waiting = with_pending_update(
            &installed(),
            ID,
            Some(PendingUpdate {
                version: "4.9.140".into(),
                abilities: strings(&["Show notifications"]),
                reach: reach(&["notifications"], &[]),
            }),
        )
        .unwrap();
        assert_eq!(
            waiting[0].pending_update.as_ref().unwrap().version,
            "4.9.140"
        );
        let updated = with_update(&waiting, ID, "Dark\u{202e} Reader", "4.9.140").unwrap();
        assert_eq!(updated[0].version, "4.9.140");
        assert_eq!(updated[0].name, "Dark Reader");
        assert!(updated[0].pending_update.is_none());
        assert!(with_update(&updated, "missing", "x", "1").is_err());
    }
}
