//! Links a page hands to the chrome.
//!
//! Tab webviews bake a page script that intercepts `Cmd`/`Ctrl`+click and
//! middle-click on `<a href>` (Opt+click stays peek's), `preventDefault`s
//! WebKit's open-in-new-window, and posts the href to the `wisOpenInTab`
//! message handler, which re-emits it as a **background** tab. Widget panels
//! bake that script plus [`panel_page_script`]: a plain click on a link to
//! another site posts to `wisOpenInTabFg` and opens a **foreground** tab,
//! while same-site clicks stay in the panel.
//!
//! Engine new-window requests
//! (`window.open` / `target=_blank`) flow through `on_new_window` instead,
//! which routes plain navigations here as [`OPEN_TAB_FOREGROUND_EVENT`] and
//! keeps geometry-carrying popups native (see `create_tab_webview`).

use std::sync::OnceLock;

use tauri::{AppHandle, Emitter};

/// Event emitted to the chrome with the link to open in a background tab.
const OPEN_IN_TAB_EVENT: &str = "wis-open-in-tab";

/// Event emitted to the chrome with a link to open as a **foreground** tab.
pub const OPEN_TAB_FOREGROUND_EVENT: &str = "wis-open-in-tab-fg";

/// Background script-message handler (Cmd/middle-click).
pub(crate) const HANDLER_BACKGROUND: &str = "wisOpenInTab";

/// Foreground script-message handler (a panel's plain click on a
/// cross-site link).
pub(crate) const HANDLER_FOREGROUND: &str = "wisOpenInTabFg";

/// App handle so the native message handler can reach the event bus.
static APP: OnceLock<AppHandle> = OnceLock::new();

/// Record the app handle once during setup so the message handlers can emit.
pub fn set_app_handle(app: AppHandle) {
    let _ = APP.set(app);
}

fn event_for_handler(handler: &str) -> &'static str {
    if handler == HANDLER_FOREGROUND {
        OPEN_TAB_FOREGROUND_EVENT
    } else {
        OPEN_IN_TAB_EVENT
    }
}

fn emit(handler: &str, href: &str) {
    if href.is_empty() {
        return;
    }

    if let Some(app) = APP.get() {
        let _ = app.emit(event_for_handler(handler), href);
    }
}

/// Emit the open-in-tab event for a parsed href posted to `handler` (the
/// Windows/Linux dispatchers' entry; macOS reaches the same emit through its
/// script-message handlers).
#[cfg(any(windows, target_os = "linux"))]
pub(crate) fn dispatch_message(handler: &str, href: &str) {
    emit(handler, href);
}

/// Injected page script (tab webviews): `Cmd`/`Ctrl`+click or middle-click a
/// link to open it in a background tab instead of navigating / popping a window.
const OPEN_IN_TAB_JS: &str = r#"(function () {
  if (window.__wisOpenInTab) return;
  window.__wisOpenInTab = true;
  function hrefAt(e) {
    var t = e.target;
    var el = (t && t.nodeType === 1) ? t : (t && t.parentElement);
    var a = el && el.closest ? el.closest('a[href]') : null;
    if (!a) return null;
    var href = a.href || '';
    if (!/^https?:/i.test(href)) return null;
    return href;
  }
  function post(href) {
    try {
      if (window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.wisOpenInTab) {
        window.webkit.messageHandlers.wisOpenInTab.postMessage(href);
      }
    } catch (err) {}
  }
  document.addEventListener('click', function (e) {
    if (!e.isTrusted || e.button !== 0 || e.altKey || !(e.metaKey || e.ctrlKey)) return;
    var href = hrefAt(e);
    if (!href) return;
    e.preventDefault();
    e.stopPropagation();
    post(href);
  }, true);
  document.addEventListener('auxclick', function (e) {
    if (!e.isTrusted || e.button !== 1) return;
    var href = hrefAt(e);
    if (!href) return;
    e.preventDefault();
    e.stopPropagation();
    post(href);
  }, true);
  // Middle-button mousedown autoscroll / pre-empts the default new-window.
  document.addEventListener('mousedown', function (e) {
    if (e.button === 1 && hrefAt(e)) e.preventDefault();
  }, true);
})();"#;

/// The trigger, baked into tab webviews as a builder-level initialization
/// script off macOS, where it runs in the page's world.
#[cfg(not(target_os = "macos"))]
pub fn page_script() -> &'static str {
    OPEN_IN_TAB_JS
}

