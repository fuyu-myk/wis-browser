//! The cookie-notice and annoyance filter lists Settings offers as switches.

use crate::settings::UserSettings;

pub const COOKIE_NOTICES_LIST: &str = "https://secure.fanboy.co.nz/fanboy-cookiemonster_ubo.txt";
/// Fanboy's Annoyance list, which contains the whole cookie list.
pub const ANNOYANCES_LIST: &str = "https://secure.fanboy.co.nz/fanboy-annoyance_ubo.txt";

impl UserSettings {
    pub fn notice_list(&self) -> Option<&'static str> {
        if self.block_annoyances {
            Some(ANNOYANCES_LIST)
        } else if self.block_cookie_notices {
            Some(COOKIE_NOTICES_LIST)
        } else {
            None
        }
    }

    /// The user's subscriptions and the notice list, each once.
    pub fn subscribed_lists(&self) -> Vec<String> {
        let mut lists = self.filter_subscriptions.clone();
        if let Some(notices) = self.notice_list() {
            if !lists.iter().any(|url| url == notices) {
                lists.push(notices.to_owned());
            }
        }
        lists
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::UserSettingsPatch;

    fn with(cookie_notices: bool, annoyances: bool) -> UserSettings {
        UserSettings::default().with_patch(UserSettingsPatch {
            block_cookie_notices: Some(cookie_notices),
            block_annoyances: Some(annoyances),
            ..Default::default()
        })
    }

    #[test]
    fn cookie_notices_are_blocked_until_switched_off() {
        let mut older = serde_json::to_value(UserSettings::default()).unwrap();
        let fields = older.as_object_mut().unwrap();
        fields.remove("block_cookie_notices");
        fields.remove("block_annoyances");
        let read: UserSettings = serde_json::from_value(older).unwrap();

        assert_eq!(read.notice_list(), Some(COOKIE_NOTICES_LIST));
        assert_eq!(with(false, false).notice_list(), None);
    }

    #[test]
    fn annoyances_replace_the_cookie_list_they_contain() {
        assert_eq!(with(true, true).notice_list(), Some(ANNOYANCES_LIST));
        assert_eq!(with(false, true).notice_list(), Some(ANNOYANCES_LIST));
    }

    #[test]
    fn the_notice_list_joins_the_subscriptions_once() {
        let settings = with(true, false);
        let lists = settings.subscribed_lists();
        assert_eq!(lists.last().map(String::as_str), Some(COOKIE_NOTICES_LIST));
        assert_eq!(lists.len(), settings.filter_subscriptions.len() + 1);

        let subscribed = settings.with_patch(UserSettingsPatch {
            filter_subscriptions: Some(vec![COOKIE_NOTICES_LIST.to_owned()]),
            ..Default::default()
        });
        assert_eq!(
            subscribed.subscribed_lists(),
            vec![COOKIE_NOTICES_LIST.to_owned()]
        );

        assert_eq!(
            with(false, false).subscribed_lists(),
            UserSettings::default().filter_subscriptions
        );
    }
}
