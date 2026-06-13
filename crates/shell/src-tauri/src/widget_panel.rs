//! Quick-launch widget panels: per-widget child webviews floated above the
//! tab webviews.
//!
//! Each opened widget gets its own panel webview (`widget-panel-<id>`), so
//! switching widgets is a hide/show of already-loaded pages rather than a
//! navigation: instant, and every widget keeps its own in-page state. The
//! frontend keeps the pool small (LRU cap + tab-sleep idle destroy).
//!
//! A panel shares the active workspace's data store (so logins persist like a
//! tab's would) and routes through the workspace proxy; an ephemeral (private)
//! workspace gets an incognito panel instead. Like tab webviews it holds no
//! Tauri capability, so the page can never call a command. The chrome owns
//! open / bounds / close; dismissal-on-outside-interaction is wired
//! frontend-side, as are the guardrails (hard-destroy on workspace switch,
//! destroy outright in private workspaces, idle destroy).

use serde::Serialize;
use tauri::webview::{PageLoadEvent, PageLoadPayload, WebviewBuilder};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl};
use url::Url;
use wis_core::{WisError, WisResult};

use crate::state::AppState;

/// Label prefix of widget panel webviews; the suffix is the widget's id.
const WIDGET_PANEL_PREFIX: &str = "widget-panel-";

/// Broadcast whenever a panel webview is created or destroyed (not on
/// hide/show), so the settings page can refresh its live-panel readout.
const WIDGET_PANELS_CHANGED_EVENT: &str = "widget-panels-changed";

/// Panel webview -> chrome: the panel's committed URL on each main-frame
/// navigation, so the header bar can show the widget's current host.
const WPANEL_NAV_EVENT: &str = "wis-wpanel-nav";

#[derive(Clone, Serialize)]
struct WPanelNavPayload {
    id: String,
    url: String,
}

fn notify_panels_changed(app: &AppHandle) {
    if let Err(err) = app.emit(WIDGET_PANELS_CHANGED_EVENT, ()) {
        log::warn!("emit {WIDGET_PANELS_CHANGED_EVENT} failed: {err}");
    }
}

/// Re-promote every live widget panel to the top of the window's subview
/// stack. Tab webviews are created lazily (sleep-wake, splits, layout
/// changes) and each `add_child` lands on top of everything, so panel
/// stacking must be restored whenever a tab webview is created and whenever
/// a panel is shown (a hidden panel keeps its stale position). Callers
/// re-promote overlays afterwards, preserving tabs < panels < overlays.
pub fn promote_panels_to_front(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;

        let panels: Vec<_> = app
            .webviews()
            .iter()
            .filter(|(label, _)| label.starts_with(WIDGET_PANEL_PREFIX))
            .map(|(_, wv)| wv.clone())
            .collect();

        for wv in panels {
            let _ = wv.with_webview(|w| unsafe {
                let view: *mut AnyObject = w.inner() as *mut AnyObject;
                if view.is_null() {
                    return;
                }
                let superview: *mut AnyObject = msg_send![view, superview];
                if superview.is_null() {
                    return;
                }
                let _: () = msg_send![superview, addSubview: view];
            });
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
    }
}

/// Validate the frontend-supplied widget id before it becomes part of a
/// webview label (UUIDs in practice; reject anything else).
fn panel_label(id: &str) -> WisResult<String> {
    let valid = !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !valid {
        return Err(WisError::InvalidInput {
            message: format!("invalid widget panel id: {id:?}"),
        });
    }

    Ok(format!("{WIDGET_PANEL_PREFIX}{id}"))
}

