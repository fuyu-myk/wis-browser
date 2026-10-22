//! Tauri IPC commands exposing the browser state model to the frontend.
//!
//! Each mutating command runs through [`mutate`], which:
//!   1. Acquires the async mutex on the in-memory state.
//!   2. Snapshots the previous state for rollback.
//!   3. Computes the next state via a pure transition function from
//!      `wis-core`.
//!   4. Persists the next state to the configured `Storage`: destructive
//!      transitions (close/delete, settings) write immediately and revert
//!      the in-memory state on failure ([`WisError::PersistenceFailed`]);
//!      everything else queues a debounced "latest snapshot wins" write via
//!      [`crate::save_coalescer::SaveCoalescer`], so high-frequency bursts
//!      (drag reorders, zoom ticks, load/title sync) coalesce to one write.
//!
//! Read-only commands acquire the mutex briefly to clone the relevant slice.
//!
//! The command surface is split by domain into submodules; this module owns
//! the shared `mutate` machinery and the small read commands, and re-exports
//! every command so the handler list and cross-module call sites keep
//! addressing them as `commands::*`. Submodules pull the shared helpers and
//! the common imports below in through `use super::*`.

pub(crate) use tauri::image::Image;
pub(crate) use tauri::path::BaseDirectory;
pub(crate) use tauri::webview::{PageLoadEvent, PageLoadPayload, WebviewBuilder};
pub(crate) use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl};
pub(crate) use url::Url;
pub(crate) use wis_core::{
    AccentColor, BrowserState, PaneBranch, PrivacyMode, SplitDir, Tab, TabFolder, TabFolderId,
    TabId, ThemeMode, UpstreamProxy, UserSettings, UserSettingsPatch, WisError, WisResult,
    Workspace, WorkspaceId,
};
pub(crate) use wis_network::{Decision, OutboundRequest, RequestType};

pub(crate) use crate::state::AppState;

mod closed_tabs;
mod history;
mod settings;
mod system;
mod tab;
mod webview;
mod workspace;

pub use closed_tabs::*;
pub use history::*;
pub use settings::*;
pub use system::*;
pub use tab::*;
pub(crate) use webview::workspace_id_to_data_store_bytes;
pub use webview::*;
pub(crate) use workspace::is_chrome_url;
pub use workspace::*;

/// Tauri event name fired whenever a tab's state changed via a wry callback
/// (URL sync, title change, loading state). Chrome listens once and refetches
/// `BrowserState`: the React-Query cache is the single source of truth.
const BROWSER_STATE_CHANGED: &str = "browser-state-changed";

pub(crate) fn emit_state_changed(app: &AppHandle) {
    if let Err(err) = app.emit(BROWSER_STATE_CHANGED, ()) {
        log::warn!("emit {BROWSER_STATE_CHANGED} failed: {err}");
    }
}

// Helpers
// ---------------------------------------------------------------------------

/// Snapshot the current state. Used by every read-only command.
pub(crate) async fn read_state(state: &State<'_, AppState>) -> BrowserState {
    state.browser.lock().await.clone()
}

/// How a mutation's snapshot reaches disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Persist {
    /// Write before returning; revert the in-memory state on failure. For
    /// destructive transitions where losing the write would surprise.
    Immediate,
    /// Queue a debounced write; the latest snapshot wins. For everything
    /// else, so mutation bursts coalesce into one write.
    Coalesced,
}

/// Drive a state-mutating transition end-to-end: compute → persist → commit
/// (or revert on immediate-persist failure), then broadcast
/// `browser-state-changed` so the chrome invalidates its React-Query cache.
///
/// `transition` is a pure function from the current state to `(next_state,
/// return_value)`. It must not perform I/O.
pub(crate) async fn mutate_with<R, F>(
    app: &AppHandle,
    state: &State<'_, AppState>,
    persist: Persist,
    transition: F,
) -> WisResult<R>
where
    F: FnOnce(&BrowserState) -> WisResult<(BrowserState, R)>,
{
    let mut guard = state.browser.lock().await;
    let previous = guard.clone();
    let (next, value) = transition(&guard)?;
    let next = next.with_hidden_tabs_touched(&previous);
    *guard = next.clone();

    match persist {
        Persist::Immediate => {
            // Stamp under the state lock so sequence order matches mutation order.
            let coalescer = state.save_coalescer.clone();
            let seq = coalescer.next_seq();
            let storage = state.storage.clone();
            let result =
                tokio::task::spawn_blocking(move || coalescer.write_now(&storage, seq, &next))
                    .await
                    .map_err(|join_err| WisError::PersistenceFailed {
                        reason: format!("persistence task panicked: {join_err}"),
                    })?;

            if let Err(err) = result {
                log::error!("persisting state failed: {err}; reverting in-memory state");
                *guard = previous;

                return Err(WisError::PersistenceFailed {
                    reason: err.to_string(),
                });
            }
        }
        Persist::Coalesced => {
            state.save_coalescer.schedule(state.storage.clone(), next);
        }
    }

    #[cfg(target_os = "macos")]
    let actives = crate::extensions::changed_active_tabs(&previous, &guard);
    drop(guard);
    emit_state_changed(app);
    #[cfg(target_os = "macos")]
    if let Some(actives) = actives {
        crate::extensions::sync_active(app, actives);
    }

    Ok(value)
}

