//! Video popout: a floating, always-on-top mini player in its own native
//! window, free to live outside the main browser window.
//!
//! A second video pipeline is impossible here (MSE/DRM streams cannot be
//! re-opened in another webview), so the popout *is the tab's own webview*:
//! when the chrome decides a tab should pop out, the webview is reparented
//! into a small borderless `video-popout` window and an injected script pins
//! the playing `<video>` across the whole viewport. While popped, the video
//! element is reparented to the document root (original slot restored on exit),
//! so no player wrapper can re-anchor, scale, or crop the fixed pinning, and
//! the whole `<body>` is visibility-hidden over a black root so nothing can
//! paint above it or fade it out.
//!
//! An overlay provides the controls: an X (top right),
//! click to pause/play, double-click left/right to seek -/+10s, drag
//! anywhere to move the window (native `start_dragging`), and corner zones
//! to resize aspect-locked. Returning to the tab (or X) reparents the
//! webview back into the main window and unwinds the page styling.
//!
//! Split of responsibilities: the page script only reports intents over the
//! narrow `wisPopout` message handler (it holds no Tauri capability); this
//! module owns the native window lifecycle and validates every payload; the
//! chrome decides *when* to pop out (automatically when a tab playing video
//! leaves the visible layout, or explicitly via the in-page hover button /
//! the Pop Out Video key action) and persists the remembered geometry.
//! The sender of each message is identified by the `accessibilityIdentifier`
//! stamped on the tab's WKWebView, never by page-supplied data, so a hostile
//! page can at worst fiddle with its own popout.

use std::sync::OnceLock;

use tauri::{AppHandle, Manager, State};
use wis_core::{WisError, WisResult};

use crate::state::AppState;
mod geometry;
mod script;

pub(crate) use geometry::MAX_DELTA;
pub use geometry::*;
pub use script::page_script;

static APP: OnceLock<AppHandle> = OnceLock::new();

pub fn set_app_handle(app: AppHandle) {
    let _ = APP.set(app);
}

/// Label of the floating popout window (at most one exists).
pub const POPOUT_WINDOW_LABEL: &str = "video-popout";

/// Tab webview labels are `tab-<uuid>`; the uuid is what events carry.
const TAB_LABEL_PREFIX: &str = "tab-";

/// Media-state changes per tab: `{ tab, playing, present }`. `playing` gates
/// the automatic popout on tab switch; `present` (a usable video exists,
/// playing or not) gates the manual triggers. The chrome keeps both maps.
const MEDIA_EVENT: &str = "wis-popout-media";
/// The page's hover button asked to pop out: `{ tab }`. The chrome decides
/// (same gates as the Pop Out Video key action).
const REQUESTED_EVENT: &str = "wis-popout-requested";
/// The popout window opened for `{ tab }` (enter handshake completed).
const OPENED_EVENT: &str = "wis-popout-opened";
/// The popout closed for `{ tab, x?, y?, width? }`; geometry present when a
/// window was live, for the chrome to remember.
const CLOSED_EVENT: &str = "wis-popout-closed";

const ENTER_EVAL: &str = "window.__wisPopoutEnter && window.__wisPopoutEnter();";
const EXIT_EVAL: &str = "window.__wisPopoutExit && window.__wisPopoutExit();";
/// Moving a WKWebView between windows (reparent) pauses its media; this
/// re-kicks playback page-side after each move, unless the pause was the
/// user's own (X button, click-pause).
const RESUME_EVAL: &str = "window.__wisPopoutResume && window.__wisPopoutResume();";

/// Popout bookkeeping on [`AppState`]. `pending` bridges the enter handshake
/// (open command issued, page not yet confirmed); `active` is the live
/// window. Both are label-keyed so stray or forged page messages for any
/// other webview are ignored.
#[derive(Default)]
pub struct VideoPopoutState {
    pub pending: Option<PendingOpen>,
    pub active: Option<ActivePopout>,
}

pub struct PendingOpen {
    pub label: String,
    pub remembered_pos: Option<(f64, f64)>,
}

pub struct ActivePopout {
    pub label: String,
    pub aspect: f64,
    /// Label of the window the tab lives in, so folding the popout back in
    /// reparents to the right window.
    pub origin_window: String,
}