/// Open widget `id`'s panel at `url` with the given rect. If that panel is
/// already live (visible or hidden) it is revealed, navigating only if needed.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn open_widget_panel(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    url: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    dark: bool,
) -> WisResult<()> {
    let label = panel_label(&id)?;
    let parsed = Url::parse(&url).map_err(|_| WisError::InvalidUrl { url: url.clone() })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(WisError::InvalidInput {
            message: format!("widget panel refuses non-web URL: {}", parsed.scheme()),
        });
    }

    // Already live: just reveal it (state kept; no reload).
    if app.webviews().contains_key(&label) {
        let shown = show_widget_panel(app, id, x, y, width, height).await?;
        debug_assert!(shown);
        return Ok(());
    }

    let (workspace_id, ephemeral, privacy_mode, user_agent, fraud_warning, media_extensions) = {
        let browser = state.browser.lock().await;
        let ws_id = browser.active_workspace_id;
        let (ephemeral, privacy_mode) = browser
            .workspace(ws_id)
            .map(|w| (w.ephemeral, w.privacy_mode))
            .unwrap_or((false, Default::default()));
        let user_agent = wis_core::ua_for_host(
            parsed.host_str().unwrap_or_default(),
            &browser.settings.ua_site_overrides,
            &browser.settings.user_agent_profile,
            wis_core::DEFAULT_MAC_OS_VERSION,
        );

        (
            ws_id,
            ephemeral,
            privacy_mode,
            user_agent,
            browser.settings.fraud_warning_enabled,
            browser.settings.media_extensions_enabled,
        )
    };

    let proxy_port = state
        .ensure_proxy_for(workspace_id)
        .await
        .map_err(wis_core::WisError::from)?;
    let proxy_url = format!("http://127.0.0.1:{proxy_port}")
        .parse::<Url>()
        .expect("loopback proxy URL is valid");

    let window = app
        .get_window("main")
        .ok_or_else(|| WisError::InvalidInput {
            message: "main window missing".into(),
        })?;

    let mut builder = WebviewBuilder::new(&label, WebviewUrl::External(parsed))
        .focused(true)
        .proxy_url(proxy_url);
    if let Some(ua) = &user_agent {
        builder = builder.user_agent(ua);
    }
    if ephemeral {
        builder = builder.incognito(true);
    } else {
        builder = builder.data_store_identifier(crate::commands::workspace_id_to_data_store_bytes(
            workspace_id,
        ));
    }
    // File downloads from widget sites go through the same sanitised-download pipeline as tabs.
    builder = crate::downloads::attach_download_handler(builder, ephemeral);
    builder = builder.on_new_window({
        let app = app.clone();
        move |url, _features| {
            crate::popup_chrome::ensure_running(&app, url.as_str());
            tauri::webview::NewWindowResponse::Allow
        }
    });

    // Report the panel's URL on each navigation so the header shows its host.
    builder = builder.on_page_load({
        let app = app.clone();
        let id = id.clone();
        move |_webview, payload: PageLoadPayload<'_>| {
            if matches!(payload.event(), PageLoadEvent::Started) {
                let _ = app.emit(
                    WPANEL_NAV_EVENT,
                    WPanelNavPayload {
                        id: id.clone(),
                        url: payload.url().to_string(),
                    },
                );
            }
        }
    });

    let child = window
        .add_child(
            builder,
            tauri::LogicalPosition::new(x, y),
            tauri::LogicalSize::new(width.max(1.0), height.max(1.0)),
        )
        .map_err(|err| WisError::InvalidInput {
            message: format!("widget panel add_child failed: {err}"),
        })?;

    // Style before first paint.
    let _ = child.hide();
    // Round only the bottom corners: the top edge sits flush under the
    // accent header bar (overlay-widgetheader), which carries the top
    // rounding, so the two read as one card.
    #[cfg(target_os = "macos")]
    {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};

        let _ = child.with_webview(move |w| unsafe {
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
            let mask: usize = (1 << 2) | (1 << 3);
            let _: () = msg_send![layer, setMaskedCorners: mask];
            let _: () = msg_send![layer, setMasksToBounds: true];

            // The engine draws no background, but the layer carries the theme
            // surface colour (mirrors --surface-raised in tokens.css).
            let key_c = std::ffi::CString::new("drawsBackground").expect("no NUL");
            let key: *mut AnyObject =
                msg_send![class!(NSString), stringWithUTF8String: key_c.as_ptr()];
            let no_value: *mut AnyObject = msg_send![class!(NSNumber), numberWithBool: false];
            let _: () = msg_send![view, setValue: no_value, forKey: key];
            let (r, g, b) = if dark {
                (0.255_f64, 0.255_f64, 0.298_f64) // #41414C
            } else {
                (0.980_f64, 0.980_f64, 0.976_f64) // #FAFAF9
            };
            let surface: *mut AnyObject = msg_send![
                class!(NSColor),
                colorWithSRGBRed: r, green: g, blue: b, alpha: 1.0_f64
            ];
            if !surface.is_null() {
                #[repr(transparent)]
                #[derive(Clone, Copy)]
                struct CGColorRef(*const std::ffi::c_void);
                unsafe impl objc2::encode::Encode for CGColorRef {
                    const ENCODING: objc2::encode::Encoding = objc2::encode::Encoding::Pointer(
                        &objc2::encode::Encoding::Struct("CGColor", &[]),
                    );
                }
                let cg: CGColorRef = msg_send![surface, CGColor];
                if !cg.0.is_null() {
                    let _: () = msg_send![layer, setBackgroundColor: cg];
                }
                let _: () = msg_send![view, setUnderPageBackgroundColor: surface];
            }
        });
    }
    let _ = child.show();

    // Apple-proxied Safe Browsing warning.
    crate::webview_prefs::apply_fraud_warning(&child, fraud_warning);
    crate::webview_prefs::apply_media_extensions(&child, media_extensions);

    // Per-site camera/microphone consent gate.
    crate::permissions::install(&child);

    // A muted widget stays muted across panel reclaim/reopen.
    {
        let muted = state
            .muted_widgets
            .lock()
            .expect("muted_widgets poisoned")
            .contains(&id);
        if muted {
            crate::audio::apply_page_muted(&child, true);
        }
    }

    // Keep overlays (incl. the resize strip) above the freshly added panel.
    crate::overlay::promote_overlays_to_front(&app);
    notify_panels_changed(&app);

    // Same engine-level tracker blocking as tab webviews; best-effort.
    if let Some(spec) = state.content_rule_spec(privacy_mode) {
        if let Err(err) = crate::content_filter::attach_rule_list(&child, &spec) {
            log::warn!("widget panel: content filter attach skipped: {err}");
        }
    }

    Ok(())
}

