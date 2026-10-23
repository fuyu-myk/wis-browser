//! What was blocked on each tab's page, for the site panel (macOS). WebKit
//! reports the loads Wis's rule lists block; a load the workspace proxy
//! refused shows up as a failed load whose host the filter engine blocks or
//! whose aliases it uncloaked.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};

use serde::Serialize;
use wis_core::TabId;

const MAX_HOSTS: usize = 64;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct PageBlocks {
    pub total: u32,
    /// Most blocked first.
    pub hosts: Vec<(String, u32)>,
}

#[derive(Default)]
struct Tally {
    total: u32,
    hosts: HashMap<String, u32>,
}

impl Tally {
    fn record(&mut self, host: &str) {
        self.total = self.total.saturating_add(1);
        if let Some(count) = self.hosts.get_mut(host) {
            *count = count.saturating_add(1);
        } else if self.hosts.len() < MAX_HOSTS {
            self.hosts.insert(host.to_owned(), 1);
        }
    }

    fn summary(&self) -> PageBlocks {
        let mut hosts: Vec<(String, u32)> = self
            .hosts
            .iter()
            .map(|(host, count)| (host.clone(), *count))
            .collect();
        hosts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        PageBlocks {
            total: self.total,
            hosts,
        }
    }
}

static TALLIES: LazyLock<Mutex<HashMap<String, Tally>>> = LazyLock::new(Default::default);

fn tallies() -> MutexGuard<'static, HashMap<String, Tally>> {
    TALLIES.lock().unwrap_or_else(PoisonError::into_inner)
}

fn host_of(url: &str) -> Option<String> {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
}

pub fn record(label: &str, url: &str) {
    if let Some(host) = host_of(url) {
        record_host(label, &host);
    }
}

fn record_host(label: &str, host: &str) {
    if !label.starts_with("tab-") {
        return;
    }
    let mut tallies = tallies();
    match tallies.get_mut(label) {
        Some(tally) => tally.record(host),
        None => tallies.entry(label.to_owned()).or_default().record(host),
    }
}

pub fn forget(label: &str) {
    tallies().remove(label);
}

#[tauri::command]
pub fn tab_page_blocks(id: TabId) -> PageBlocks {
    tallies()
        .get(&format!("tab-{}", id.as_uuid()))
        .map(Tally::summary)
        .unwrap_or_default()
}

#[cfg(target_os = "macos")]
static APP: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

#[cfg(target_os = "macos")]
pub fn install(app: tauri::AppHandle) {
    let _ = APP.set(app);
    wry::set_content_rule_block_hook(Box::new(|webview, identifier, url| {
        if !identifier.starts_with(crate::protection::content_filter::RULE_LIST_PREFIX) {
            return;
        }
        // SAFETY: wry calls the hook on the main thread with a live WKWebView.
        if let Some(label) = unsafe { crate::platform::native_webview::label(webview) } {
            record(&label, url);
        }
    }));
}

/// # Safety
///
/// Main thread only; `webview` is a live `WKWebView`.
#[cfg(target_os = "macos")]
pub unsafe fn watch_failed_loads(webview: *mut std::ffi::c_void) {
    if let Some(webview) = webview.cast::<objc2_06::runtime::AnyObject>().as_ref() {
        imp::watch(webview);
    }
}

/// # Safety
///
/// Main thread only; `webview` is a live `WKWebView`.
#[cfg(target_os = "macos")]
pub unsafe fn stop_watching(webview: *mut std::ffi::c_void) {
    if let Some(webview) = webview.cast::<objc2_06::runtime::AnyObject>().as_ref() {
        imp::stop_watching(webview);
    }
}