/// [`mutate_with`] using the default debounced persistence.
pub(crate) async fn mutate<R, F>(
    app: &AppHandle,
    state: &State<'_, AppState>,
    transition: F,
) -> WisResult<R>
where
    F: FnOnce(&BrowserState) -> WisResult<(BrowserState, R)>,
{
    mutate_with(app, state, Persist::Coalesced, transition).await
}

/// [`mutate_with`] for destructive transitions: immediate write + rollback.
pub(crate) async fn mutate_immediate<R, F>(
    app: &AppHandle,
    state: &State<'_, AppState>,
    transition: F,
) -> WisResult<R>
where
    F: FnOnce(&BrowserState) -> WisResult<(BrowserState, R)>,
{
    mutate_with(app, state, Persist::Immediate, transition).await
}

/// Convenience for transitions that don't have an additional return value.
pub(crate) async fn mutate_unit<F>(
    app: &AppHandle,
    state: &State<'_, AppState>,
    transition: F,
) -> WisResult<()>
where
    F: FnOnce(&BrowserState) -> WisResult<BrowserState>,
{
    mutate(app, state, |s| transition(s).map(|next| (next, ()))).await
}

/// [`mutate_unit`] with immediate (destructive-op) persistence.
pub(crate) async fn mutate_unit_immediate<F>(
    app: &AppHandle,
    state: &State<'_, AppState>,
    transition: F,
) -> WisResult<()>
where
    F: FnOnce(&BrowserState) -> WisResult<BrowserState>,
{
    mutate_immediate(app, state, |s| transition(s).map(|next| (next, ()))).await
}

/// Favicon attach helper exposed to [`crate::favicons`] so the capture
/// pipeline can persist `tab.favicon = Some(ref)` through the same
/// transaction the rest of the chrome uses.
pub(crate) async fn mutate_unit_for_favicon(
    app: &AppHandle,
    state: &State<'_, AppState>,
    tab_id: wis_core::TabId,
    favicon: Option<wis_core::FaviconRef>,
) -> WisResult<()> {
    mutate_unit(app, state, |s| s.with_tab_favicon(tab_id, favicon.clone())).await
}

/// Event broadcast after a history row changes so an open history page can
/// refresh live.
pub const HISTORY_CHANGED_EVENT: &str = "history-changed";

/// Record a finished page load in browsing history, unless the visit isn't
/// recordable: only `http(s)` pages are stored, and never from a private
/// (ephemeral) workspace. The visit is keyed to the *tab's own* workspace (read
/// from state) rather than the id captured when the webview was built, so it
/// lands in the right per-workspace bucket regardless of when the webview was
/// created. The title is read from current state (set by the document-title
/// observer during load).
pub(crate) async fn record_history_visit(
    app: &AppHandle,
    tab_id: TabId,
    fallback_workspace_id: WorkspaceId,
    url: &url::Url,
) {
    if !matches!(url.scheme(), "http" | "https") {
        return;
    }

    let state = app.state::<AppState>();
    let (workspace_id, ephemeral, title) = {
        let browser = state.browser.lock().await;
        let workspace_id = browser
            .tab(tab_id)
            .map(|t| t.workspace_id)
            .unwrap_or(fallback_workspace_id);
        let ephemeral = browser
            .workspace(workspace_id)
            .map(|w| w.ephemeral)
            .unwrap_or(true);
        let title = browser
            .tab(tab_id)
            .map(|t| t.title.clone())
            .unwrap_or_default();

        (workspace_id, ephemeral, title)
    };

    if ephemeral {
        return;
    }

    let storage = state.storage.clone();
    let url_string = url.to_string();
    match tokio::task::spawn_blocking(move || {
        storage.record_visit(workspace_id, &url_string, &title)
    })
    .await
    {
        Ok(Ok(())) => {
            let _ = app.emit(HISTORY_CHANGED_EVENT, ());
        }
        Ok(Err(err)) => log::warn!("record_visit for {tab_id} failed: {err}"),
        Err(err) => log::warn!("record_visit task for {tab_id} panicked: {err}"),
    }
}

