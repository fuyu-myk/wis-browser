//! The filter lists allowed uBO's trusted scriptlets: uBO's own default lists,
//! as uBO itself allows.

pub const UBO_FILTERS: &str = "https://ublockorigin.github.io/uAssets/filters/filters.txt";
pub const UBO_QUICK_FIXES: &str = "https://ublockorigin.github.io/uAssets/filters/quick-fixes.txt";

pub fn is_trusted_list(url: &str) -> bool {
    url == UBO_FILTERS || url == UBO_QUICK_FIXES
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::privacy::notice_lists::{ANNOYANCES_LIST, COOKIE_NOTICES_LIST};
    use crate::settings::user_settings::UserSettings;

    #[test]
    fn only_ubo_s_default_lists_are_trusted() {
        for list in UserSettings::default().filter_subscriptions {
            assert!(is_trusted_list(&list), "{list}");
        }
        for list in [
            COOKIE_NOTICES_LIST,
            ANNOYANCES_LIST,
            "https://ublockorigin.github.io/uAssets/filters/annoyances-cookies.txt",
            "https://ublockorigin.github.io.evil.test/uAssets/filters/filters.txt",
            "http://ublockorigin.github.io/uAssets/filters/filters.txt",
        ] {
            assert!(!is_trusted_list(list), "{list}");
        }
    }
}