#[cfg(target_os = "macos")]
fn record_failed_load(label: &str, url: &str) {
    use tauri::Manager;

    let (Some(app), Some(host)) = (APP.get(), host_of(url)) else {
        return;
    };
    let state = app.state::<crate::state::AppState>();
    let blocker = state
        .tracker_blocker
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    if blocker.blocks_tunnel(&host)
        || crate::protection::uncloak::was_uncloaked(&state, label, &host)
    {
        record_host(label, &host);
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::cell::OnceCell;

    use objc2_06::rc::Retained;
    use objc2_06::runtime::{AnyObject, NSObject, NSObjectProtocol};
    use objc2_06::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
    use objc2_foundation::{NSError, NSURL};

    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "WisPageBlocksLoadDelegate"]
        struct LoadDelegate;

        unsafe impl NSObjectProtocol for LoadDelegate {}

        impl LoadDelegate {
            #[unsafe(method(webView:resourceLoad:didCompleteWithError:response:))]
            fn did_complete(
                &self,
                webview: &AnyObject,
                load: &AnyObject,
                error: Option<&NSError>,
                _response: Option<&AnyObject>,
            ) {
                if error.is_none() {
                    return;
                }
                // SAFETY: WebKit passes the live webview and load info on the
                // main thread.
                let (label, url) = unsafe {
                    let url: Option<Retained<NSURL>> = msg_send![load, originalURL];
                    let webview: *const AnyObject = webview;
                    (
                        crate::platform::native_webview::label(webview.cast()),
                        url.and_then(|u| u.absoluteString()),
                    )
                };
                if let (Some(label), Some(url)) = (label, url) {
                    super::record_failed_load(&label, &url.to_string());
                }
            }
        }
    );

    thread_local! {
        static DELEGATE: OnceCell<Retained<LoadDelegate>> = const { OnceCell::new() };
    }

    /// `_resourceLoadDelegate` is weak, so one delegate lives for the app.
    pub fn watch(webview: &AnyObject) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        // SAFETY: on the main thread with a live WKWebView; the SPI setter is
        // checked before it is sent.
        unsafe {
            let supported: bool =
                msg_send![webview, respondsToSelector: sel!(_setResourceLoadDelegate:)];
            if !supported {
                return;
            }
            DELEGATE.with(|cell| {
                let delegate = cell.get_or_init(|| {
                    let this = LoadDelegate::alloc(mtm).set_ivars(());
                    msg_send![super(this), init]
                });
                let _: () = msg_send![webview, _setResourceLoadDelegate: &**delegate];
            });
        }
    }

    pub fn stop_watching(webview: &AnyObject) {
        // SAFETY: on the main thread with a live WKWebView; the SPI setter is
        // checked before it is sent.
        unsafe {
            let supported: bool =
                msg_send![webview, respondsToSelector: sel!(_setResourceLoadDelegate:)];
            if supported {
                let none: *mut AnyObject = std::ptr::null_mut();
                let _: () = msg_send![webview, _setResourceLoadDelegate: none];
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use objc2_06::runtime::{AnyObject, Bool};
        use objc2_06::{class, sel, ClassType};
        use objc2_foundation::NSError;

        #[test]
        fn the_delegate_registers_webkit_s_completion_method() {
            super::LoadDelegate::class()
                .verify_sel::<(&AnyObject, &AnyObject, Option<&NSError>, Option<&AnyObject>), ()>(
                    sel!(webView:resourceLoad:didCompleteWithError:response:),
                )
                .unwrap();
        }

        #[test]
        fn every_send_matches_its_method() {
            class!(_WKContentRuleListAction)
                .verify_sel::<(), Bool>(sel!(blockedLoad))
                .unwrap();
            class!(WKWebView)
                .verify_sel::<(*mut AnyObject,), ()>(sel!(_setResourceLoadDelegate:))
                .unwrap();
            class!(_WKResourceLoadInfo)
                .verify_sel::<(), *mut AnyObject>(sel!(originalURL))
                .unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_s_blocks_are_counted_by_host_most_first() {
        let mut tally = Tally::default();
        for host in ["b.test", "a.test", "b.test", "c.test", "a.test", "b.test"] {
            tally.record(host);
        }
        assert_eq!(
            tally.summary(),
            PageBlocks {
                total: 6,
                hosts: vec![
                    ("b.test".into(), 3),
                    ("a.test".into(), 2),
                    ("c.test".into(), 1)
                ],
            }
        );
    }

    #[test]
    fn hosts_past_the_cap_still_count_toward_the_total() {
        let mut tally = Tally::default();
        for n in 0..MAX_HOSTS + 5 {
            tally.record(&format!("h{n}.test"));
        }
        tally.record("h0.test");
        let summary = tally.summary();
        assert_eq!(summary.total as usize, MAX_HOSTS + 6);
        assert_eq!(summary.hosts.len(), MAX_HOSTS);
        assert_eq!(summary.hosts[0], ("h0.test".into(), 2));
    }

    #[test]
    fn only_tabs_are_counted_and_a_new_page_starts_from_nothing() {
        let tab = "tab-00000000-0000-4000-8000-0000000000aa";
        record(tab, "https://ads.test/a.js");
        record(tab, "not a url");
        record("peek-page-1", "https://ads.test/a.js");
        assert_eq!(tallies().get(tab).map(|t| t.total), Some(1));
        assert!(!tallies().contains_key("peek-page-1"));

        forget(tab);
        assert!(!tallies().contains_key(tab));
    }
}