/// Injected page script (widget panels only): a plain left click on a link to
/// another registrable site opens it as a foreground tab; same-site links stay
/// in the panel. `registrable` mirrors `popup_chrome::registrable`.
const PANEL_CROSS_SITE_JS: &str = r#"(function () {
  if (window.__wisPanelLinks) return;
  window.__wisPanelLinks = true;
  var CC_SECOND = ['co', 'com', 'org', 'net', 'ac', 'gov', 'edu', 'ne', 'or'];
  function registrable(host) {
    host = String(host || '').replace(/\.+$/, '').toLowerCase();
    if (!host) return '';
    if (/^[\d.]+$/.test(host) || host.charAt(0) === '[') return host;
    var labels = host.split('.').filter(Boolean);
    if (labels.length <= 2) return labels.join('.');
    var tld = labels[labels.length - 1];
    var second = labels[labels.length - 2];
    var take = tld.length === 2 && CC_SECOND.indexOf(second) >= 0 ? 3 : 2;
    return labels.slice(labels.length - take).join('.');
  }
  function crossSiteHref(e) {
    var t = e.target;
    var el = (t && t.nodeType === 1) ? t : (t && t.parentElement);
    var a = el && el.closest ? el.closest('a[href]') : null;
    if (!a || a.hasAttribute('download')) return null;
    var href = a.href || '';
    if (!/^https?:/i.test(href)) return null;
    var here = registrable(location.hostname);
    if (!here) return null;
    var host;
    try { host = new URL(href).hostname; } catch (err) { return null; }
    return registrable(host) === here ? null : href;
  }
  document.addEventListener('click', function (e) {
    if (e.button !== 0 || e.metaKey || e.ctrlKey || e.altKey || e.shiftKey) return;
    var active = navigator.userActivation && navigator.userActivation.isActive;
    if (!e.isTrusted && !active) return;
    var href = crossSiteHref(e);
    if (!href) return;
    e.preventDefault();
    e.stopPropagation();
    try {
      if (window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.wisOpenInTabFg) {
        window.webkit.messageHandlers.wisOpenInTabFg.postMessage(href);
      }
    } catch (err) {}
  }, true);
})();"#;

/// The panel trigger, baked into widget panel webviews alongside
/// [`page_script`] off macOS.
#[cfg(not(target_os = "macos"))]
pub fn panel_page_script() -> &'static str {
    PANEL_CROSS_SITE_JS
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::CStr;
    use std::os::raw::c_char;

    use super::{
        emit, HANDLER_BACKGROUND, HANDLER_FOREGROUND, OPEN_IN_TAB_JS, PANEL_CROSS_SITE_JS,
    };
    use crate::content_world::macos as content_world;
    use objc2::rc::Id;
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{
        class, declare_class, msg_send, msg_send_id, mutability, ClassType, DeclaredClass,
    };

    declare_class!(
        struct OpenInTabHandler;

        unsafe impl ClassType for OpenInTabHandler {
            type Super = NSObject;
            type Mutability = mutability::InteriorMutable;
            const NAME: &'static str = "WisOpenInTabHandler";
        }

        impl DeclaredClass for OpenInTabHandler {
            type Ivars = ();
        }

        unsafe impl OpenInTabHandler {
            #[method(userContentController:didReceiveScriptMessage:)]
            fn did_receive(&self, _ucc: *mut AnyObject, message: *mut AnyObject) {
                unsafe { handle_message(message) }
            }
        }
    );

    impl OpenInTabHandler {
        fn new() -> Id<Self> {
            unsafe { msg_send_id![Self::alloc(), init] }
        }
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

    unsafe fn handle_message(message: *mut AnyObject) {
        if message.is_null() {
            return;
        }

        let name: *mut AnyObject = msg_send![message, name];
        let handler = nsstring_to_string(name).unwrap_or_default();
        let body: *mut AnyObject = msg_send![message, body];
        let Some(href) = nsstring_to_string(body) else {
            return;
        };

        emit(&handler, &href);
    }

    /// Register one [`OpenInTabHandler`] under each of `names` on a webview,
    /// with `scripts` beside them.
    fn install_names<R: tauri::Runtime>(
        webview: &tauri::webview::Webview<R>,
        names: &'static [&'static str],
        scripts: &'static [(&'static str, &'static str)],
    ) {
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

            let handler = OpenInTabHandler::new();
            for name in names {
                content_world::add_handler(controller, &*handler, name);
            }
            for (key, source) in scripts {
                content_world::install_script(wk, controller, key, source, true);
            }
        });
    }

    pub fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
        install_names(
            webview,
            &[HANDLER_BACKGROUND],
            &[(HANDLER_BACKGROUND, OPEN_IN_TAB_JS)],
        );
    }

    pub fn install_panel<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
        install_names(
            webview,
            &[HANDLER_BACKGROUND, HANDLER_FOREGROUND],
            &[
                (HANDLER_BACKGROUND, OPEN_IN_TAB_JS),
                (HANDLER_FOREGROUND, PANEL_CROSS_SITE_JS),
            ],
        );
    }
}

/// Install the Cmd/middle-click open-in-tab channel on a tab webview. No-op off macOS.
pub fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
    #[cfg(target_os = "macos")]
    imp::install(webview);

    #[cfg(not(target_os = "macos"))]
    let _ = webview;
}

/// Install both open-in-tab channels on a widget panel webview. No-op off macOS.
pub fn install_panel<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
    #[cfg(target_os = "macos")]
    imp::install_panel(webview);

    #[cfg(not(target_os = "macos"))]
    let _ = webview;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handler_name_selects_event() {
        assert_eq!(
            event_for_handler(HANDLER_FOREGROUND),
            OPEN_TAB_FOREGROUND_EVENT
        );
        assert_eq!(event_for_handler(HANDLER_BACKGROUND), OPEN_IN_TAB_EVENT);
        assert_eq!(event_for_handler("unknown"), OPEN_IN_TAB_EVENT);
    }

    #[test]
    fn panel_script_posts_to_the_foreground_channel_only() {
        assert!(PANEL_CROSS_SITE_JS.contains("messageHandlers.wisOpenInTabFg.postMessage"));
        assert!(!PANEL_CROSS_SITE_JS.contains("wisOpenInTab.postMessage"));
        assert!(!OPEN_IN_TAB_JS.contains("wisOpenInTabFg"));
    }
}
