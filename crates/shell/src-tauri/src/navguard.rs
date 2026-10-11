//! Navigation safety interstitial.
//!
//! Complements "Peek": where Peek previews an Opt+clicked link, this guards an
//! *actual* navigation. A narrow `WKScriptMessageHandler` (`wisNavGuard`) +
//! injected script intercepts a plain (non-Option) left-click on a risky link
//! **before it navigates** (`preventDefault`), so the page is never loaded in
//! the background. The href is re-assessed authoritatively by
//! [`wis_core::assess_link`]; if risky and the host hasn't been waved through
//! this session, a full-screen interstitial is raised. The user either
//! dismisses it (staying on the current page) or proceeds, which navigates and
//! remembers the host for the session. A cancelled click that turns out safe,
//! or whose host was already waved through, is carried out natively: in a new
//! tab for a link that targets one, else in the tab that was clicked.
//!
//! The injected script does a *coarse* risk check (to avoid intercepting the
//! vast majority of safe clicks); Rust produces the authoritative assessment.

use std::sync::OnceLock;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use url::Url;
use wis_core::{LinkAssessment, RiskLevel, TabId, WisError, WisResult};

use crate::state::AppState;

const NAV_GUARD_EVENT: &str = "wis-nav-guard";

/// App handle for the native message handler to reach the async runtime.
static APP: OnceLock<AppHandle> = OnceLock::new();

/// Record the app handle once during setup so the `wisNavGuard` handler can emit.
pub fn set_app_handle(app: AppHandle) {
    let _ = APP.set(app);
}

/// Drive a nav-guard request from a parsed `{ href, text }` payload (the Windows
/// dispatcher's entry; macOS reaches `intake` through its `wisNavGuard`
/// script-message handler).
#[cfg(any(windows, target_os = "linux"))]
pub(crate) fn dispatch_message(label: &str, href: String, text: String, new_tab: bool) {
    if href.is_empty() {
        return;
    }
    let Some(tab) = tab_of(label) else {
        return;
    };

    if let Some(app) = APP.get() {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            intake(app, tab, href, text, new_tab).await;
        });
    }
}

/// The tab behind webview `label` (`tab-<uuid>`).
fn tab_of(label: &str) -> Option<TabId> {
    label
        .strip_prefix("tab-")
        .and_then(|id| uuid::Uuid::parse_str(id).ok())
        .map(TabId::from_uuid)
}

#[derive(Debug, Serialize, Clone)]
struct NavGuardEvent {
    url: String,
    host: String,
    assessment: LinkAssessment,
    /// True when the page already started navigating (page-load fallback path):
    /// "Take me back" must then go back rather than just dismiss.
    navigated: bool,
    /// The tab that triggered the guard, when known (page-load path).
    tab_id: Option<TabId>,
}

/// Whether an assessment is risky enough to interrupt a real navigation.
///
/// High always interrupts. Medium interrupts only when it has a reason beyond
/// plain HTTP.
fn should_guard(a: &LinkAssessment) -> bool {
    match a.risk {
        RiskLevel::High => true,
        RiskLevel::Medium => a.warnings.iter().any(|w| w.code != "insecure_http"),
        RiskLevel::Low => false,
    }
}

/// Assess a click cancelled in `tab` and, if risky and not bypassed, raise the
/// interstitial; otherwise carry the click out, since nothing has loaded.
async fn intake(app: AppHandle, tab: TabId, href: String, text: String, new_tab: bool) {
    let Ok(url) = Url::parse(&href) else {
        return;
    };
    if !matches!(url.scheme(), "http" | "https") {
        return;
    }
    let Some(host) = url.host_str().map(|h| h.to_string()) else {
        return;
    };

    let anchor = if text.is_empty() {
        None
    } else {
        Some(text.as_str())
    };
    let assessment = wis_core::assess_link(&href, anchor);
    let bypassed = app
        .state::<AppState>()
        .nav_guard_bypass
        .lock()
        .await
        .contains(&host);
    if !should_guard(&assessment) || bypassed {
        if new_tab {
            crate::link_open::open_in_foreground_tab(url.as_str());
        } else if let Err(err) = navigate_tab(&app, tab, url) {
            log::warn!("{err}");
        }

        return;
    }

    let _ = tauri::Emitter::emit(
        &app,
        NAV_GUARD_EVENT,
        NavGuardEvent {
            url: url.to_string(),
            host,
            assessment,
            navigated: false,
            tab_id: Some(tab),
        },
    );
}