/// Re-show widget `id`'s hidden panel without navigating, preserving its
/// in-page state. Returns `false` when that panel isn't live (the caller
/// should open it).
#[tauri::command]
pub async fn show_widget_panel(
    app: AppHandle,
    id: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> WisResult<bool> {
    let label = panel_label(&id)?;
    let Some(wv) = app.webviews().get(&label).cloned() else {
        return Ok(false);
    };
    wv.show().map_err(|err| WisError::InvalidInput {
        message: format!("widget panel show: {err}"),
    })?;
    set_media_suspended(&wv, false);

    promote_panels_to_front(&app);
    crate::overlay::promote_overlays_to_front(&app);
    set_widget_panel_bounds(app, id, x, y, width, height).await?;

    Ok(true)
}

/// Navigate widget `id`'s live panel to a new URL (e.g. re-applying a
/// theme-dependent query parameter). No-op when the panel isn't live.
#[tauri::command]
pub async fn navigate_widget_panel(app: AppHandle, id: String, url: String) -> WisResult<()> {
    let label = panel_label(&id)?;
    let parsed = Url::parse(&url).map_err(|_| WisError::InvalidUrl { url: url.clone() })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(WisError::InvalidInput {
            message: format!("widget panel refuses non-web URL: {}", parsed.scheme()),
        });
    }
    let Some(wv) = app.webviews().get(&label).cloned() else {
        return Ok(());
    };
    wv.navigate(parsed).map_err(|err| WisError::InvalidInput {
        message: format!("widget panel navigate: {err}"),
    })?;

    Ok(())
}

/// Hard-reload widget `id`'s panel (bypasses caches). Native `reloadFromOrigin`
/// rather than injected JS, so it works even when the page is hung and would
/// never execute an injected `location.reload()`. No-op when the panel isn't
/// live.
#[tauri::command]
pub async fn reload_widget_panel(app: AppHandle, id: String) -> WisResult<()> {
    let label = panel_label(&id)?;
    let Some(wv) = app.webviews().get(&label).cloned() else {
        return Ok(());
    };

    #[cfg(target_os = "macos")]
    {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        wv.with_webview(|w| unsafe {
            let wk: *mut AnyObject = w.inner() as *mut AnyObject;
            if !wk.is_null() {
                let _: *mut AnyObject = msg_send![wk, reloadFromOrigin];
            }
        })
        .map_err(|err| WisError::InvalidInput {
            message: format!("widget panel reload: {err}"),
        })?;
    }

    #[cfg(not(target_os = "macos"))]
    {
        wv.eval("window.location.reload()")
            .map_err(|err| WisError::InvalidInput {
                message: format!("widget panel reload eval: {err}"),
            })?;
    }

    Ok(())
}

/// Hide widget `id`'s panel, keeping the page alive for an instant re-show.
/// No-op when it isn't live.
#[tauri::command]
pub async fn hide_widget_panel(app: AppHandle, id: String) -> WisResult<()> {
    let label = panel_label(&id)?;
    if let Some(wv) = app.webviews().get(&label).cloned() {
        wv.hide().map_err(|err| WisError::InvalidInput {
            message: format!("widget panel hide: {err}"),
        })?;

        set_media_suspended(&wv, true);
    }

    Ok(())
}

