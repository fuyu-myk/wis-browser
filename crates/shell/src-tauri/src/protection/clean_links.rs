//! Tracking parameters come off links as they are followed, by the table
//! behind "Copy clean link" less the subscriber ids on subscription pages,
//! unless the destination site has blocking off in the site panel. On macOS
//! and Windows a link click in a tab, and the redirects it sets off, are
//! cancelled and re-navigated clean; WebKitGTK cannot tell a main-frame
//! navigation from a subframe's, so Linux leaves them. Links opened in new
//! tabs, Peek, the navigation guard and links from other apps are cleaned on
//! every platform.

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

/// What a WebView2 main-frame navigation start says about itself.
#[cfg(any(windows, test))]
struct Webview2Start {
    user_initiated: bool,
    redirected: bool,
    new_document: bool,
    /// A `Content-Type` header.
    has_body: bool,
}

/// The WebKit navigation type and method a WebView2 start stands for.
/// WebView2 counts its own API loads as user-initiated, so a typed load is
/// cleaned there too.
#[cfg(any(windows, test))]
fn webview2_navigation(start: &Webview2Start, continues_link: bool) -> (isize, &'static str) {
    let link = if start.redirected {
        continues_link
    } else {
        start.user_initiated && start.new_document
    };
    let navigation_type = if link { LINK_ACTIVATED } else { -1 };
    (navigation_type, if start.has_body { "POST" } else { "GET" })
}

/// Clean re-navigations one click may set off, so redirects bouncing between
/// sites cannot loop.
#[cfg(any(windows, test))]
const CLEAN_HOPS: u8 = 3;

#[cfg(any(windows, test))]
#[derive(Default)]
struct TabStarts {
    /// The link click whose redirects share this navigation id.
    link: Option<u64>,
    /// Wis's last clean re-navigation since the click.
    renavigated: Option<String>,
    hops: u8,
    /// The next start is that re-navigation, not a new click.
    expecting_own: bool,
}

/// What each tab's WebView2 starts need remembered, by label. Wis's own
/// re-navigation counts as user-initiated there, so unlike macOS its redirects
/// are cleaned too, and these guard against a server sending it back tracked.
#[cfg(any(windows, test))]
#[derive(Default)]
struct Webview2Tabs(std::collections::HashMap<String, TabStarts>);

#[cfg(any(windows, test))]
impl Webview2Tabs {
    fn replacement(
        &mut self,
        label: &str,
        id: u64,
        url: &str,
        start: &Webview2Start,
    ) -> Option<String> {
        let tab = self.0.entry(label.to_string()).or_default();
        if !start.redirected {
            if !std::mem::take(&mut tab.expecting_own) {
                tab.renavigated = None;
                tab.hops = 0;
            }
            tab.link = None;
        }
        let (navigation_type, method) = webview2_navigation(start, tab.link == Some(id));
        if !start.redirected && navigation_type == LINK_ACTIVATED && method == "GET" {
            tab.link = Some(id);
        }
        let clean = replacement(navigation_type, method, url, None)?;
        let sent_back = tab.renavigated.as_deref() == Some(clean.as_str());
        (!sent_back && tab.hops < CLEAN_HOPS).then_some(clean)
    }

    fn renavigating(&mut self, label: &str, clean: &str) {
        let tab = self.0.entry(label.to_string()).or_default();
        tab.hops += 1;
        tab.renavigated = Some(clean.to_string());
        tab.expecting_own = true;
    }

    fn prune(&mut self, open: impl Fn(&str) -> bool) {
        self.0.retain(|label, _| open(label));
    }
}

#[cfg(windows)]
pub fn install(app: AppHandle) {
    use std::cell::RefCell;

    /// Past this many remembered tabs, closed ones are dropped.
    const PRUNE_ABOVE: usize = 32;

    thread_local! {
        static TABS: RefCell<Webview2Tabs> = RefCell::default();
    }

    wry::set_navigation_start_hook(Box::new(move |start| {
        let label = start.webview_id;
        if !label.starts_with("tab-") || crate::canvas::page::is_canvas_tab(label) {
            return true;
        }

        let facts = Webview2Start {
            user_initiated: start.user_initiated,
            redirected: start.redirected,
            new_document: start.new_document,
            has_body: start.has_body,
        };
        let clean = TABS.with(|tabs| {
            let mut tabs = tabs.borrow_mut();
            if tabs.0.len() > PRUNE_ABOVE {
                tabs.prune(|label| app.get_webview(label).is_some());
            }
            tabs.replacement(label, start.navigation_id, start.url, &facts)
        });
        let Some(clean) = clean else {
            return true;
        };
        let Some(tab) = app.get_webview(label) else {
            return true;
        };
        if !app
            .state::<AppState>()
            .site_protection_for(start.url)
            .blocking
        {
            return true;
        }

        let Ok(target) = clean.parse() else {
            return true;
        };
        TABS.with(|tabs| tabs.borrow_mut().renavigating(label, &clean));
        tauri::async_runtime::spawn(async move {
            if let Err(err) = tab.navigate(target) {
                log::warn!("clean link navigation failed: {err}");
            }
        });

        false
    }));
}

