//! "Peek": an Opt+click link preview shown in a small, isolated webview.
//!
//! Safety spine:
//! - The preview page (`peek-page`) is created **incognito** (a non-persistent
//!   data store), so a link-based scam cannot read or write the active
//!   workspace's cookies/storage. It still routes through the workspace proxy,
//!   so DoH / HTTPS-First / tracker blocking apply.
//! - The page gets **no Tauri capability** (same as tab webviews), so it can
//!   never call a command.
//! - The trigger is a *narrow* `WKScriptMessageHandler` (`wisPeek`) installed on
//!   tab webviews that only forwards `{href, text}` of an Opt+clicked link to
//!   Rust, which re-emits it to the chrome. The link is assessed by
//!   [`wis_core::assess_link`] and the result drives a warning in the UI.
//! - Reader mode transforms the preview *in place inside the isolated page*
//!   (never rendering untrusted HTML in the privileged chrome).

use std::sync::OnceLock;

use serde::Serialize;
use tauri::webview::WebviewBuilder;
use tauri::{AppHandle, Manager, State, WebviewUrl};
use url::Url;
use wis_core::{LinkAssessment, WisError, WisResult};

use crate::state::AppState;

/// Base label of the isolated preview webview.
const PEEK_LABEL: &str = "peek-page";

/// Per-window preview label: `peek-page-<window>`. Scoping by window keeps each
/// OS window's preview independent (one workspace per window).
fn peek_label(window_label: &str) -> String {
    format!("{PEEK_LABEL}-{window_label}")
}

/// Label of this window's live preview webview, when one exists.
pub fn live_peek_label(window: &tauri::Window) -> Option<String> {
    let label = peek_label(window.label());
    window
        .app_handle()
        .webviews()
        .contains_key(&label)
        .then_some(label)
}

/// Event emitted to the chrome when a link is Opt+clicked.
const PEEK_OPEN_EVENT: &str = "wis-peek-open";

/// Event carrying the preview's history availability and current URL to the
/// peek surface (per navigation and after shell-driven history steps).
const PEEK_NAV_EVENT: &str = "wis-peek-nav-state";

#[derive(Debug, Serialize, Clone)]
struct PeekNavEvent {
    /// OS-window label the preview belongs to; the surface filters on its own.
    window: String,
    back: bool,
    fwd: bool,
    url: Option<String>,
    /// Fresh assessment of `url`, so the warning banner and the high-risk
    /// "Open as tab" gate follow in-peek navigation.
    assessment: Option<LinkAssessment>,
}

fn emit_nav_state(app: &AppHandle, window_label: &str, webview: &tauri::webview::Webview) {
    if let Some((back, fwd)) = crate::swipe_nav::push_nav_state(webview) {
        let url = webview.url().ok().map(|u| u.to_string());
        let assessment = url.as_deref().map(|u| wis_core::assess_link(u, None));
        let event = PeekNavEvent {
            window: window_label.to_string(),
            back,
            fwd,
            url,
            assessment,
        };

        if let Err(err) = tauri::Emitter::emit(app, PEEK_NAV_EVENT, event) {
            log::warn!("emit {PEEK_NAV_EVENT} failed: {err}");
        }
    }
}

/// App handle for the native message handler to reach the async runtime.
static APP: OnceLock<AppHandle> = OnceLock::new();

/// Record the app handle once during setup so the `wisPeek` handler can emit.
pub fn set_app_handle(app: AppHandle) {
    let _ = APP.set(app);
}

/// Emit the peek-open event from a parsed `{ href, text, x, y }` payload (the
/// Windows dispatcher's entry; macOS reaches the same emit through its `wisPeek`
/// script-message handler).
#[cfg(any(windows, target_os = "linux"))]
pub(crate) fn dispatch_message(href: String, text: String, x: f64, y: f64) {
    use tauri::Emitter;

    if href.is_empty() {
        return;
    }

    if let Some(app) = APP.get() {
        let _ = app.emit(PEEK_OPEN_EVENT, PeekOpenEvent { href, text, x, y });
    }
}

#[derive(Debug, Serialize, Clone)]
struct PeekOpenEvent {
    href: String,
    text: String,
    /// Click point in the source page's viewport coords (-1 when unknown);
    /// the chrome translates it for the grow-from-link origin.
    x: f64,
    y: f64,
}