/// Fallback path: assess a navigation as the main-frame load *starts*. Catches
/// navigations the click interceptor can't (address bar, JS redirects, or if
/// the click handler was bypassed). Halts the load and raises the interstitial.
pub async fn maybe_guard(app: AppHandle, tab_id: TabId, url: Url) {
    if !matches!(url.scheme(), "http" | "https") {
        return;
    }
    let Some(host) = url.host_str().map(|h| h.to_string()) else {
        return;
    };

    let assessment = wis_core::assess_link(url.as_str(), None);
    if !should_guard(&assessment) {
        return;
    }

    {
        let state = app.state::<AppState>();
        if state.nav_guard_bypass.lock().await.contains(&host) {
            return;
        }
    }

    stop_tab_load(&app, tab_id);
    let _ = tauri::Emitter::emit(
        &app,
        NAV_GUARD_EVENT,
        NavGuardEvent {
            url: url.to_string(),
            host,
            assessment,
            navigated: true,
            tab_id: Some(tab_id),
        },
    );
}

/// Halt an in-flight load so the page can't keep running before the user
/// decides. Also backs the chrome's stop button (`tab_stop`).
pub(crate) fn stop_tab_load(app: &AppHandle, tab_id: TabId) {
    let label = format!("tab-{}", tab_id.as_uuid());
    let Some(wv) = app.webviews().get(&label).cloned() else {
        return;
    };

    #[cfg(target_os = "macos")]
    {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        let _ = wv.with_webview(|w| unsafe {
            let wk: *mut AnyObject = w.inner() as *mut AnyObject;
            if wk.is_null() {
                return;
            }
            let _: () = msg_send![wk, stopLoading];
        });
    }

    #[cfg(windows)]
    {
        crate::win_webview2::with_core_webview2(&wv, |core| {
            // SAFETY: runs on the UI thread via `with_core_webview2`.
            if let Err(err) = unsafe { core.Stop() } {
                log::warn!("webview2 Stop failed: {err}");
            }
        });
    }

    #[cfg(target_os = "linux")]
    {
        let _ = wv.with_webview(|w| {
            use webkit2gtk::WebViewExt;
            w.inner().stop_loading();
        });
    }

    #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
    {
        let _ = wv;
    }
}

/// User chose to continue: remember the host for the rest of the session, then
/// drive the tab to the URL.
#[tauri::command]
pub async fn nav_guard_proceed(
    app: AppHandle,
    state: State<'_, AppState>,
    tab_id: TabId,
    url: String,
    host: String,
) -> WisResult<()> {
    state.nav_guard_bypass.lock().await.insert(host);

    let parsed = Url::parse(&url).map_err(|_| WisError::InvalidUrl { url: url.clone() })?;
    navigate_tab(&app, tab_id, parsed)
}

/// Load `url` in `tab`, if its webview is still open.
fn navigate_tab(app: &AppHandle, tab: TabId, url: Url) -> WisResult<()> {
    let label = format!("tab-{}", tab.as_uuid());
    if let Some(wv) = app.webviews().get(&label).cloned() {
        wv.navigate(url).map_err(|err| WisError::InvalidInput {
            message: format!("nav guard navigate failed: {err}"),
        })?;
    }

    Ok(())
}