#[cfg(target_os = "macos")]
pub fn install(app: AppHandle) {
    wry::set_main_frame_navigation_hook(Box::new(move |webview, url, navigation_type, method| {
        // SAFETY: wry calls the hook on the main thread with a live WKWebView.
        let label = unsafe { crate::platform::native_webview::label(webview) };
        if let Some(label) = &label {
            crate::protection::uncloak::opening_page(&app.state::<AppState>(), label, url);
        }
        if navigation_type != LINK_ACTIVATED || method != "GET" {
            return true;
        }

        // SAFETY: as above.
        let current = unsafe { crate::platform::native_webview::committed_url(webview) };
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

    const START: Webview2Start = Webview2Start {
        user_initiated: true,
        redirected: false,
        new_document: true,
        has_body: false,
    };

    #[test]
    fn a_webview2_user_load_counts_as_a_link_click() {
        assert_eq!(webview2_navigation(&START, false), (LINK_ACTIVATED, "GET"));
    }

    #[test]
    fn webview2_reloads_history_and_script_loads_are_not() {
        let reload = Webview2Start {
            new_document: false,
            ..START
        };
        let script = Webview2Start {
            user_initiated: false,
            ..START
        };
        assert_ne!(webview2_navigation(&reload, false).0, LINK_ACTIVATED);
        assert_ne!(webview2_navigation(&script, false).0, LINK_ACTIVATED);
    }

    #[test]
    fn a_webview2_redirect_counts_only_when_it_continues_a_link_click() {
        let redirect = Webview2Start {
            redirected: true,
            user_initiated: false,
            ..START
        };
        assert_eq!(webview2_navigation(&redirect, true).0, LINK_ACTIVATED);
        assert_ne!(webview2_navigation(&redirect, false).0, LINK_ACTIVATED);
    }

    #[test]
    fn a_webview2_request_with_a_body_is_a_post() {
        let form = Webview2Start {
            has_body: true,
            ..START
        };
        assert_eq!(webview2_navigation(&form, false).1, "POST");
    }

    const REDIRECT: Webview2Start = Webview2Start {
        user_initiated: false,
        redirected: true,
        ..START
    };

    /// Follows a start the way the hook does, re-navigating when told to.
    fn follow(
        tabs: &mut Webview2Tabs,
        id: u64,
        url: &str,
        start: &Webview2Start,
    ) -> Option<String> {
        let clean = tabs.replacement("tab-a", id, url, start)?;
        tabs.renavigating("tab-a", &clean);
        Some(clean)
    }

    #[test]
    fn a_webview2_link_click_and_its_redirect_are_cleaned() {
        let mut tabs = Webview2Tabs::default();
        assert_eq!(
            follow(&mut tabs, 1, TRACKED, &START).as_deref(),
            Some("https://a.example/p?id=7")
        );
        assert_eq!(
            follow(&mut tabs, 2, "https://a.example/p?id=7", &START),
            None
        );
        assert_eq!(
            follow(&mut tabs, 2, "https://b.example/?fbclid=X", &REDIRECT).as_deref(),
            Some("https://b.example/")
        );
    }

    #[test]
    fn a_webview2_redirect_of_another_tabs_link_is_left_alone() {
        let mut tabs = Webview2Tabs::default();
        assert_eq!(
            tabs.replacement("tab-a", 1, "https://shim.example/", &START),
            None
        );
        assert_eq!(tabs.replacement("tab-b", 1, TRACKED, &REDIRECT), None);
        assert!(tabs.replacement("tab-a", 1, TRACKED, &REDIRECT).is_some());
    }

    #[test]
    fn a_server_sending_the_clean_url_back_tracked_is_let_through() {
        let mut tabs = Webview2Tabs::default();
        let clean = follow(&mut tabs, 1, TRACKED, &START).expect("tracked click");
        assert_eq!(follow(&mut tabs, 2, &clean, &START), None);
        assert_eq!(follow(&mut tabs, 2, TRACKED, &REDIRECT), None);
    }

    #[test]
    fn redirects_bouncing_between_sites_stop_being_cleaned() {
        let mut tabs = Webview2Tabs::default();
        let mut renavigations = 0;
        let mut next = follow(&mut tabs, 1, "https://a.example/?utm_source=x", &START);
        let mut id = 1;
        while let Some(clean) = next {
            renavigations += 1;
            assert!(renavigations <= CLEAN_HOPS, "cleaning did not stop");
            id += 1;
            assert_eq!(follow(&mut tabs, id, &clean, &START), None);
            let bounce = if clean.contains("a.example") {
                "https://b.example/?utm_source=x"
            } else {
                "https://a.example/?utm_source=x"
            };
            next = follow(&mut tabs, id, bounce, &REDIRECT);
        }
    }

    #[test]
    fn a_new_click_cleans_afresh() {
        let mut tabs = Webview2Tabs::default();
        for id in 0..u64::from(CLEAN_HOPS) + 1 {
            assert!(follow(&mut tabs, id * 2, TRACKED, &START).is_some());
            assert_eq!(
                follow(&mut tabs, id * 2 + 1, "https://a.example/p?id=7", &START),
                None
            );
        }
    }

    #[test]
    fn closed_tabs_are_pruned() {
        let mut tabs = Webview2Tabs::default();
        tabs.replacement("tab-a", 1, TRACKED, &START);
        tabs.replacement("tab-b", 1, TRACKED, &START);
        tabs.prune(|label| label == "tab-b");
        assert_eq!(tabs.0.len(), 1);
        assert!(tabs.0.contains_key("tab-b"));
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
