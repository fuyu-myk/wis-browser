//! Autoplay policy enforcement.
//!
//! On macOS, WebKit's per-page autoplay policy is set on every main-frame
//! navigation through the vendored wry hook (`WKWebpagePreferences`
//! `_autoplayPolicy`, SPI guarded by `respondsToSelector:`), so the per-site
//! table applies there and a change reaches a page on its next load.
//!
//! On Linux, each navigation wry accepts carries its site's policy as WebKitGTK
//! website policies (`wry::set_autoplay_policy_hook`), sound included, and a
//! change reaches a page on its next load. As on macOS, media in every frame
//! follows the page's policy. WebKitGTK raises the decision for subframes too
//! without naming the frame, so a frame's own URL only sets whether its media
//! preloads.
//!
//! On Windows, there is no runtime knob: WebView2's own rule (sound after the
//! user interacts with the page) stays.

use std::collections::HashMap;

use wis_core::{autoplay_policy_for_host, AutoplayPolicy};

/// Sync mirror of the two settings, read on the main thread per navigation.
pub struct AutoplayCache {
    pub policy: AutoplayPolicy,
    pub overrides: HashMap<String, AutoplayPolicy>,
}

/// The policy for the page at `url`.
pub fn policy_for_url(cache: &AutoplayCache, url: &str) -> AutoplayPolicy {
    let host = url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_default();
    autoplay_policy_for_host(&host, &cache.overrides, cache.policy)
}

#[cfg(target_os = "macos")]
pub mod imp {
    use objc2::runtime::AnyObject;
    use objc2::{msg_send, sel};

    use wis_core::AutoplayPolicy;

    /// `_WKWebsiteAutoplayPolicy`: Default = 0, Allow = 1,
    /// AllowWithoutSound = 2, Deny = 3.
    fn wk_value(policy: AutoplayPolicy) -> isize {
        match policy {
            AutoplayPolicy::Allow => 1,
            AutoplayPolicy::BlockAudible => 2,
            AutoplayPolicy::Block => 3,
        }
    }

    pub unsafe fn apply(preferences: *mut AnyObject, policy: AutoplayPolicy) {
        if preferences.is_null() {
            return;
        }
        let supported: bool = msg_send![preferences, respondsToSelector: sel!(_setAutoplayPolicy:)];
        if supported {
            let _: () = msg_send![preferences, _setAutoplayPolicy: wk_value(policy)];
        }
    }
}

/// Give every accepted navigation the policy for its URL.
#[cfg(target_os = "linux")]
pub fn install_hook(app: tauri::AppHandle) {
    use tauri::Manager;

    wry::set_autoplay_policy_hook(Box::new(move |url| {
        let policy = app
            .state::<crate::state::AppState>()
            .autoplay_policy_for(url);
        Some(gtk_policy(policy))
    }));
}

#[cfg(target_os = "linux")]
fn gtk_policy(policy: AutoplayPolicy) -> webkit2gtk::AutoplayPolicy {
    match policy {
        AutoplayPolicy::Allow => webkit2gtk::AutoplayPolicy::Allow,
        AutoplayPolicy::BlockAudible => webkit2gtk::AutoplayPolicy::AllowWithoutSound,
        AutoplayPolicy::Block => webkit2gtk::AutoplayPolicy::Deny,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_url_s_host_selects_the_override() {
        let cache = AutoplayCache {
            policy: AutoplayPolicy::Block,
            overrides: HashMap::from([("video.example".to_string(), AutoplayPolicy::Allow)]),
        };
        assert_eq!(
            policy_for_url(&cache, "https://www.video.example/watch?v=1"),
            AutoplayPolicy::Allow
        );
        assert_eq!(
            policy_for_url(&cache, "https://news.example/"),
            AutoplayPolicy::Block
        );
        assert_eq!(policy_for_url(&cache, "about:blank"), AutoplayPolicy::Block);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn each_setting_maps_to_its_webkitgtk_policy() {
        use webkit2gtk::AutoplayPolicy as Gtk;

        assert_eq!(gtk_policy(AutoplayPolicy::Allow), Gtk::Allow);
        assert_eq!(
            gtk_policy(AutoplayPolicy::BlockAudible),
            Gtk::AllowWithoutSound
        );
        assert_eq!(gtk_policy(AutoplayPolicy::Block), Gtk::Deny);
    }
}
