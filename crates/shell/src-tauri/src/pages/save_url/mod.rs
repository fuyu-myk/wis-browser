//! "Save as" for a link, image, video or audio under the page menu.
//!
//! The tab's own engine downloads the URL in its session, so the request
//! carries the site's cookies, streams to disk and is tracked like any other
//! download. WebView2 cannot start a download, so on Windows Wis fetches the
//! URL itself in the tab's session (`fetch_win`).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};
use url::Url;
use wis_core::{TabId, WisError, WisResult};

#[cfg(windows)]
mod fetch_win;
#[cfg(windows)]
use fetch_win::start;

const MARK_TTL: Duration = Duration::from_secs(120);
/// Bounds what a burst of saves that never started can leave behind.
const MAX_MARKS: usize = 16;
struct Mark {
    url: Url,
    /// The window that asked, for a failure the page menu cannot see.
    window: String,
    at: Instant,
}

/// Downloads that ask where they go.
struct Marks(Vec<Mark>);

impl Marks {
    fn mark(&mut self, url: Url, window: String, now: Instant) {
        self.expire(now);
        if self.0.len() == MAX_MARKS {
            self.0.remove(0);
        }
        self.0.push(Mark {
            url,
            window,
            at: now,
        });
    }

    fn take(&mut self, url: &Url, now: Instant) -> Option<String> {
        self.expire(now);
        let index = self.0.iter().position(|mark| mark.url == *url)?;
        Some(self.0.remove(index).window)
    }

    fn expire(&mut self, now: Instant) {
        self.0.retain(|mark| now.duration_since(mark.at) < MARK_TTL);
    }
}

static MARKS: Mutex<Marks> = Mutex::new(Marks(Vec::new()));

fn mark(reported: &str, window: String) {
    if let Ok(url) = Url::parse(reported) {
        MARKS
            .lock()
            .expect("save-as marks poisoned")
            .mark(url, window, Instant::now());
    }
}

/// The window that asked for `url`s download with "Save as", using up the
/// mark. `None` for any other download.
pub(crate) fn take_mark(url: &Url) -> Option<String> {
    MARKS
        .lock()
        .expect("save-as marks poisoned")
        .take(url, Instant::now())
}

/// A download that finished without ever being tracked failed before any
/// response, so its destination was never asked for.
pub(crate) fn finished_untracked(app: &AppHandle, url: &Url, success: bool) {
    if success {
        return;
    }
    if let Some(window) = take_mark(url) {
        crate::chrome::remote_toast::show(app, &window, "The download could not start");
    }
}

fn invalid(message: impl Into<String>) -> WisError {
    WisError::InvalidInput {
        message: message.into(),
    }
}

/// An http(s) URL, the only kind a page download may take.
fn web_url(raw: &str) -> WisResult<Url> {
    match Url::parse(raw) {
        Ok(url) if matches!(url.scheme(), "http" | "https") => Ok(url),
        _ => Err(WisError::InvalidUrl {
            url: raw.to_string(),
        }),
    }
}

/// What the page menu saves.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SaveTarget {
    Link,
    Source,
}

fn saved_url(raw: &str, target: SaveTarget, clean: impl FnOnce(&str) -> String) -> WisResult<Url> {
    match target {
        SaveTarget::Link => web_url(&clean(raw)),
        SaveTarget::Source => web_url(raw),
    }
}