/// Catch in-page (SPA) navigations that never fire a page-load event. Surfaced through the
/// document-title-changed hook: when the webview's live URL no longer matches
/// the tab's stored URL (and no full load is in flight), the URL is synced and
/// the visit is recorded. Skipped while `loading` so a real navigation records once,
/// through the page-load path.
pub(crate) async fn note_in_page_navigation(
    app: &AppHandle,
    tab_id: TabId,
    fallback_workspace_id: WorkspaceId,
    live_url: Url,
    title: String,
) {
    let live_url = crate::canvas::stored_url(live_url);
    let canvas = crate::canvas::is_canvas_url(&live_url);
    if !canvas && !matches!(live_url.scheme(), "http" | "https") {
        return;
    }

    let state = app.state::<AppState>();
    let (workspace_id, ephemeral, changed) = {
        let browser = state.browser.lock().await;
        let workspace_id = browser
            .tab(tab_id)
            .map(|t| t.workspace_id)
            .unwrap_or(fallback_workspace_id);
        let ephemeral = browser
            .workspace(workspace_id)
            .map(|w| w.ephemeral)
            .unwrap_or(true);
        let changed = browser
            .tab(tab_id)
            .map(|t| t.url != live_url && !t.loading)
            .unwrap_or(false);

        (workspace_id, ephemeral, changed)
    };

    if ephemeral {
        return;
    }

    if changed {
        if let Err(err) = mutate_unit(app, &state, |s| {
            s.with_tab_url_synced(tab_id, live_url.as_str())
        })
        .await
        {
            log::warn!("url sync for {tab_id} failed: {err}");
            return;
        }
        record_history_visit(app, tab_id, fallback_workspace_id, &live_url).await;
    } else if !canvas {
        let storage = state.storage.clone();
        let url_string = live_url.to_string();
        let changed = tokio::task::spawn_blocking(move || {
            storage.update_visit_title(workspace_id, &url_string, &title)
        })
        .await;

        match changed {
            Ok(Ok(())) => {
                let _ = app.emit(HISTORY_CHANGED_EVENT, ());
            }
            Ok(Err(err)) => log::warn!("update_visit_title for {tab_id} failed: {err}"),
            Err(err) => log::warn!("update_visit_title task for {tab_id} panicked: {err}"),
        }
    }
}

// ---------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------

/// The active workspace is per-window: present the calling window's, not the
/// global primary. `snapshot` is a fresh local clone, so this is a view override
/// rather than a shared mutation.
///
/// Self-heals: an unbound window, or one whose workspace was deleted out from
/// under it, keeps the persisted primary (which `with_workspace_removed` already
/// reassigned to a live one).
#[tauri::command]
pub async fn get_browser_state(
    window: tauri::Window,
    state: State<'_, AppState>,
) -> Result<BrowserState, WisError> {
    let mut snapshot = read_state(&state).await;
    if let Some(ws) = state.window_workspace(window.label()) {
        if snapshot.workspaces.iter().any(|w| w.id == ws) {
            snapshot.active_workspace_id = ws;
        }
    }

    Ok(snapshot)
}

#[tauri::command]
pub async fn get_workspace(state: State<'_, AppState>, id: WorkspaceId) -> WisResult<Workspace> {
    read_state(&state).await.workspace(id).cloned()
}