/// The interceptor script with the shared text-TLD set baked in, so the coarse
/// JS gate and `wis_core::assess_link` agree on what anchor text names a site.
fn navguard_js() -> &'static str {
    static JS: OnceLock<String> = OnceLock::new();
    JS.get_or_init(|| {
        let tlds = serde_json::to_string(wis_core::link_safety::TEXT_DOMAIN_TLDS)
            .unwrap_or_else(|_| "[]".to_string());
        NAVGUARD_JS.replace("__WIS_TEXT_TLDS__", &tlds)
    })
}

/// Injected page script: intercept a plain left-click on a risky link before it
/// navigates. Option+click is left for Peek. The JS risk check is coarse; the
/// authoritative assessment is recomputed in Rust.
const NAVGUARD_JS: &str = r#"(function () {
  if (window.__wisNavGuardInstalled) return;
  window.__wisNavGuardInstalled = true;
  var TEXT_TLDS = __WIS_TEXT_TLDS__;
  function hostFromText(t) {
    t = (t || '').trim();
    if (!t || t.length > 200) return '';
    try {
      if (/^[a-z][a-z0-9+.-]*:\/\//i.test(t)) return new URL(t).hostname.toLowerCase().replace(/^www\./, '');
    } catch (e) {}
    if (/\s/.test(t) || t.indexOf('.') < 0) return '';
    try {
      var h = new URL('https://' + t).hostname.toLowerCase().replace(/^www\./, '');
      if (h.indexOf('.') < 0) return '';
      return TEXT_TLDS.indexOf(h.slice(h.lastIndexOf('.') + 1)) >= 0 ? h : '';
    } catch (e) { return ''; }
  }
  function shouldGuard(href, text) {
    var u;
    try { u = new URL(href, location.href); } catch (e) { return false; }
    var scheme = u.protocol.slice(0, -1).toLowerCase();
    if (scheme !== 'http' && scheme !== 'https') return false;
    if (u.username || u.password) return true;
    var host = u.hostname.toLowerCase().replace(/^www\./, '');
    if (!host) return false;
    if (/^\d{1,3}(\.\d{1,3}){3}$/.test(host)) return true;
    var labels = host.split('.').filter(Boolean);
    if (labels.some(function (l) { return l.indexOf('xn--') === 0; })) return true;
    if (labels.length > 5) return true;
    var th = hostFromText(text);
    if (th && th !== host && host.slice(-(th.length + 1)) !== ('.' + th)) return true;
    return false;
  }
  function anchorFrom(t) {
    var el = (t && t.nodeType === 1) ? t : (t && t.parentElement);
    return el && el.closest ? el.closest('a[href]') : null;
  }
  document.addEventListener('click', function (e) {
    if (!e.isTrusted || e.altKey || e.button !== 0) return; // Option+click is Peek
    var a = anchorFrom(e.target);
    if (!a) return;
    var href = a.href || '';
    if (!href) return;
    try {
      var u = new URL(href, location.href);
      if (u.hash && u.origin === location.origin && u.pathname === location.pathname && u.search === location.search) return;
    } catch (err) {}
    var text = (a.textContent || '').replace(/\s+/g, ' ').trim().slice(0, 200);
    if (!shouldGuard(href, text)) return;
    e.preventDefault();
    e.stopPropagation();
    var target = a.getAttribute('target') || '';
    var newTab = target !== '' && !/^_(self|top|parent)$/i.test(target);
    try {
      if (window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.wisNavGuard) {
        window.webkit.messageHandlers.wisNavGuard.postMessage({ href: href, text: text, newTab: newTab });
      }
    } catch (err) {}
  }, true);
})();"#;

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;

    use objc2::rc::Id;
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{
        class, declare_class, msg_send, msg_send_id, mutability, ClassType, DeclaredClass,
    };

    use super::{intake, navguard_js, tab_of, APP};
    use crate::content_world::macos as content_world;

    struct HandlerIvars {
        label: String,
    }

    declare_class!(
        struct NavGuardHandler;

        unsafe impl ClassType for NavGuardHandler {
            type Super = NSObject;
            type Mutability = mutability::InteriorMutable;
            const NAME: &'static str = "WisNavGuardHandler";
        }

        impl DeclaredClass for NavGuardHandler {
            type Ivars = HandlerIvars;
        }

        unsafe impl NavGuardHandler {
            #[method(userContentController:didReceiveScriptMessage:)]
            fn did_receive(&self, _ucc: *mut AnyObject, message: *mut AnyObject) {
                let label = self.ivars().label.clone();
                unsafe { handle_message(&label, message) }
            }
        }
    );

    impl NavGuardHandler {
        fn new(label: String) -> Id<Self> {
            let this = Self::alloc().set_ivars(HandlerIvars { label });
            unsafe { msg_send_id![super(this), init] }
        }
    }

    unsafe fn ns_string(s: &str) -> *mut AnyObject {
        let c = CString::new(s).unwrap_or_default();
        msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()]
    }

    unsafe fn nsstring_to_string(ns: *mut AnyObject) -> Option<String> {
        if ns.is_null() {
            return None;
        }

        let is_str: bool = msg_send![ns, isKindOfClass: class!(NSString)];
        if !is_str {
            return None;
        }

        let utf8: *const c_char = msg_send![ns, UTF8String];
        if utf8.is_null() {
            return None;
        }

        Some(CStr::from_ptr(utf8).to_string_lossy().into_owned())
    }

    unsafe fn dict_string(dict: *mut AnyObject, key: &str) -> Option<String> {
        let k = ns_string(key);
        let v: *mut AnyObject = msg_send![dict, objectForKey: k];
        nsstring_to_string(v)
    }

    unsafe fn dict_bool(dict: *mut AnyObject, key: &str) -> bool {
        let v: *mut AnyObject = msg_send![dict, objectForKey: ns_string(key)];
        if v.is_null() {
            return false;
        }

        let is_number: bool = msg_send![v, isKindOfClass: class!(NSNumber)];
        is_number && msg_send![v, boolValue]
    }

    unsafe fn handle_message(label: &str, message: *mut AnyObject) {
        if message.is_null() {
            return;
        }
        let Some(tab) = tab_of(label) else {
            return;
        };

        let body: *mut AnyObject = msg_send![message, body];
        if body.is_null() {
            return;
        }

        let is_dict: bool = msg_send![body, isKindOfClass: class!(NSDictionary)];
        if !is_dict {
            return;
        }

        let Some(href) = dict_string(body, "href") else {
            return;
        };
        if href.is_empty() {
            return;
        }
        let text = dict_string(body, "text").unwrap_or_default();
        let new_tab = dict_bool(body, "newTab");

        if let Some(app) = APP.get() {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                intake(app, tab, href, text, new_tab).await;
            });
        }
    }

    /// Install the navigation-guard user script + message handler on a tab webview.
    pub fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
        let label = webview.label().to_string();
        let _ = webview.with_webview(move |w| unsafe {
            let wk: *mut AnyObject = w.inner() as *mut AnyObject;
            if wk.is_null() {
                return;
            }

            let config: *mut AnyObject = msg_send![wk, configuration];
            if config.is_null() {
                return;
            }

            let controller: *mut AnyObject = msg_send![config, userContentController];
            if controller.is_null() {
                return;
            }

            let handler = NavGuardHandler::new(label.clone());
            content_world::add_handler(controller, &*handler, "wisNavGuard");
            content_world::install_script(
                wk,
                controller,
                &label,
                "wisNavGuard",
                navguard_js(),
                true,
            );
        });
    }
}

/// Install the navigation-guard channel on a tab webview. No-op off macOS.
pub fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
    #[cfg(target_os = "macos")]
    imp::install(webview);
    #[cfg(not(target_os = "macos"))]
    let _ = webview;
}

/// The click-interceptor script, exposed so the Windows tab builder can inject
/// it as an init script (macOS injects it via `imp::install`); it posts risky
/// links to `wisNavGuard`, routed by `win_dispatch` to [`dispatch_message`].
#[cfg(any(windows, target_os = "linux"))]
pub fn page_script() -> &'static str {
    navguard_js()
}
