//! Autoplay policy enforcement.
//!
//! On macOS, WebKit's per-page autoplay policy is set on every main-frame
//! navigation through the vendored wry hook (`WKWebpagePreferences`
//! `_autoplayPolicy`, SPI guarded by `respondsToSelector:`), so the per-site
//! table applies there and a change reaches a page on its next load.
//!
//! On Linux, WebKitGTK's `media-playback-requires-user-gesture` is toggled per
//! webview, at creation and when each main-frame load starts. The setting
//! cannot separate sound, so "block audible" blocks all media there.
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

/// Register the per-navigation hook with wry. Call once at setup.
#[cfg(target_os = "macos")]
pub fn install_hook(app: tauri::AppHandle) {
    use tauri::Manager;

    wry::set_webpage_preferences_hook(Box::new(move |url, preferences| {
        let policy = app
            .state::<crate::state::AppState>()
            .autoplay_policy_for(url);
        // SAFETY: called by the navigation delegate on the main thread with a
        // live WKWebpagePreferences; the pointer is used only for this call.
        unsafe { imp::apply(preferences.cast(), policy) };
    }));
}

#[cfg(target_os = "macos")]
mod imp {
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

/// Apply `policy` to a live webview's WebKitGTK settings.
#[cfg(target_os = "linux")]
pub fn apply<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>, policy: AutoplayPolicy) {
    use webkit2gtk::{SettingsExt, WebViewExt};

    let _ = webview.with_webview(move |w| {
        if let Some(settings) = w.inner().settings() {
            settings.set_media_playback_requires_user_gesture(policy != AutoplayPolicy::Allow);
        }
    });
}

/// Re-apply the policy for the main-frame document `webview`.
#[cfg(target_os = "linux")]
pub fn apply_for_page_load<R: tauri::Runtime>(
    app: &tauri::AppHandle,
    webview: &tauri::webview::Webview<R>,
    url: &str,
) {
    use tauri::Manager;

    let policy = app
        .state::<crate::state::AppState>()
        .autoplay_policy_for(url);
    apply(webview, policy);
}

/// Push a changed setting to every live remote webview, each under the
/// policy for the page it shows.
#[cfg(target_os = "linux")]
pub fn apply_to_all_remote(app: &tauri::AppHandle) {
    use tauri::Manager;

    let state = app.state::<crate::state::AppState>();
    for (label, webview) in app.webviews() {
        if crate::webview_prefs::is_remote(&label) {
            let url = webview.url().map(|u| u.to_string()).unwrap_or_default();
            apply(&webview, state.autoplay_policy_for(&url));
        }
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
}