/// Create (or replace) the isolated preview webview pointed at `url`, positioned
/// at the given rect, and return the link's safety assessment.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn open_peek(
    window: tauri::Window,
    state: State<'_, AppState>,
    url: String,
    anchor_text: Option<String>,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> WisResult<LinkAssessment> {
    let assessment = wis_core::assess_link(&url, anchor_text.as_deref());

    // A malformed URL is already flagged High; there's nothing to load.
    let parsed = match Url::parse(&url) {
        Ok(u) => u,
        Err(_) => return Ok(assessment),
    };
    // Only ever preview real web content in the isolated view.
    if !matches!(parsed.scheme(), "http" | "https") {
        return Ok(assessment);
    }

    let app = window.app_handle();
    let label = peek_label(window.label());

    // Replace any existing preview in this window.
    if let Some(existing) = app.webviews().get(&label).cloned() {
        crate::webview_close::terminate_page(&existing);
        let _ = existing.close();
    }

    let (active_ws, ephemeral, user_agent, fraud_warning, media_extensions, global_privacy_control) = {
        let browser = state.browser.lock().await;
        // Preview through the calling window's workspace (one per window), so a
        // peek inherits that window's proxy / blocking / UA.
        let active_ws = state
            .window_workspace(window.label())
            .unwrap_or(browser.active_workspace_id);
        let ephemeral = browser
            .workspace(active_ws)
            .map(|w| w.ephemeral)
            .unwrap_or(false);
        let user_agent = wis_core::ua_for_host(
            parsed.host_str().unwrap_or_default(),
            &browser.settings.ua_site_overrides,
            &browser.settings.user_agent_profile,
            wis_core::UaPlatform::host(),
        );
        (
            active_ws,
            ephemeral,
            user_agent,
            browser.settings.fraud_warning_enabled,
            browser.settings.media_extensions_enabled,
            browser.settings.global_privacy_control,
        )
    };
    #[cfg(not(windows))]
    let _ = ephemeral;
    let proxy_port = state
        .ensure_proxy_for(active_ws)
        .await
        .map_err(wis_core::WisError::from)?;
    let proxy_url = format!("http://127.0.0.1:{proxy_port}")
        .parse::<Url>()
        .expect("loopback proxy URL is valid");

    #[cfg(target_os = "linux")]
    let autoplay_policy = state.autoplay_policy_for(parsed.as_str());

    let mut builder = WebviewBuilder::new(&label, WebviewUrl::External(parsed))
        .initialization_script(tauri_runtime_wry::SKIP_TAURI_SCRIPTS)
        .focused(true)
        .incognito(true)
        .disable_drag_drop_handler()
        .proxy_url(proxy_url);

    // Custom-scheme navigations app handoff.
    {
        let app_for_nav = app.clone();
        let label_for_nav = label.clone();
        builder = builder.on_navigation(move |url| {
            if crate::app_launch::should_intercept(url) {
                crate::app_launch::intake_from_webview(
                    &app_for_nav,
                    label_for_nav.clone(),
                    url.clone(),
                );
                return false;
            }
            true
        });
    }

    // Swipe back/forward inside the preview.
    let swipe_accent = state
        .swipe_accent
        .lock()
        .expect("swipe_accent poisoned")
        .clone();
    builder = builder.initialization_script(crate::swipe_nav::accent_script(&swipe_accent));
    builder = builder
        .initialization_script_for_all_frames(crate::gpc::page_script(global_privacy_control));

    #[cfg(target_os = "macos")]
    {
        builder = builder.initialization_script(crate::swipe_nav::phase_flag_script());
    }
    builder = builder.initialization_script(crate::swipe_nav::page_script());

    // Feed the gesture's direction gating and the surface's arrow dulling on
    // every navigation (also re-fires after the reader toggle's reload).
    {
        let app = app.clone();
        let window_label = window.label().to_string();
        builder = builder.on_page_load(move |webview, _payload| {
            #[cfg(target_os = "linux")]
            if matches!(_payload.event(), tauri::webview::PageLoadEvent::Started) {
                crate::autoplay::apply_for_page_load(&app, &webview, _payload.url().as_str());
            }

            emit_nav_state(&app, &window_label, &webview);
        });
    }

    // Fluent overlay scrollbars; WebView2 only. The message shim and the JS
    // find engine ride the builder so find-in-peek can search and highlight.
    #[cfg(windows)]
    {
        builder = builder.scroll_bar_style(tauri::webview::ScrollBarStyle::FluentOverlay);
        builder = builder
            .initialization_script(crate::win_dispatch::SHIM_SCRIPT)
            .initialization_script(crate::find::engine_script());
    }

    if let Some(ua) = &user_agent {
        builder = builder.user_agent(ua);
    }
    // Peek is always incognito, so its downloads stay out of history.
    builder = crate::downloads::attach_download_handler(builder, true);

    #[cfg(windows)]
    {
        let dir = if ephemeral {
            crate::commands::ephemeral_webview_data_dir(app, active_ws)
        } else {
            crate::commands::workspace_webview_data_dir(app, active_ws)
        };
        builder = builder.data_directory(dir);
    }

    let child = window
        .add_child(
            builder,
            tauri::LogicalPosition::new(x, y),
            tauri::LogicalSize::new(width.max(1.0), height.max(1.0)),
        )
        .map_err(|err| {
            log::warn!("peek add_child failed: {err}");
            WisError::InvalidInput {
                message: format!("peek add_child failed: {err}"),
            }
        })?;

    // Apple-proxied Safe Browsing warning.
    crate::webview_prefs::apply_fraud_warning(&child, fraud_warning);
    crate::webview_prefs::apply_media_extensions(&child, media_extensions);
    #[cfg(target_os = "linux")]
    crate::autoplay::apply(&child, autoplay_policy);

    // Per-site camera/microphone consent gate.
    crate::permissions::install(&child);

    // Native fallback for page-first editing keys (undo/redo/select-all).
    crate::edit_keys::install(&child);

    // Find-in-peek match counts (WebKitGTK FindController signals).
    #[cfg(target_os = "linux")]
    {
        let label_for_find = label.clone();
        let _ = child.with_webview(move |w| {
            let wv = w.inner();
            crate::find::install_linux(&wv, &label_for_find);
        });
    }

    // Round the preview's corners so it sits cleanly inside the card.
    #[cfg(target_os = "macos")]
    {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;

        let _ = child.with_webview(|w| unsafe {
            let view: *mut AnyObject = w.inner() as *mut AnyObject;
            if view.is_null() {
                return;
            }
            let _: () = msg_send![view, setWantsLayer: true];
            let layer: *mut AnyObject = msg_send![view, layer];
            if layer.is_null() {
                return;
            }
            let _: () = msg_send![layer, setCornerRadius: 12.0_f64];
            let _: () = msg_send![layer, setMasksToBounds: true];
        });
    }

    #[cfg(windows)]
    {
        crate::win_webview2::set_corner_radius(&child, 12.0);
        crate::win_dispatch::install_find_only(&child);
    }

    Ok(assessment)
}

