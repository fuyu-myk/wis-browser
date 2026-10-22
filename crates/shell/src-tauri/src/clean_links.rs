//! Tracking parameters come off links as they are followed, by the table
//! behind "Copy clean link" less the subscriber ids on subscription pages,
//! unless the destination site has blocking off in the site panel. On macOS a
//! link click in a tab, and the redirects it sets off, are cancelled and
//! re-navigated clean. Links opened in new tabs, Peek, the navigation guard
//! and links from other apps are cleaned on every platform.

use tauri::{AppHandle, Manager};

use crate::state::AppState;

/// `WKNavigationTypeLinkActivated`. A redirect keeps the type of the
/// navigation it continues.
const LINK_ACTIVATED: isize = 0;

pub fn cleaned(app: &AppHandle, href: &str) -> String {
    let blocking = app
        .try_state::<AppState>()
        .is_none_or(|state| state.site_protection_for(href).blocking);
    followed(href, blocking)
}

fn followed(href: &str, blocking: bool) -> String {
    if !blocking {
        return href.to_string();
    }
    let cleaned = wis_core::clean_followed_link(href);
    if !cleaned.removed.is_empty() {
        log::debug!(
            "removed {} from a followed link",
            cleaned.removed.join(", ")
        );
    }
    cleaned.url
}

fn without_fragment(url: &str) -> &str {
    url.split_once('#').map_or(url, |(before, _)| before)
}

/// The clean URL to load instead of a link click or its redirect, before the
/// destination's site protection is consulted.
fn replacement(
    navigation_type: isize,
    method: &str,
    url: &str,
    current: Option<&str>,
) -> Option<String> {
    if navigation_type != LINK_ACTIVATED || method != "GET" {
        return None;
    }
    if current.is_some_and(|current| without_fragment(current) == without_fragment(url)) {
        return None;
    }
    let clean = wis_core::clean_followed_link(url).url;
    (clean != url).then_some(clean)
}

#[tauri::command]
pub fn clean_followed_link(app: AppHandle, url: String) -> String {
    cleaned(&app, &url)
}

#[cfg(target_os = "macos")]
pub fn install(app: AppHandle) {
    wry::set_main_frame_navigation_hook(Box::new(move |webview, url, navigation_type, method| {
        // SAFETY: wry calls the hook on the main thread with a live WKWebView.
        let label = unsafe { crate::native_webview::label(webview) };
        if let Some(label) = &label {
            crate::uncloak::opening_page(&app.state::<AppState>(), label, url);
        }
        if navigation_type != LINK_ACTIVATED || method != "GET" {
            return true;
        }

        // SAFETY: as above.
        let current = unsafe { crate::native_webview::committed_url(webview) };
        let Some(clean) = replacement(navigation_type, method, url, current.as_deref()) else {
            return true;
        };
        let Some(tab) = label.and_then(|label| app.get_webview(&label)) else {
            return true;
        };
        if !app.state::<AppState>().site_protection_for(url).blocking {
            return true;
        }

        let Ok(target) = clean.parse() else {
            return true;
        };
        tauri::async_runtime::spawn(async move {
            if let Err(err) = tab.navigate(target) {
                log::warn!("clean link navigation failed: {err}");
            }
        });

        false
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRACKED: &str = "https://a.example/p?id=7&utm_source=mail";

    #[test]
    fn a_followed_link_loses_its_tracking_parameters() {
        assert_eq!(
            followed(
                "https://a.example/p?id=7&utm_source=mail&fbclid=X#top",
                true
            ),
            "https://a.example/p?id=7#top"
        );
    }

    #[test]
    fn a_site_with_blocking_off_keeps_them() {
        assert_eq!(followed(TRACKED, false), TRACKED);
    }

    #[test]
    fn a_link_click_or_its_redirect_is_replaced_clean() {
        assert_eq!(
            replacement(LINK_ACTIVATED, "GET", TRACKED, Some("https://b.example/")),
            Some("https://a.example/p?id=7".into())
        );
        assert_eq!(
            replacement(LINK_ACTIVATED, "GET", TRACKED, None),
            Some("https://a.example/p?id=7".into())
        );
    }

    #[test]
    fn a_jump_within_the_page_is_left_alone() {
        let here = "https://a.example/p?id=7&utm_source=mail#intro";
        let anchor = "https://a.example/p?id=7&utm_source=mail#section";
        assert_eq!(replacement(LINK_ACTIVATED, "GET", anchor, Some(here)), None);
        assert_eq!(
            replacement(LINK_ACTIVATED, "GET", anchor, Some(TRACKED)),
            None
        );
    }

    #[test]
    fn other_navigations_and_clean_links_are_left_alone() {
        assert_eq!(replacement(LINK_ACTIVATED, "POST", TRACKED, None), None);
        assert_eq!(replacement(-1, "GET", TRACKED, None), None);
        assert_eq!(replacement(2, "GET", TRACKED, None), None);
        assert_eq!(
            replacement(LINK_ACTIVATED, "GET", "https://a.example/p?id=7", None),
            None
        );
    }
}