/// Download `url` in the tab's own session, asking where it goes.
#[tauri::command]
pub async fn save_url_as(
    app: AppHandle,
    tab_id: TabId,
    url: String,
    target: SaveTarget,
) -> WisResult<()> {
    let url = saved_url(&url, target, |raw| {
        crate::protection::clean_links::cleaned(&app, raw)
    })?;
    let webview = app
        .get_webview(&format!("tab-{}", tab_id.as_uuid()))
        .ok_or(WisError::TabNotFound { id: tab_id })?;

    start(&app, tab_id, &webview, url).await
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
async fn start(_: &AppHandle, _: TabId, webview: &tauri::Webview, url: Url) -> WisResult<()> {
    let window = webview.window().label().to_string();
    let (tx, rx) = tokio::sync::oneshot::channel();
    webview
        .with_webview(move |w| {
            #[cfg(target_os = "macos")]
            let started = {
                // SAFETY: `with_webview` runs this on the main thread, and
                // `inner` is the tab's live WKWebView.
                let wk = unsafe { &*(w.inner() as *const objc2_web_kit::WKWebView) };
                wry::start_download(wk, url.as_str(), move |reported| mark(&reported, window))
            };
            #[cfg(target_os = "linux")]
            let started = {
                use webkit2gtk::{DownloadExt, URIRequestExt, WebViewExt};
                let download = w.inner().download_uri(url.as_str());
                if let Some(reported) = download
                    .as_ref()
                    .and_then(|download| download.request())
                    .and_then(|request| request.uri())
                {
                    mark(&reported, window);
                }
                download.is_some()
            };
            let _ = tx.send(started);
        })
        .map_err(|err| invalid(format!("reaching the tab: {err}")))?;

    match rx.await {
        Ok(true) => Ok(()),
        _ => Err(invalid("the download could not start")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(path: &str) -> Url {
        Url::parse(&format!("https://example.com/{path}")).unwrap()
    }

    fn marks_of(urls: &[Url], now: Instant) -> Marks {
        let mut marks = Marks(Vec::new());
        for url in urls {
            marks.mark(url.clone(), "main".into(), now);
        }
        marks
    }

    #[test]
    fn a_mark_is_taken_once_with_its_window() {
        let now = Instant::now();
        let mut marks = Marks(Vec::new());
        marks.mark(url("a.zip"), "second".into(), now);
        assert_eq!(marks.take(&url("a.zip"), now).as_deref(), Some("second"));
        assert_eq!(marks.take(&url("a.zip"), now), None);
    }

    #[test]
    fn an_unmarked_url_is_not_taken() {
        let now = Instant::now();
        let mut marks = marks_of(&[url("a.zip")], now);
        assert_eq!(marks.take(&url("b.zip"), now), None);
        assert!(marks.take(&url("a.zip"), now).is_some());
    }

    #[test]
    fn two_saves_of_one_url_are_two_marks() {
        let now = Instant::now();
        let mut marks = marks_of(&[url("a.zip"), url("a.zip")], now);
        assert!(marks.take(&url("a.zip"), now).is_some());
        assert!(marks.take(&url("a.zip"), now).is_some());
        assert_eq!(marks.take(&url("a.zip"), now), None);
    }

    #[test]
    fn a_mark_expires() {
        let now = Instant::now();
        let mut marks = marks_of(&[url("a.zip")], now);
        assert_eq!(marks.take(&url("a.zip"), now + MARK_TTL), None);
    }

    #[test]
    fn the_oldest_mark_goes_past_the_cap() {
        let now = Instant::now();
        let urls: Vec<Url> = (0..=MAX_MARKS).map(|i| url(&i.to_string())).collect();
        let mut marks = marks_of(&urls, now);
        assert_eq!(marks.take(&url("0"), now), None);
        assert!(marks.take(&url(&MAX_MARKS.to_string()), now).is_some());
    }

    fn clean(href: &str) -> String {
        wis_core::clean_followed_link(href).url
    }

    #[test]
    fn a_link_is_saved_as_following_it_would_go() {
        let raw = "https://example.com/a.zip?utm_source=feed";
        let saved = saved_url(raw, SaveTarget::Link, clean);
        assert_eq!(saved.unwrap().as_str(), "https://example.com/a.zip");
    }

    #[test]
    fn a_source_is_saved_as_the_page_loaded_it() {
        let raw = "https://cdn.example.com/a.png?utm_source=feed";
        let saved = saved_url(raw, SaveTarget::Source, clean);
        assert_eq!(saved.unwrap().as_str(), raw);
    }

    #[test]
    fn the_target_is_named_as_the_chrome_sends_it() {
        let parse = serde_json::from_str::<SaveTarget>;
        assert!(matches!(parse("\"link\""), Ok(SaveTarget::Link)));
        assert!(matches!(parse("\"source\""), Ok(SaveTarget::Source)));
        assert!(parse("\"Source\"").is_err());
        assert!(parse("\"other\"").is_err());
    }

    #[test]
    fn only_web_urls_are_saved() {
        assert!(web_url("https://example.com/a.zip").is_ok());
        assert!(web_url("http://intranet.local/a.zip").is_ok());
        for raw in [
            "file:///etc/hosts",
            "data:text/plain,hi",
            "javascript:alert(1)",
            "ftp://example.com/a.zip",
            "blob:https://example.com/0f0e",
            "not a url",
        ] {
            assert!(
                matches!(web_url(raw), Err(WisError::InvalidUrl { .. })),
                "{raw}"
            );
        }
    }
}
