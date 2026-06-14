//! Floating overlay webviews layered above the tab content webviews.
//!
//! Tab webviews are added via `add_child` and composite above the root
//! chrome webview by creation order, so any HTML positioned "over" the
//! page is invisible. This module mints transparent child webviews after
//! the tab webviews so they composite on top, sized only to the region
//! their surface needs (so clicks elsewhere fall through to the page).
//!
//! Each overlay loads the main app bundle. The JS entry routes to the
//! correct surface using the webview LABEL (`overlay-<name>`), not the
//! URL query.
//!
//! Three commands:
//!   - [`focus_webview`]: promote any named webview to first responder.
//!     Used by overlay surfaces (whose input would otherwise be ignored
//!     while a tab webview holds focus) and by the main chrome.
//!   - [`ensure_overlay`]: lazily create the overlay webview if it does
//!     not already exist. Idempotent.
//!   - [`set_overlay_bounds`] / [`hide_overlay`]: reposition/resize, or
//!     remove the overlay from the view hierarchy.
//!
//! Overlay labels follow `overlay-<name>` to keep them distinct from tab
//! webviews (which are `tab-<uuid>`).

use tauri::webview::WebviewBuilder;
use tauri::{AppHandle, Manager, WebviewUrl};
use wis_core::{WisError, WisResult};

const OVERLAY_PREFIX: &str = "overlay-";

fn overlay_label(name: &str) -> String {
    format!("{OVERLAY_PREFIX}{name}")
}

/// Re-promote every existing overlay webview to the top of its window's
/// subview stack.
pub fn promote_overlays_to_front(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;

        let overlays: Vec<_> = app
            .webviews()
            .iter()
            .filter(|(label, _)| label.starts_with(OVERLAY_PREFIX))
            .map(|(_, wv)| wv.clone())
            .collect();

        for wv in overlays {
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

/// Promote the named webview to its window's first responder.
///
/// `label` may name a tab webview, an overlay webview, or the main
/// chrome window. Missing labels resolve to a no-op.
#[tauri::command]
pub async fn focus_webview(app: AppHandle, label: String) -> WisResult<()> {
    #[cfg(target_os = "macos")]
    {
        use objc2::msg_send;
        use objc2::runtime::{AnyObject, Bool};

        let child = app.webviews().get(&label).cloned();
        let window = if child.is_none() {
            app.get_webview_window(&label)
        } else {
            None
        };

        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let make_first_responder = move |w: tauri::webview::PlatformWebview| unsafe {
            let view: *mut AnyObject = w.inner() as *mut AnyObject;
            if !view.is_null() {
                let window: *mut AnyObject = msg_send![view, window];
                if !window.is_null() {
                    let _: Bool = msg_send![window, makeFirstResponder: view];
                }
            }
            let _ = tx.send(());
        };

        let outcome = if let Some(wv) = child {
            wv.with_webview(make_first_responder)
        } else if let Some(win) = window {
            win.with_webview(make_first_responder)
        } else {
            return Ok(());
        };

        outcome.map_err(|err| WisError::InvalidInput {
            message: format!("with_webview failed: {err}"),
        })?;
        let _ = rx.recv_timeout(std::time::Duration::from_millis(500));
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, label);
    }

    Ok(())
}

/// Lazily create the overlay child webview for `name` if absent.
/// Idempotent: calling again on an existing overlay is a no-op.
#[tauri::command]
pub async fn ensure_overlay(app: AppHandle, name: String) -> WisResult<()> {
    let label = overlay_label(&name);
    if app.webviews().contains_key(&label) {
        return Ok(());
    }

    let window = app
        .get_window("main")
        .ok_or_else(|| WisError::InvalidInput {
            message: "main window missing".into(),
        })?;

    let url = WebviewUrl::App(format!("index.html?surface={name}").into());
    let builder = WebviewBuilder::new(&label, url)
        .focused(false)
        .transparent(true)
        .disable_drag_drop_handler();

    let wv = window
        .add_child(
            builder,
            tauri::LogicalPosition::new(0.0_f64, 0.0_f64),
            tauri::LogicalSize::new(1.0_f64, 1.0_f64),
        )
        .map_err(|err| WisError::InvalidInput {
            message: format!("add_child for {label} failed: {err}"),
        })?;

    wv.hide().map_err(|err| WisError::InvalidInput {
        message: format!("initial hide for {label}: {err}"),
    })?;

    log::info!("overlay {label} ensured");
    Ok(())
}

#[tauri::command]
pub async fn set_overlay_bounds(
    app: AppHandle,
    name: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> WisResult<()> {
    let label = overlay_label(&name);
    let Some(wv) = app.webviews().get(&label).cloned() else {
        return Err(WisError::InvalidInput {
            message: format!("overlay {label} not found; call ensure_overlay first"),
        });
    };
    wv.set_position(tauri::LogicalPosition::new(x, y))
        .map_err(|err| WisError::InvalidInput {
            message: format!("set_position for {label}: {err}"),
        })?;
    wv.set_size(tauri::LogicalSize::new(width.max(1.0), height.max(1.0)))
        .map_err(|err| WisError::InvalidInput {
            message: format!("set_size for {label}: {err}"),
        })?;
    wv.show().map_err(|err| WisError::InvalidInput {
        message: format!("show for {label}: {err}"),
    })?;
    Ok(())
}

/// Position + size an overlay **without showing it**. Lets the chrome
/// pre-size a hidden overlay to its eventual bounds so the first real show
/// does not resize the webview (a resize concurrent with an entrance
/// animation makes the content render at a transient layout for a frame).
#[tauri::command]
pub async fn size_overlay(
    app: AppHandle,
    name: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> WisResult<()> {
    let label = overlay_label(&name);
    let Some(wv) = app.webviews().get(&label).cloned() else {
        return Err(WisError::InvalidInput {
            message: format!("overlay {label} not found; call ensure_overlay first"),
        });
    };

    wv.set_position(tauri::LogicalPosition::new(x, y))
        .map_err(|err| WisError::InvalidInput {
            message: format!("set_position for {label}: {err}"),
        })?;
    wv.set_size(tauri::LogicalSize::new(width.max(1.0), height.max(1.0)))
        .map_err(|err| WisError::InvalidInput {
            message: format!("set_size for {label}: {err}"),
        })?;

    Ok(())
}

#[tauri::command]
pub async fn hide_overlay(app: AppHandle, name: String) -> WisResult<()> {
    let label = overlay_label(&name);
    let Some(wv) = app.webviews().get(&label).cloned() else {
        return Ok(());
    };
    wv.hide().map_err(|err| WisError::InvalidInput {
        message: format!("hide for {label}: {err}"),
    })
}