/// Reposition/resize the preview webview. No-op if there's no live preview.
#[tauri::command]
pub async fn set_peek_bounds(
    window: tauri::Window,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> WisResult<()> {
    let label = peek_label(window.label());
    let Some(wv) = window.app_handle().webviews().get(&label).cloned() else {
        return Ok(());
    };

    wv.set_position(tauri::LogicalPosition::new(x, y))
        .map_err(|err| WisError::InvalidInput {
            message: format!("peek set_position: {err}"),
        })?;
    wv.set_size(tauri::LogicalSize::new(width.max(1.0), height.max(1.0)))
        .map_err(|err| WisError::InvalidInput {
            message: format!("peek set_size: {err}"),
        })?;

    #[cfg(windows)]
    crate::win_webview2::reapply_corner_radius(&wv, (width.max(1.0), height.max(1.0)));

    Ok(())
}

/// Destroy the preview webview.
#[tauri::command]
pub async fn close_peek(window: tauri::Window) -> WisResult<()> {
    let label = peek_label(window.label());
    if let Some(wv) = window.app_handle().webviews().get(&label).cloned() {
        crate::webview_close::terminate_page(&wv);
        wv.close().map_err(|err| WisError::InvalidInput {
            message: format!("peek close: {err}"),
        })?;
    }
    #[cfg(windows)]
    {
        crate::win_webview2::forget_corner_radius(&label);
        crate::win_focus::restore_webview_focus(window.app_handle(), window.label());
    }

    Ok(())
}

/// Reload the preview. No-op without a live preview. Native reload rather
/// than an eval, so a hung preview still responds.
#[tauri::command]
pub async fn peek_reload(window: tauri::Window) -> WisResult<()> {
    let Some(label) = live_peek_label(&window) else {
        return Ok(());
    };
    let Some(wv) = window.app_handle().webviews().get(&label).cloned() else {
        return Ok(());
    };

    wv.reload().map_err(|err| WisError::InvalidInput {
        message: format!("peek reload failed: {err}"),
    })
}

/// Navigate the preview back in its history. No-op without a live preview.
#[tauri::command]
pub async fn peek_go_back(window: tauri::Window) -> WisResult<()> {
    peek_history_step(&window, "back")
}

/// Counterpart to [`peek_go_back`].
#[tauri::command]
pub async fn peek_go_forward(window: tauri::Window) -> WisResult<()> {
    peek_history_step(&window, "forward")
}

fn peek_history_step(window: &tauri::Window, direction: &str) -> WisResult<()> {
    let label = peek_label(window.label());
    let app = window.app_handle().clone();
    let Some(wv) = app.webviews().get(&label).cloned() else {
        return Ok(());
    };

    wv.eval(format!("window.history.{direction}()"))
        .map_err(|err| WisError::InvalidInput {
            message: format!("peek history.{direction}() eval failed: {err}"),
        })?;

    // A same-document step fires no load event; re-read once it settles so
    // the arrows and gesture gating stay fresh.
    let window_label = window.label().to_string();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        let Some(wv) = app.webviews().get(&label).cloned() else {
            return;
        };

        emit_nav_state(&app, &window_label, &wv);
    });

    Ok(())
}

