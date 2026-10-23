//! The blocking engine and WebKit rule lists, built from the cached filter
//! lists the settings subscribe to.

use std::path::Path;

use wis_core::privacy::trusted_lists::is_trusted_list;
use wis_core::UserSettings;
use wis_network::filter_lists::{
    load_cached_easylist, load_cached_resources, load_cached_subscriptions,
};
use wis_network::{NetworkResult, TrackerBlocker};

use crate::content_filter::{build_rule_specs_from, RuleSpecs};

/// Owned, so a build can run on a blocking thread.
pub struct Subscriptions {
    custom: Vec<String>,
    trusted: Vec<String>,
    notices: Option<&'static str>,
}

impl Subscriptions {
    pub fn of(settings: &UserSettings) -> Self {
        let notices = settings.notice_list();
        let (trusted, custom) = settings
            .filter_subscriptions
            .iter()
            .filter(|url| Some(url.as_str()) != notices)
            .cloned()
            .partition(|url| is_trusted_list(url));
        Self {
            custom,
            trusted,
            notices,
        }
    }
}

/// EasyList and the subscriptions feed the engine only; WebKit's rule cap
/// keeps them out of its lists, except the notice list's network rules.
pub fn build(
    cache_dir: &Path,
    filter_text: &str,
    subscriptions: &Subscriptions,
) -> NetworkResult<(TrackerBlocker, RuleSpecs)> {
    let easylist = load_cached_easylist(cache_dir);
    let custom = load_cached_subscriptions(cache_dir, &subscriptions.custom);
    let trusted = load_cached_subscriptions(cache_dir, &subscriptions.trusted);
    let notices = subscriptions
        .notices
        .and_then(|url| load_cached_subscriptions(cache_dir, &[url.to_owned()]).pop());

    let engine_texts: Vec<&str> = std::iter::once(filter_text)
        .chain(easylist.as_deref())
        .chain(custom.iter().map(String::as_str))
        .chain(notices.as_deref())
        .collect();
    let trusted_texts: Vec<&str> = trusted.iter().map(String::as_str).collect();
    let mut blocker =
        TrackerBlocker::from_filter_texts_with_cache(&engine_texts, &trusted_texts, cache_dir)?;
    if let Some(json) = load_cached_resources(cache_dir) {
        if let Err(err) = blocker.load_resources(&json) {
            log::warn!("filter_lists: scriptlet resources load failed: {err}");
        }
    }

    let notice_rules = notices
        .as_deref()
        .map(wis_network::content_filter::network_rules);
    Ok((
        blocker,
        build_rule_specs_from(filter_text, notice_rules.as_deref()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wis_core::privacy::trusted_lists::{UBO_FILTERS, UBO_QUICK_FIXES};
    use wis_core::UserSettingsPatch;

    #[test]
    fn ubo_s_own_lists_are_the_trusted_ones() {
        let custom = "https://lists.example/extra.txt".to_owned();
        let settings = UserSettings::default().with_patch(UserSettingsPatch {
            filter_subscriptions: Some(vec![
                UBO_FILTERS.to_owned(),
                custom.clone(),
                UBO_QUICK_FIXES.to_owned(),
            ]),
            ..Default::default()
        });
        let subscriptions = Subscriptions::of(&settings);

        assert_eq!(subscriptions.trusted, vec![UBO_FILTERS, UBO_QUICK_FIXES]);
        assert_eq!(subscriptions.custom, vec![custom]);
        assert_eq!(subscriptions.notices, settings.notice_list());
    }
}