/// Suspend or resume all media playback in a panel webview (macOS 11.3+
/// public API; silently skipped when the selector is unavailable).
fn set_media_suspended<R: tauri::Runtime>(wv: &tauri::webview::Webview<R>, suspended: bool) {
    let _ = wv.with_webview(move |w| {
        #[cfg(target_os = "macos")]
        unsafe {
            use objc2::runtime::AnyObject;
            use objc2::{msg_send, sel};

            let wk = w.inner() as *mut AnyObject;
            if wk.is_null() {
                return;
            }

            let responds: bool = msg_send![
                wk,
                respondsToSelector: sel!(setAllMediaPlaybackSuspended:completionHandler:)
            ];
            if !responds {
                return;
            }

            let nil_block: *mut AnyObject = std::ptr::null_mut();
            let _: () = msg_send![
                wk,
                setAllMediaPlaybackSuspended: suspended,
                completionHandler: nil_block
            ];
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = (w, suspended);
        }
    });
}

/// Reposition/resize widget `id`'s panel. No-op when it isn't live.
#[tauri::command]
pub async fn set_widget_panel_bounds(
    app: AppHandle,
    id: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> WisResult<()> {
    let label = panel_label(&id)?;
    let Some(wv) = app.webviews().get(&label).cloned() else {
        return Ok(());
    };

    wv.set_position(tauri::LogicalPosition::new(x, y))
        .map_err(|err| WisError::InvalidInput {
            message: format!("widget panel set_position: {err}"),
        })?;
    wv.set_size(tauri::LogicalSize::new(width.max(1.0), height.max(1.0)))
        .map_err(|err| WisError::InvalidInput {
            message: format!("widget panel set_size: {err}"),
        })?;

    Ok(())
}

/// Destroy widget `id`'s panel webview. No-op when it isn't live.
#[tauri::command]
pub async fn close_widget_panel(app: AppHandle, id: String) -> WisResult<()> {
    let label = panel_label(&id)?;
    if let Some(wv) = app.webviews().get(&label).cloned() {
        wv.close().map_err(|err| WisError::InvalidInput {
            message: format!("widget panel close: {err}"),
        })?;
        notify_panels_changed(&app);
    }

    Ok(())
}

/// Destroy every live widget panel (workspace switch, dock unmount).
#[tauri::command]
pub async fn close_all_widget_panels(app: AppHandle) -> WisResult<()> {
    let panels: Vec<_> = app
        .webviews()
        .iter()
        .filter(|(label, _)| label.starts_with(WIDGET_PANEL_PREFIX))
        .map(|(_, wv)| wv.clone())
        .collect();
    let any = !panels.is_empty();
    for wv in panels {
        if let Err(err) = wv.close() {
            log::warn!("widget panel close: {err}");
        }
    }
    if any {
        notify_panels_changed(&app);
    }

    Ok(())
}

/// Mute or unmute widget `id`'s page (engine-level, so messaging pings stay
/// silent too). Session-scoped: re-applied when a reclaimed panel reopens.
#[tauri::command]
pub async fn set_widget_panel_muted(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    muted: bool,
) -> WisResult<()> {
    let label = panel_label(&id)?;
    {
        let mut set = state.muted_widgets.lock().expect("muted_widgets poisoned");
        if muted {
            set.insert(id);
        } else {
            set.remove(&id);
        }
    }
    if let Some(wv) = app.webviews().get(&label).cloned() {
        crate::audio::apply_page_muted(&wv, muted);
    }

    Ok(())
}

/// Whether widget `id` is currently muted.
#[tauri::command]
pub async fn widget_panel_muted(state: State<'_, AppState>, id: String) -> WisResult<bool> {
    Ok(state
        .muted_widgets
        .lock()
        .expect("muted_widgets poisoned")
        .contains(&id))
}

/// Widget ids of the currently live panel webviews (visible or hidden), for
/// the settings Performance readout.
#[tauri::command]
pub async fn list_widget_panels(app: AppHandle) -> WisResult<Vec<String>> {
    Ok(app
        .webviews()
        .keys()
        .filter_map(|label| label.strip_prefix(WIDGET_PANEL_PREFIX).map(str::to_string))
        .collect())
}