#[tauri::command]
pub async fn get_tab(state: State<'_, AppState>, id: TabId) -> WisResult<Tab> {
    read_state(&state).await.tab(id).cloned()
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<UserSettings, WisError> {
    Ok(read_state(&state).await.settings)
}

// ---------------------------------------------------------------------------
// Workspace mutations
// ---------------------------------------------------------------------------

/// Apply a page-zoom factor to a tab's live webview (no-op if it has no native
/// webview yet, e.g. a chrome page). Shared by [`tab_zoom`] and tab-webview
/// creation so a restored tab regains its zoom.
pub(crate) fn apply_tab_zoom(app: &AppHandle, id: TabId, factor: f64) {
    let label = format!("tab-{}", id.as_uuid());
    let Some(webview) = app.webviews().get(&label).cloned() else {
        return;
    };
    apply_zoom_to_webview(&webview, factor);
}

/// Push a zoom factor into one live webview.
pub(crate) fn apply_zoom_to_webview<R: tauri::Runtime>(
    webview: &tauri::webview::Webview<R>,
    factor: f64,
) {
    #[cfg(target_os = "macos")]
    {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        let _ = webview.with_webview(move |w| unsafe {
            let wk: *mut AnyObject = w.inner() as *mut AnyObject;
            if !wk.is_null() {
                let _: () = msg_send![wk, setPageZoom: factor];
            }
        });
    }

    // The controller's `ZoomFactor` is the user-facing (Ctrl +/-) page zoom,
    // the analog of WKWebView's `setPageZoom:`.
    #[cfg(windows)]
    crate::win_webview2::with_controller(webview, move |controller| {
        // SAFETY: runs on the UI thread via `with_controller`.
        let _ = unsafe { controller.SetZoomFactor(factor) };
    });

    // WebKitGTK's zoom-level is the same user-facing page zoom.
    #[cfg(target_os = "linux")]
    {
        use webkit2gtk::WebViewExt;
        let _ = webview.with_webview(move |w| {
            w.inner().set_zoom_level(factor);
        });
    }

    #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
    {
        let _ = (webview, factor);
    }
}

/// Force a tab webview's `prefers-color-scheme` to match the browser theme by
/// overriding its native appearance. `System` clears the override so the page
/// follows the OS again. No-op off macOS.
pub(crate) fn apply_tab_appearance<R: tauri::Runtime>(
    webview: &tauri::webview::Webview<R>,
    theme: ThemeMode,
) {
    #[cfg(target_os = "macos")]
    {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};

        let _ = webview.with_webview(move |w| unsafe {
            let view: *mut AnyObject = w.inner() as *mut AnyObject;
            if view.is_null() {
                return;
            }
            let name: Option<&str> = match theme {
                ThemeMode::Dark => Some("NSAppearanceNameDarkAqua"),
                ThemeMode::Light => Some("NSAppearanceNameAqua"),
                ThemeMode::System => None,
            };
            let appearance: *mut AnyObject = match name {
                Some(n) => {
                    let Ok(c) = std::ffi::CString::new(n) else {
                        return;
                    };
                    let ns: *mut AnyObject =
                        msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()];
                    msg_send![class!(NSAppearance), appearanceNamed: ns]
                }
                None => std::ptr::null_mut(),
            };
            let _: () = msg_send![view, setAppearance: appearance];
        });
    }

    // WebView2's preferred color scheme lives on the profile. `Auto` follows the OS.
    #[cfg(windows)]
    crate::win_webview2::with_core_webview2(webview, move |core| {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            ICoreWebView2_13, COREWEBVIEW2_PREFERRED_COLOR_SCHEME_AUTO,
            COREWEBVIEW2_PREFERRED_COLOR_SCHEME_DARK, COREWEBVIEW2_PREFERRED_COLOR_SCHEME_LIGHT,
        };
        use windows::core::Interface;

        let scheme = match theme {
            ThemeMode::Dark => COREWEBVIEW2_PREFERRED_COLOR_SCHEME_DARK,
            ThemeMode::Light => COREWEBVIEW2_PREFERRED_COLOR_SCHEME_LIGHT,
            ThemeMode::System => COREWEBVIEW2_PREFERRED_COLOR_SCHEME_AUTO,
        };
        if let Ok(c13) = core.cast::<ICoreWebView2_13>() {
            if let Ok(profile) = unsafe { c13.Profile() } {
                // SAFETY: runs on the UI thread via `with_core_webview2`.
                let _ = unsafe { profile.SetPreferredColorScheme(scheme) };
            }
        }
    });

    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = (webview, theme);
    }
}

/// Re-apply the theme appearance to every live tab webview (on a theme change).
pub(crate) fn apply_appearance_to_all_tabs(app: &AppHandle, theme: ThemeMode) {
    for (label, webview) in app.webviews() {
        if label.starts_with("tab-") && !crate::canvas::is_canvas_tab(&label) {
            apply_tab_appearance(&webview, theme);
        }
    }
}