/// Toggle reader mode in the isolated preview. When enabled, the page is
/// transformed in place into a clean article; when disabled, it reloads to
/// restore the live page. The transform runs entirely inside the isolated
/// webview, so no untrusted markup reaches the chrome.
#[tauri::command]
pub async fn peek_set_reader(window: tauri::Window, enabled: bool) -> WisResult<()> {
    let label = peek_label(window.label());
    let Some(wv) = window.app_handle().webviews().get(&label).cloned() else {
        return Ok(());
    };

    let js = if enabled {
        crate::reader::reader_script()
    } else {
        "location.reload();"
    };
    wv.eval(js).map_err(|err| WisError::InvalidInput {
        message: format!("peek reader eval: {err}"),
    })
}

/// Injected page script (tab webviews): Opt+click a link to preview it instead
/// of navigating. No-ops when the handler isn't present.
const PEEK_JS: &str = r#"(function () {
  if (window.__wisPeekInstalled) return;
  window.__wisPeekInstalled = true;
  function linkAt(e) {
    if (!e.isTrusted || !e.altKey || e.button !== 0) return null;
    var t = e.target;
    var el = (t && t.nodeType === 1) ? t : (t && t.parentElement);
    var a = el && el.closest ? el.closest('a[href]') : null;
    if (!a) return null;
    var href = a.href || '';
    if (!href || href.toLowerCase().indexOf('javascript:') === 0) return null;
    return a;
  }
  function fire(a, e) {
    var href = a.href || '';
    var text = (a.textContent || '').replace(/\s+/g, ' ').trim().slice(0, 200);
    try {
      if (window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.wisPeek) {
        window.webkit.messageHandlers.wisPeek.postMessage({ href: href, text: text, x: e.clientX, y: e.clientY });
      }
    } catch (err) {}
  }
  document.addEventListener('mousedown', function (e) {
    var a = linkAt(e);
    if (!a) return;
    e.preventDefault();
    e.stopPropagation();
    fire(a, e);
  }, true);
  document.addEventListener('click', function (e) {
    if (linkAt(e)) { e.preventDefault(); e.stopPropagation(); }
  }, true);
})();"#;

/// The Opt+click trigger: a builder-level initialization script on Windows,
/// where it runs in the page's world, and installed in Wis's own world on
/// Linux.
#[cfg(not(target_os = "macos"))]
pub fn page_script() -> &'static str {
    PEEK_JS
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;

    use objc2::rc::Id;
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{
        class, declare_class, msg_send, msg_send_id, mutability, ClassType, DeclaredClass,
    };
    use tauri::Emitter;

    use super::{PeekOpenEvent, APP, PEEK_JS, PEEK_OPEN_EVENT};
    use crate::content_world::macos as content_world;

    declare_class!(
        struct PeekHandler;

        unsafe impl ClassType for PeekHandler {
            type Super = NSObject;
            type Mutability = mutability::InteriorMutable;
            const NAME: &'static str = "WisPeekHandler";
        }

        impl DeclaredClass for PeekHandler {
            type Ivars = ();
        }

        unsafe impl PeekHandler {
            #[method(userContentController:didReceiveScriptMessage:)]
            fn did_receive(&self, _ucc: *mut AnyObject, message: *mut AnyObject) {
                unsafe { handle_message(message) }
            }
        }
    );

    impl PeekHandler {
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

    unsafe fn dict_f64(dict: *mut AnyObject, key: &str) -> Option<f64> {
        let k = ns_string(key);
        let v: *mut AnyObject = msg_send![dict, objectForKey: k];
        if v.is_null() {
            return None;
        }

        let is_num: bool = msg_send![v, isKindOfClass: class!(NSNumber)];
        if !is_num {
            return None;
        }

        let d: f64 = msg_send![v, doubleValue];
        Some(d)
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
        let x = dict_f64(body, "x").unwrap_or(-1.0);
        let y = dict_f64(body, "y").unwrap_or(-1.0);

        if let Some(app) = APP.get() {
            let _ = app.emit(PEEK_OPEN_EVENT, PeekOpenEvent { href, text, x, y });
        }
    }

    /// Install the Opt+click trigger and its `wisPeek` handler on a tab webview.
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

            let handler = PeekHandler::new();
            content_world::add_handler(controller, &*handler, "wisPeek");
            content_world::install_script(wk, controller, &label, "wisPeek", PEEK_JS, true);
        });
    }
}

/// Install the Opt+click peek channel on a tab webview. No-op off macOS.
pub fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
    #[cfg(target_os = "macos")]
    imp::install(webview);
    #[cfg(not(target_os = "macos"))]
    let _ = webview;
}