fn validate_label(label: &str) -> WisResult<()> {
    let valid = label.strip_prefix(TAB_LABEL_PREFIX).is_some_and(|suffix| {
        !suffix.is_empty()
            && suffix.len() <= 64
            && suffix
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
    });
    if !valid {
        return Err(WisError::InvalidInput {
            message: format!("invalid tab webview label: {label:?}"),
        });
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Commands (chrome -> native)
// ---------------------------------------------------------------------------

/// Pop `label`'s tab out into the floating player. The page confirms via the
/// enter handshake (`entered` message), which actually creates the window;
/// if no suitable video is present the webview simply stays hidden.
/// `x`/`y` carry the chrome-remembered position of the last popout; the size
/// always starts from the predefined default.
#[tauri::command]
pub async fn open_video_popout(
    app: AppHandle,
    state: State<'_, AppState>,
    label: String,
    x: Option<f64>,
    y: Option<f64>,
) -> WisResult<()> {
    validate_label(&label)?;

    #[cfg(target_os = "macos")]
    {
        let Some(webview) = app.webviews().get(&label).cloned() else {
            return Ok(());
        };

        // Single-popout invariant: an existing popout for another tab is
        // restored first; re-opening the same tab is a no-op.
        let previous = {
            let guard = state.video_popout.lock().expect("video_popout poisoned");
            guard.active.as_ref().map(|a| a.label.clone())
        };
        if let Some(prev) = previous {
            if prev == label {
                return Ok(());
            }
            imp::close_and_notify(&app, &prev, true);
        }

        {
            let mut guard = state.video_popout.lock().expect("video_popout poisoned");
            guard.pending = Some(PendingOpen {
                label: label.clone(),
                remembered_pos: x.zip(y),
            });
        }

        webview
            .eval(ENTER_EVAL)
            .map_err(|err| WisError::InvalidInput {
                message: format!("video popout enter eval: {err}"),
            })?;
    }

    #[cfg(not(target_os = "macos"))]
    let _ = (app, state, label, x, y);

    Ok(())
}

/// Restore `label`'s tab out of the popout (page styling unwound, webview
/// reparented back into the main window, window closed). Emits the closed
/// event with the final geometry so the chrome can remember it. Safe to call
/// in any phase: a pending-only handshake is cancelled, no popout is a no-op.
#[tauri::command]
pub async fn close_video_popout(
    app: AppHandle,
    state: State<'_, AppState>,
    label: String,
) -> WisResult<()> {
    validate_label(&label)?;

    #[cfg(target_os = "macos")]
    {
        let pending_cancelled = {
            let mut guard = state.video_popout.lock().expect("video_popout poisoned");
            match &guard.pending {
                Some(p) if p.label == label => {
                    guard.pending = None;
                    true
                }
                _ => false,
            }
        };
        if pending_cancelled {
            // The page may have applied popout styling before the cancel.
            if let Some(webview) = app.webviews().get(&label).cloned() {
                let _ = webview.eval(EXIT_EVAL);
            }
        }

        imp::close_and_notify(&app, &label, true);
    }

    #[cfg(not(target_os = "macos"))]
    let _ = (app, state, label);

    Ok(())
}

/// A real navigation in a popped-out tab destroys the page (and its pinned
/// video) under the player; restore the tab to a plain hidden webview.
/// Called from the tab webview's page-load hook; no-op for other tabs.
pub fn handle_navigation(app: &AppHandle, label: &str) {
    #[cfg(target_os = "macos")]
    {
        let is_active = {
            let state = app.state::<AppState>();
            let guard = state.video_popout.lock().expect("video_popout poisoned");
            guard.active.as_ref().is_some_and(|a| a.label == label)
        };
        if is_active {
            imp::close_and_notify(app, label, false);
        }
    }

    #[cfg(not(target_os = "macos"))]
    let _ = (app, label);
}

/// Close the popout window outright (main window teardown). The reparented
/// webview dies with it; no restore is possible or needed.
pub fn close_popout_window(app: &AppHandle) {
    if let Some(window) = app.get_window(POPOUT_WINDOW_LABEL) {
        let _ = window.close();
    }
}

/// Install the `wisPopout` message handler on a tab webview (the script
/// itself rides the builder, see [`page_script`]). No-op off macOS.
pub fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
    #[cfg(target_os = "macos")]
    imp::install(webview);

    #[cfg(not(target_os = "macos"))]
    let _ = webview;
}

#[cfg(target_os = "macos")]
mod imp;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_validation_requires_tab_prefix() {
        assert!(validate_label("tab-3f2a1c").is_ok());
        assert!(validate_label("tab-").is_err());
        assert!(validate_label("widget-panel-x").is_err());
        assert!(validate_label("tab-../etc").is_err());
    }
}
