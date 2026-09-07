//! Per-site page zoom.
//!
//! The percentages live in `UserSettings`; this is the sync mirror the
//! page-load hook reads on the main thread, plus the helpers that push a
//! factor into live webviews. Zooming a page writes the site's percentage,
//! and resetting removes it.

use std::collections::HashMap;

/// Sync mirror of the zoom settings, read per navigation.
pub struct ZoomCache {
    pub default_percent: u16,
    pub overrides: HashMap<String, u16>,
}

/// The zoom for the page at `url`. A URL with no host (a chrome page, or
/// `about:blank`) takes the default.
pub fn percent_for_url(cache: &ZoomCache, url: &str) -> u16 {
    let host = url::Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(str::to_owned))
        .unwrap_or_default();
    wis_core::zoom_percent_for_host(&host, &cache.overrides, cache.default_percent)
}

/// Re-apply the right zoom to every live tab, each from its own URL. For a
/// settings change, which can move any number of sites at once.
pub fn apply_to_all_tabs(app: &tauri::AppHandle) {
    use tauri::Manager;

    let state = app.state::<crate::state::AppState>();
    for (label, webview) in app.webviews() {
        if !label.starts_with("tab-") {
            continue;
        }

        let url = webview.url().map(|u| u.to_string()).unwrap_or_default();
        let percent = state.zoom_percent_for_url(&url);

        crate::commands::apply_zoom_to_webview(&webview, wis_core::zoom_factor(percent));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache() -> ZoomCache {
        ZoomCache {
            default_percent: 110,
            overrides: HashMap::from([("docs.example".to_string(), 120)]),
        }
    }

    #[test]
    fn the_url_s_host_selects_the_zoom() {
        assert_eq!(
            percent_for_url(&cache(), "https://www.docs.example/a/b"),
            120
        );
        assert_eq!(percent_for_url(&cache(), "https://news.example/"), 110);
    }

    #[test]
    fn a_url_without_a_host_takes_the_default() {
        assert_eq!(percent_for_url(&cache(), "about:blank"), 110);
        assert_eq!(percent_for_url(&cache(), "not a url"), 110);
    }
}
