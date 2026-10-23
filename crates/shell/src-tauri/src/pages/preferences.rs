//! WebKit's per-page preferences, set on every main-frame navigation of every
//! webview: the autoplay policy and the site's protection.

#[cfg(target_os = "macos")]
pub fn install_hook(app: tauri::AppHandle) {
    use tauri::Manager;

    let dev_chrome = tauri::is_dev()
        .then(|| app.config().build.dev_url.as_ref().map(url::Url::origin))
        .flatten();

    wry::set_webpage_preferences_hook(Box::new(move |url, preferences| {
        let state = app.state::<crate::state::AppState>();
        let autoplay = state.autoplay_policy_for(url);
        let protection = if is_page_of(dev_chrome.as_ref(), url) {
            wis_core::SiteProtection::default()
        } else {
            state.site_protection_for(url)
        };
        let wis_rule_lists = if protection.blocking {
            Vec::new()
        } else {
            state.wis_rule_list_ids()
        };
        // SAFETY: called by the navigation delegate on the main thread with a
        // live WKWebpagePreferences; the pointer is used only for these calls.
        unsafe {
            crate::tabs::autoplay::imp::apply(preferences.cast(), autoplay);
            crate::protection::site_protection::imp::apply(
                preferences.cast(),
                protection,
                &wis_rule_lists,
            );
        }
    }));
}

#[cfg(any(target_os = "macos", test))]
fn is_page_of(origin: Option<&url::Origin>, url: &str) -> bool {
    origin.is_some_and(|origin| url::Url::parse(url).is_ok_and(|u| u.origin() == *origin))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_dev_server_s_origin_is_the_dev_chrome() {
        let dev = url::Url::parse("http://localhost:3010/").unwrap().origin();
        assert!(is_page_of(
            Some(&dev),
            "http://localhost:3010/index.html?surface=site"
        ));
        assert!(!is_page_of(Some(&dev), "http://localhost:3000/"));
        assert!(!is_page_of(None, "http://localhost:3010/"));
        assert!(!is_page_of(Some(&dev), "not a url"));
    }
}
