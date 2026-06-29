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
//! remembers the host for the session.
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
#[cfg(windows)]
pub(crate) fn dispatch_message(href: String, text: String) {
    if href.is_empty() {
        return;
    }

    if let Some(app) = APP.get() {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            intake(app, href, text).await;
        });
    }
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

/// Assess an intercepted link click and, if risky and not bypassed, raise the
/// interstitial. The click was already cancelled in the page, so nothing has
/// loaded.
async fn intake(app: AppHandle, href: String, text: String) {
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
    if !should_guard(&assessment) {
        return;
    }

    {
        let state = app.state::<AppState>();
        if state.nav_guard_bypass.lock().await.contains(&host) {
            return;
        }
    }

    let _ = tauri::Emitter::emit(
        &app,
        NAV_GUARD_EVENT,
        NavGuardEvent {
            url: url.to_string(),
            host,
            assessment,
            navigated: false,
            tab_id: None,
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

/// Halt an in-flight load so the page can't keep running before the user decides.
fn stop_tab_load(app: &AppHandle, tab_id: TabId) {
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
    #[cfg(not(target_os = "macos"))]
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
    let label = format!("tab-{}", tab_id.as_uuid());
    if let Some(wv) = app.webviews().get(&label).cloned() {
        wv.navigate(parsed).map_err(|err| WisError::InvalidInput {
            message: format!("nav guard proceed navigate failed: {err}"),
        })?;
    }

    Ok(())
}

/// Injected page script: intercept a plain left-click on a risky link before it
/// navigates. Option+click is left for Peek. The JS risk check is coarse; the
/// authoritative assessment is recomputed in Rust.
const NAVGUARD_JS: &str = r#"(function () {
  if (window.__wisNavGuardInstalled) return;
  window.__wisNavGuardInstalled = true;
  function hostFromText(t) {
    t = (t || '').trim();
    if (!t || t.length > 200) return '';
    try {
      if (/^[a-z][a-z0-9+.-]*:\/\//i.test(t)) return new URL(t).hostname.toLowerCase().replace(/^www\./, '');
    } catch (e) {}
    if (/\s/.test(t) || t.indexOf('.') < 0) return '';
    try {
      var h = new URL('https://' + t).hostname.toLowerCase().replace(/^www\./, '');
      return h.indexOf('.') >= 0 ? h : '';
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
    if (e.altKey || e.button !== 0) return; // Option+click is Peek
    var a = anchorFrom(e.target);
    if (!a) return;
    var href = a.href || '';
    if (!href) return;
    var text = (a.textContent || '').replace(/\s+/g, ' ').trim().slice(0, 200);
    if (!shouldGuard(href, text)) return;
    e.preventDefault();
    e.stopPropagation();
    try {
      if (window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.wisNavGuard) {
        window.webkit.messageHandlers.wisNavGuard.postMessage({ href: href, text: text });
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

    use super::{intake, APP, NAVGUARD_JS};

    declare_class!(
        struct NavGuardHandler;

        unsafe impl ClassType for NavGuardHandler {
            type Super = NSObject;
            type Mutability = mutability::InteriorMutable;
            const NAME: &'static str = "WisNavGuardHandler";
        }

        impl DeclaredClass for NavGuardHandler {
            type Ivars = ();
        }

        unsafe impl NavGuardHandler {
            #[method(userContentController:didReceiveScriptMessage:)]
            fn did_receive(&self, _ucc: *mut AnyObject, message: *mut AnyObject) {
                unsafe { handle_message(message) }
            }
        }
    );

    impl NavGuardHandler {
        fn new() -> Id<Self> {
            unsafe { msg_send_id![Self::alloc(), init] }
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

    unsafe fn handle_message(message: *mut AnyObject) {
        if message.is_null() {
            return;
        }

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

        if let Some(app) = APP.get() {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                intake(app, href, text).await;
            });
        }
    }

    /// Install the navigation-guard user script + message handler on a tab webview.
    pub fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
        let _ = webview.with_webview(|w| unsafe {
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

            let src = ns_string(NAVGUARD_JS);
            let script: *mut AnyObject = msg_send![class!(WKUserScript), alloc];
            let injection_time: isize = 1; // WKUserScriptInjectionTimeAtDocumentEnd
            let script: *mut AnyObject = msg_send![
                script,
                initWithSource: src,
                injectionTime: injection_time,
                forMainFrameOnly: false,
            ];
            let _: () = msg_send![controller, addUserScript: script];
            let _: () = msg_send![script, release];

            let handler = NavGuardHandler::new();
            let name = ns_string("wisNavGuard");
            let _: () = msg_send![controller, addScriptMessageHandler: &*handler, name: name];
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
