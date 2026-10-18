//! What an extension may do beyond what it was installed with: the
//! permissions and sites it asked for while running and was allowed.

use crate::extensions::{abilities, is_site_pattern, with_changed, InstalledExtension};
use crate::{WisError, WisResult};

pub const MAX_GRANTED: usize = 64;
const MAX_GRANT: usize = 512;

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
