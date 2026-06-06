//! Tauri IPC commands exposing the browser state model to the frontend.
//!
//! Each mutating command runs through [`mutate`], which:
//!   1. Acquires the async mutex on the in-memory state.
//!   2. Snapshots the previous state for rollback.
//!   3. Computes the next state via a pure transition function from
//!      `wis-core`.
//!   4. Persists the next state to the configured `Storage`.
//!   5. On persist failure: reverts the in-memory state and surfaces a
//!      [`WisError::PersistenceFailed`].
//!
//! Read-only commands acquire the mutex briefly to clone the relevant slice.

use tauri::image::Image;
use tauri::path::BaseDirectory;
use tauri::webview::{PageLoadEvent, PageLoadPayload, WebviewBuilder};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl};
use url::Url;
use wis_core::{
    AccentColor, BrowserState, PaneBranch, PrivacyMode, SplitDir, Tab, TabId, UpstreamProxy,
    UserSettings, UserSettingsPatch, WisError, WisResult, Workspace, WorkspaceId,
};
use wis_network::{Decision, OutboundRequest, RequestType};

use crate::state::AppState;

/// Tauri event name fired whenever a tab's state changed via a wry callback
/// (URL sync, title change, loading state). Chrome listens once and refetches
/// `BrowserState`: the React-Query cache is the single source of truth.
const BROWSER_STATE_CHANGED: &str = "browser-state-changed";

fn emit_state_changed(app: &AppHandle) {
    if let Err(err) = app.emit(BROWSER_STATE_CHANGED, ()) {
        log::warn!("emit {BROWSER_STATE_CHANGED} failed: {err}");
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Snapshot the current state. Used by every read-only command.
async fn read_state(state: &State<'_, AppState>) -> BrowserState {
    state.browser.lock().await.clone()
}

/// Drive a state-mutating transition end-to-end: compute → persist → commit
/// (or revert on persist failure), then broadcast `browser-state-changed`
/// invalidates its React-Query cache.
///
/// `transition` is a pure function from the current state to `(next_state,
/// return_value)`. It must not perform I/O.
async fn mutate<R, F>(app: &AppHandle, state: &State<'_, AppState>, transition: F) -> WisResult<R>
where
    F: FnOnce(&BrowserState) -> WisResult<(BrowserState, R)>,
{
    let mut guard = state.browser.lock().await;
    let previous = guard.clone();
    let (next, value) = transition(&guard)?;
    *guard = next.clone();

    let storage = state.storage.clone();
    let persist = tokio::task::spawn_blocking(move || storage.save_state(&next))
        .await
        .map_err(|join_err| WisError::PersistenceFailed {
            reason: format!("persistence task panicked: {join_err}"),
        })?;

    if let Err(err) = persist {
        log::error!("persisting state failed: {err}; reverting in-memory state");
        *guard = previous;

        return Err(WisError::PersistenceFailed {
            reason: err.to_string(),
        });
    }

    drop(guard);
    emit_state_changed(app);

    Ok(value)
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
    if !matches!(live_url.scheme(), "http" | "https") {
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
    } else {
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

#[tauri::command]
pub async fn get_history(
    state: State<'_, AppState>,
    query: wis_storage::HistoryQuery,
) -> WisResult<Vec<wis_storage::HistoryEntry>> {
    let storage = state.storage.clone();
    run_history(move || storage.query_history(&query)).await
}

#[tauri::command]
pub async fn delete_history(state: State<'_, AppState>, ids: Vec<i64>) -> WisResult<()> {
    let storage = state.storage.clone();
    run_history(move || storage.delete_history_entries(&ids)).await
}

#[tauri::command]
pub async fn clear_history(
    state: State<'_, AppState>,
    workspace_id: Option<WorkspaceId>,
) -> WisResult<()> {
    let storage = state.storage.clone();
    run_history(move || storage.clear_history(workspace_id)).await
}

/// Run a blocking storage call off the async runtime, mapping both the join
/// failure and the storage error onto [`WisError::PersistenceFailed`].
async fn run_history<T, F>(op: F) -> WisResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> wis_storage::StorageResult<T> + Send + 'static,
{
    tokio::task::spawn_blocking(op)
        .await
        .map_err(|err| WisError::PersistenceFailed {
            reason: format!("history task panicked: {err}"),
        })?
        .map_err(|err| WisError::PersistenceFailed {
            reason: err.to_string(),
        })
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_browser_state(state: State<'_, AppState>) -> Result<BrowserState, WisError> {
    Ok(read_state(&state).await)
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

#[tauri::command]
pub async fn create_workspace(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    accent: AccentColor,
) -> WisResult<Workspace> {
    mutate(&app, &state, |s| Ok(s.with_new_workspace(name, accent))).await
}

/// Create a private (ephemeral) workspace. Its tabs use a non-persistent
/// WebView data store and the workspace is never written to disk.
#[tauri::command]
pub async fn create_private_workspace(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    accent: AccentColor,
) -> WisResult<Workspace> {
    mutate(&app, &state, |s| {
        Ok(s.with_new_private_workspace(name, accent))
    })
    .await
}

#[tauri::command]
pub async fn rename_workspace(
    app: AppHandle,
    state: State<'_, AppState>,
    id: WorkspaceId,
    name: String,
) -> WisResult<Workspace> {
    mutate(&app, &state, |s| {
        let next = s.with_renamed_workspace(id, name)?;
        let renamed = next.workspace(id)?.clone();

        Ok((next, renamed))
    })
    .await
}

#[tauri::command]
pub async fn delete_workspace(
    app: AppHandle,
    state: State<'_, AppState>,
    id: WorkspaceId,
) -> WisResult<()> {
    mutate_unit(&app, &state, |s| s.with_workspace_removed(id)).await
}

#[tauri::command]
pub async fn recolor_workspace(
    app: AppHandle,
    state: State<'_, AppState>,
    id: WorkspaceId,
    accent: AccentColor,
) -> WisResult<Workspace> {
    mutate(&app, &state, |s| {
        let next = s.with_recoloured_workspace(id, accent)?;
        let recoloured = next.workspace(id)?.clone();

        Ok((next, recoloured))
    })
    .await
}

#[tauri::command]
pub async fn set_active_workspace(
    app: AppHandle,
    state: State<'_, AppState>,
    id: WorkspaceId,
) -> WisResult<()> {
    mutate_unit(&app, &state, |s| s.with_active_workspace(id)).await
}

/// Change the privacy mode for a specific workspace. The workspace proxy is
/// dropped so the next navigation rebuilds it with the new policy, and the
/// engine-level rule list on every open tab in the workspace is re-applied.
#[tauri::command]
pub async fn set_workspace_privacy_mode(
    app: AppHandle,
    state: State<'_, AppState>,
    id: WorkspaceId,
    mode: PrivacyMode,
) -> WisResult<Workspace> {
    let workspace = mutate(&app, &state, |s| {
        let next = s.with_workspace_privacy_mode(id, mode)?;
        let updated = next.workspace(id)?.clone();
        Ok((next, updated))
    })
    .await?;

    state.update_proxy_policy(id).await;

    let spec = state.content_rule_spec(mode);
    let tab_labels: Vec<String> = {
        let browser = state.browser.lock().await;
        browser
            .tabs_for(id)
            .map(|tabs| {
                tabs.iter()
                    .map(|tab| format!("tab-{}", tab.id.as_uuid()))
                    .collect()
            })
            .unwrap_or_default()
    };
    let total = tab_labels.len();
    let mut reattached = 0usize;
    for label in tab_labels {
        let Some(webview) = app.webviews().get(&label).cloned() else {
            continue;
        };
        let _ = crate::content_filter::clear_rule_lists(&webview);
        if let Some(spec) = &spec {
            match crate::content_filter::attach_rule_list(&webview, spec) {
                Ok(()) => reattached += 1,
                Err(err) => log::warn!("content filter: re-attach for {label} skipped: {err}"),
            }
        }
    }
    log::info!("workspace {id} privacy mode now {mode:?}: re-attached {reattached}/{total} tabs");

    Ok(workspace)
}

/// Set (or clear) a workspace's upstream proxy. The running proxy's policy is
/// updated in place so open tabs route through the new upstream on their next
/// request without losing their connection.
#[tauri::command]
pub async fn set_workspace_upstream(
    app: AppHandle,
    state: State<'_, AppState>,
    id: WorkspaceId,
    upstream: Option<UpstreamProxy>,
) -> WisResult<Workspace> {
    let workspace = mutate(&app, &state, |s| {
        let next = s.with_workspace_upstream_proxy(id, upstream.clone())?;
        let updated = next.workspace(id)?.clone();
        Ok((next, updated))
    })
    .await?;

    state.update_proxy_policy(id).await;

    let tab_labels: Vec<String> = {
        let browser = state.browser.lock().await;
        browser
            .tabs_for(id)
            .map(|tabs| {
                tabs.iter()
                    .filter(|tab| !crate::commands::is_chrome_url(tab.url.as_str()))
                    .map(|tab| format!("tab-{}", tab.id.as_uuid()))
                    .collect()
            })
            .unwrap_or_default()
    };
    for label in tab_labels {
        if let Some(webview) = app.webviews().get(&label).cloned() {
            let _ = webview.eval("window.location.reload()");
        }
    }

    log::info!("workspace {id} upstream proxy updated");

    Ok(workspace)
}

/// Whether a URL is a chrome page (no native webview / not reloadable as a site).
pub(crate) fn is_chrome_url(url: &str) -> bool {
    url.starts_with("about:")
}

/// Probe an upstream proxy for reachability (TCP connect, plus a SOCKS5
/// handshake) so Settings can show a connection status. Does not change any
/// workspace; returns `Ok(())` when reachable, or an error message. Built-in
/// Tor is "probed" by bootstrapping the embedded client.
#[tauri::command]
pub async fn test_upstream(state: State<'_, AppState>, upstream: UpstreamProxy) -> WisResult<()> {
    match upstream {
        UpstreamProxy::Tor => {
            state
                .ensure_tor()
                .await
                .map(|_| ())
                .map_err(|e| WisError::InvalidInput {
                    message: e.to_string(),
                })
        }
        other => wis_network::probe_upstream(&other)
            .await
            .map_err(|reason| WisError::InvalidInput { message: reason }),
    }
}

/// Whether the built-in Tor client is compiled into this build.
#[tauri::command]
pub fn tor_available() -> bool {
    cfg!(feature = "tor")
}

// ---------------------------------------------------------------------------
// Tab mutations
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn create_tab(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    url: String,
) -> WisResult<Tab> {
    mutate(&app, &state, |s| s.with_new_tab(workspace_id, &url)).await
}

/// Split the workspace's focused pane along `dir`, opening `url` in the new
/// pane and focusing it. Returns the newly created tab. The frontend's webview
/// reconciler then spins up its native webview and tiles it into the pane.
#[tauri::command]
pub async fn split_pane(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    dir: SplitDir,
    place_new_first: bool,
    url: String,
) -> WisResult<Tab> {
    mutate(&app, &state, |s| {
        s.with_pane_split(workspace_id, dir, place_new_first, &url)
    })
    .await
}

/// Replace a pane's tab with an already-open `moved_tab` (drag to pane centre).
/// A throwaway placeholder in that pane is closed.
#[tauri::command]
pub async fn replace_pane_leaf(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    target_tab: TabId,
    moved_tab: TabId,
) -> WisResult<()> {
    mutate_unit(&app, &state, |s| {
        s.with_pane_leaf_replaced(workspace_id, target_tab, moved_tab)
    })
    .await
}

/// Split the `target_tab` pane along `dir` with the already-open `moved_tab`
/// (dragged from the tab list onto a pane). `place_new_first` puts the moved
/// tab in the left/top slot.
#[tauri::command]
pub async fn split_pane_with(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    target_tab: TabId,
    dir: SplitDir,
    place_new_first: bool,
    moved_tab: TabId,
) -> WisResult<()> {
    mutate_unit(&app, &state, |s| {
        s.with_pane_split_with(workspace_id, target_tab, dir, place_new_first, moved_tab)
    })
    .await
}

/// Collapse all splits in a workspace back to a single pane (the focused tab).
/// Other panes' tabs remain open as background tabs.
#[tauri::command]
pub async fn unsplit_panes(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
) -> WisResult<()> {
    mutate_unit(&app, &state, |s| s.with_panes_unsplit(workspace_id)).await
}

/// Set the divider ratio (permille, 0..=1000) of the split reached by `path`
/// from the workspace's pane-tree root.
#[tauri::command]
pub async fn set_pane_ratio(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    path: Vec<PaneBranch>,
    ratio: u16,
) -> WisResult<()> {
    mutate_unit(&app, &state, |s| {
        s.with_pane_ratio(workspace_id, &path, ratio)
    })
    .await
}

#[tauri::command]
pub async fn close_tab(app: AppHandle, state: State<'_, AppState>, id: TabId) -> WisResult<()> {
    /// Newest-first cap on the reopen-closed-tab ring.
    const CLOSED_RING_CAP: usize = 25;

    // Record the tab for "reopen closed tab" before it's removed.
    {
        let browser = state.browser.lock().await;
        if let Ok(tab) = browser.tab(id) {
            let url = tab.url.to_string();
            let ephemeral = browser
                .workspace(tab.workspace_id)
                .map(|w| w.ephemeral)
                .unwrap_or(true);
            if !ephemeral && !url.starts_with("about:") {
                let entry = crate::state::ClosedTab {
                    workspace_id: tab.workspace_id,
                    url,
                };

                drop(browser);
                let mut ring = state.closed_tabs.lock().await;
                ring.push(entry);

                let overflow = ring.len().saturating_sub(CLOSED_RING_CAP);
                if overflow > 0 {
                    ring.drain(0..overflow);
                }
            }
        }
    }

    mutate_unit(&app, &state, |s| s.with_tab_closed(id)).await
}

/// Reopen the most-recently-closed tab in its original workspace, activating it.
/// No-op when the ring is empty or that workspace no longer exists.
#[tauri::command]
pub async fn restore_closed_tab(app: AppHandle, state: State<'_, AppState>) -> WisResult<()> {
    loop {
        let entry = {
            let mut ring = state.closed_tabs.lock().await;
            ring.pop()
        };
        let Some(entry) = entry else {
            return Ok(());
        };

        let exists = {
            let browser = state.browser.lock().await;
            browser.workspace(entry.workspace_id).is_ok()
        };
        if !exists {
            continue;
        }

        return mutate_unit(&app, &state, |s| {
            let (next, _tab) = s.with_new_tab(entry.workspace_id, &entry.url)?;
            next.with_active_workspace(entry.workspace_id)
        })
        .await;
    }
}

/// Reload a tab bypassing the HTTP cache (WKWebView `reloadFromOrigin`).
#[tauri::command]
pub async fn tab_hard_reload(app: AppHandle, id: TabId) -> WisResult<()> {
    let label = format!("tab-{}", id.as_uuid());
    let Some(webview) = app.webviews().get(&label).cloned() else {
        return Ok(());
    };

    #[cfg(target_os = "macos")]
    {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        webview
            .with_webview(|w| unsafe {
                let wk: *mut AnyObject = w.inner() as *mut AnyObject;
                if !wk.is_null() {
                    let _: *mut AnyObject = msg_send![wk, reloadFromOrigin];
                }
            })
            .map_err(|err| WisError::InvalidInput {
                message: format!("hard reload failed: {err}"),
            })?;
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = webview.eval("window.location.reload()");
    }

    Ok(())
}

/// Adjust a tab's page zoom. `kind` is `"in"`, `"out"`, or `"reset"`.
/// Read-modify-write of WKWebView `pageZoom` (clamped 0.5–3.0).
#[tauri::command]
pub async fn tab_zoom(app: AppHandle, id: TabId, kind: String) -> WisResult<()> {
    let label = format!("tab-{}", id.as_uuid());
    let Some(webview) = app.webviews().get(&label).cloned() else {
        return Ok(());
    };

    #[cfg(target_os = "macos")]
    {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        webview
            .with_webview(move |w| unsafe {
                let wk: *mut AnyObject = w.inner() as *mut AnyObject;
                if wk.is_null() {
                    return;
                }
                let current: f64 = msg_send![wk, pageZoom];
                let next = match kind.as_str() {
                    "in" => (current * 1.1).min(3.0),
                    "out" => (current / 1.1).max(0.5),
                    _ => 1.0,
                };
                let _: () = msg_send![wk, setPageZoom: next];
            })
            .map_err(|err| WisError::InvalidInput {
                message: format!("zoom failed: {err}"),
            })?;
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (webview, kind);
    }

    Ok(())
}

#[tauri::command]
pub async fn reorder_tabs(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    order: Vec<TabId>,
) -> WisResult<()> {
    mutate_unit(&app, &state, |s| s.with_tab_order(workspace_id, order)).await
}

#[tauri::command]
pub async fn pin_tab(
    app: AppHandle,
    state: State<'_, AppState>,
    id: TabId,
    pinned: bool,
) -> WisResult<Tab> {
    mutate(&app, &state, |s| s.with_tab_pinned(id, pinned)).await
}

#[tauri::command]
pub async fn set_active_tab(
    app: AppHandle,
    state: State<'_, AppState>,
    id: TabId,
) -> WisResult<()> {
    mutate_unit(&app, &state, |s| s.with_active_tab(id)).await
}

/// Navigate an existing tab's child webview to `url`, and persist the new URL
/// in state. If the webview hasn't been created yet (the frontend reconciler
/// will pick it up on the next render), only state is updated: the URL takes
/// effect when the webview comes into being.
#[tauri::command]
pub async fn navigate_tab(
    app: AppHandle,
    state: State<'_, AppState>,
    id: TabId,
    url: String,
) -> WisResult<()> {
    // Update in-memory + persisted state.
    mutate_unit(&app, &state, |s| {
        s.with_navigated_tab(id, &url).map(|(next, _)| next)
    })
    .await?;

    // Drive the actual native webview navigation.
    let label = format!("tab-{}", id.as_uuid());
    if let Some(webview) = app.webviews().get(&label).cloned() {
        let parsed = Url::parse(&url).map_err(|_| WisError::InvalidUrl { url: url.clone() })?;
        webview
            .navigate(parsed)
            .map_err(|err| WisError::InvalidInput {
                message: format!("webview navigate failed: {err}"),
            })?;
    }

    Ok(())
}

/// Create a Tauri child webview for a tab using `WebviewBuilder` directly.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn create_tab_webview(
    app: AppHandle,
    state: State<'_, AppState>,
    window_label: String,
    label: String,
    url: String,
    workspace_id: WorkspaceId,
    tab_id: TabId,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> WisResult<()> {
    let window = app
        .get_window(&window_label)
        .ok_or_else(|| WisError::InvalidInput {
            message: format!("no window labelled {window_label}"),
        })?;

    let parsed = Url::parse(&url).map_err(|_| WisError::InvalidUrl { url: url.clone() })?;

    let label_ident = label.clone();

    let ephemeral = {
        let browser = state.browser.lock().await;
        browser
            .workspace(workspace_id)
            .map(|w| w.ephemeral)
            .unwrap_or(false)
    };

    let proxy_port = state
        .ensure_proxy_for(workspace_id)
        .await
        .map_err(wis_core::WisError::from)?;
    let proxy_url = format!("http://127.0.0.1:{proxy_port}")
        .parse::<Url>()
        .expect("loopback proxy URL is valid");

    let mut builder = WebviewBuilder::new(label, WebviewUrl::External(parsed))
        .focused(false)
        .proxy_url(proxy_url);
    if ephemeral {
        builder = builder.incognito(true);
    } else {
        builder = builder.data_store_identifier(workspace_id_to_data_store_bytes(workspace_id));
    }

    // Document title changes: fires when the page sets `<title>` or
    // mutates `document.title` from JS.
    {
        let app = app.clone();
        builder = builder.on_document_title_changed(move |webview, title: String| {
            let app = app.clone();
            let live_url = webview.url().ok();
            tauri::async_runtime::spawn(async move {
                let state = app.state::<AppState>();
                let result =
                    mutate_unit(&app, &state, |s| s.with_tab_title(tab_id, title.clone())).await;
                if let Err(err) = result {
                    log::warn!("title update for {tab_id} failed: {err}");
                }
                if let Some(url) = live_url {
                    note_in_page_navigation(&app, tab_id, workspace_id, url, title).await;
                }
            });
        });
    }

    {
        let app = app.clone();
        builder = builder.on_page_load(move |_webview, payload: PageLoadPayload<'_>| {
            let event = payload.event();
            let loading = matches!(event, PageLoadEvent::Started);
            let url_string = payload.url().to_string();
            let page_url = payload.url().clone();

            let app_for_state = app.clone();
            tauri::async_runtime::spawn(async move {
                let state = app_for_state.state::<AppState>();
                let result = mutate_unit(&app_for_state, &state, |s| {
                    s.with_tab_loading(tab_id, loading)
                        .and_then(|next| next.with_tab_url_synced(tab_id, &url_string))
                })
                .await;
                if let Err(err) = result {
                    log::warn!("page load update for {tab_id} failed: {err}");
                }
            });

            if matches!(event, PageLoadEvent::Finished) {
                let app_for_favicon = app.clone();
                let page_url_for_history = page_url.clone();
                tauri::async_runtime::spawn(async move {
                    crate::favicons::capture_favicon(
                        app_for_favicon,
                        tab_id,
                        workspace_id,
                        page_url,
                    )
                    .await;
                });

                let app_for_history = app.clone();
                tauri::async_runtime::spawn(async move {
                    record_history_visit(
                        &app_for_history,
                        tab_id,
                        workspace_id,
                        &page_url_for_history,
                    )
                    .await;
                });
            }
        });
    }

    let child = window
        .add_child(
            builder,
            tauri::LogicalPosition::new(x, y),
            tauri::LogicalSize::new(width, height),
        )
        .map_err(|err| WisError::InvalidInput {
            message: format!("add_child failed: {err}"),
        })?;

    // Move every overlay back to the top of the subview stack.
    crate::overlay::promote_overlays_to_front(&app);

    // Stamp the native WKWebView so the pane-focus mouse monitor can identify
    // which tab a click landed in (see `pane_focus`).
    #[cfg(target_os = "macos")]
    {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        let ident = label_ident.clone();
        let _ = child.with_webview(move |w| unsafe {
            let wk: *mut AnyObject = w.inner() as *mut AnyObject;
            if wk.is_null() {
                return;
            }
            let Ok(c) = std::ffi::CString::new(ident.as_str()) else {
                return;
            };
            let ns: *mut AnyObject =
                msg_send![objc2::class!(NSString), stringWithUTF8String: c.as_ptr()];
            let _: () = msg_send![wk, setAccessibilityIdentifier: ns];
        });
    }
    #[cfg(not(target_os = "macos"))]
    let _ = label_ident;

    let privacy_mode = {
        let browser = state.browser.lock().await;
        browser
            .workspace(workspace_id)
            .map(|w| w.privacy_mode)
            .unwrap_or_default()
    };

    // Engine-level filtering is best-effort: a failure must not block the tab.
    if let Some(spec) = state.content_rule_spec(privacy_mode) {
        if let Err(err) = crate::content_filter::attach_rule_list(&child, &spec) {
            log::warn!("content filter: attach skipped: {err}");
        }
    }

    // "Save password?" capture.
    if !ephemeral {
        crate::capture::install(&child);
    }

    Ok(())
}

fn workspace_id_to_data_store_bytes(workspace_id: WorkspaceId) -> [u8; 16] {
    *workspace_id.as_uuid().as_bytes()
}

/// Set a tab webview's position and size in a single IPC round-trip, applying
/// both atomically. Resolving the webview is best-effort: a not-yet-created webview is
/// a silent no-op.
#[tauri::command]
pub async fn set_tab_webview_bounds(
    app: AppHandle,
    label: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> WisResult<()> {
    let Some(webview) = app.webviews().get(&label).cloned() else {
        return Ok(());
    };
    webview
        .set_position(tauri::LogicalPosition::new(x, y))
        .map_err(|err| WisError::InvalidInput {
            message: format!("set webview position failed: {err}"),
        })?;
    webview
        .set_size(tauri::LogicalSize::new(width, height))
        .map_err(|err| WisError::InvalidInput {
            message: format!("set webview size failed: {err}"),
        })?;

    Ok(())
}

/// Read the current URL of a tab's native webview. Returns `None` if the
/// webview hasn't been created yet (chrome tab or not yet reconciled).
#[tauri::command]
pub async fn webview_url(app: AppHandle, label: String) -> WisResult<Option<String>> {
    let Some(webview) = app.webviews().get(&label).cloned() else {
        return Ok(None);
    };

    match webview.url() {
        Ok(url) => Ok(Some(url.to_string())),
        Err(err) => Err(WisError::InvalidInput {
            message: format!("read webview url failed: {err}"),
        }),
    }
}

/// Sync a tab's stored URL to match where the user actually navigated inside
/// the webview (in-page link clicks, history hops). Unlike `navigate_tab`
/// this does not call `webview.navigate(...)`.
#[tauri::command]
pub async fn sync_tab_url(
    app: AppHandle,
    state: State<'_, AppState>,
    id: TabId,
    url: String,
) -> WisResult<()> {
    mutate_unit(&app, &state, |s| s.with_tab_url_synced(id, &url)).await
}

// ---------------------------------------------------------------------------
// Navigation event bridge IPC + nav controls
// ---------------------------------------------------------------------------

/// Set the tab's title from a programmatic source (e.g. a chrome-injected
/// rename).
#[tauri::command]
pub async fn update_tab_title(
    app: AppHandle,
    state: State<'_, AppState>,
    id: TabId,
    title: String,
) -> WisResult<()> {
    mutate_unit(&app, &state, |s| s.with_tab_title(id, title)).await
}

/// Set a tab's `loading` flag explicitly.
#[tauri::command]
pub async fn update_tab_loading(
    app: AppHandle,
    state: State<'_, AppState>,
    id: TabId,
    loading: bool,
) -> WisResult<()> {
    mutate_unit(&app, &state, |s| s.with_tab_loading(id, loading)).await
}

/// Reload a tab's webview.
#[tauri::command]
pub async fn tab_reload(app: AppHandle, id: TabId) -> WisResult<()> {
    let label = format!("tab-{}", id.as_uuid());
    let webview = app
        .webviews()
        .get(&label)
        .cloned()
        .ok_or_else(|| WisError::InvalidInput {
            message: format!("no webview labelled {label}"),
        })?;
    log::info!("tab_reload: {label}");
    webview
        .eval("window.location.reload()")
        .map_err(|err| WisError::InvalidInput {
            message: format!("reload eval failed: {err}"),
        })
}

/// Navigate the tab back in its history.
#[tauri::command]
pub async fn tab_go_back(app: AppHandle, id: TabId) -> WisResult<()> {
    eval_history_step(&app, id, "back")
}

/// Counterpart to [`tab_go_back`].
#[tauri::command]
pub async fn tab_go_forward(app: AppHandle, id: TabId) -> WisResult<()> {
    eval_history_step(&app, id, "forward")
}

fn eval_history_step(app: &AppHandle, id: TabId, direction: &str) -> WisResult<()> {
    let label = format!("tab-{}", id.as_uuid());
    let webview = app
        .webviews()
        .get(&label)
        .cloned()
        .ok_or_else(|| WisError::InvalidInput {
            message: format!("no webview labelled {label}"),
        })?;

    webview
        .eval(format!("window.history.{direction}()"))
        .map_err(|err| WisError::InvalidInput {
            message: format!("history.{direction}() eval failed: {err}"),
        })
}

/// Apply a CSS-style corner radius to a tab's native webview surface, macOS only.
#[tauri::command]
pub async fn set_webview_corner_radius(
    app: AppHandle,
    label: String,
    radius: f64,
) -> WisResult<()> {
    let webview = app
        .webviews()
        .get(&label)
        .cloned()
        .ok_or_else(|| WisError::InvalidInput {
            message: format!("no webview labelled {label}"),
        })?;

    #[cfg(target_os = "macos")]
    {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;

        webview
            .with_webview(move |w| unsafe {
                let wkwebview: *mut AnyObject = w.inner() as *mut AnyObject;
                if wkwebview.is_null() {
                    return;
                }

                // Force the view to be layer-backed.
                let _: () = msg_send![wkwebview, setWantsLayer: true];

                let layer: *mut AnyObject = msg_send![wkwebview, layer];
                if layer.is_null() {
                    return;
                }

                // Round + clip.
                let _: () = msg_send![layer, setCornerRadius: radius];
                let _: () = msg_send![layer, setMasksToBounds: true];
            })
            .map_err(|err| WisError::InvalidInput {
                message: format!("with_webview failed: {err}"),
            })?;
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = webview;
        let _ = radius;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    patch: UserSettingsPatch,
) -> WisResult<UserSettings> {
    let keybindings_touched = patch.keybindings.is_some();
    let settings = mutate(&app, &state, |s| {
        let next = s.with_settings(patch);
        let settings = next.settings.clone();

        Ok((next, settings))
    })
    .await?;

    if keybindings_touched {
        let effective = wis_core::effective_keymap(&settings.keybindings);
        if let Err(err) = crate::menu::apply_menu(&app, &effective) {
            log::warn!("menu rebuild after keybindings change failed: {err}");
        }
    }

    Ok(settings)
}

// ---------------------------------------------------------------------------
// Privacy decision engine (read-only inspector)
// ---------------------------------------------------------------------------

/// Ask the privacy engine what it would do with the given request, without
/// actually sending it.
///
/// The active workspace's `PrivacyMode` drives the decision; per-site
/// overrides will be plumbed through once they're modelled in `BrowserState`.
#[tauri::command]
pub async fn check_url_policy(
    state: State<'_, AppState>,
    url: String,
    source_url: String,
    request_type: RequestType,
) -> WisResult<Decision> {
    let parsed_url = Url::parse(&url).map_err(|_| WisError::InvalidUrl { url })?;
    let parsed_source =
        Url::parse(&source_url).map_err(|_| WisError::InvalidUrl { url: source_url })?;

    let policy = state.request_policy_for_active_workspace().await;
    policy
        .decide(&OutboundRequest {
            url: &parsed_url,
            source_url: &parsed_source,
            request_type,
        })
        .map_err(|err| WisError::InvalidInput {
            message: err.to_string(),
        })
}

/// Resolve `hostname` via the configured DoH resolver and return the
/// addresses as strings.
#[tauri::command]
pub async fn resolve_hostname(
    state: State<'_, AppState>,
    hostname: String,
) -> WisResult<Vec<String>> {
    let resolver = state.resolver.clone();
    resolver
        .resolve(&hostname)
        .await
        .map(|ips| ips.into_iter().map(|ip| ip.to_string()).collect())
        .map_err(|err| WisError::InvalidInput {
            message: err.to_string(),
        })
}

// ---------------------------------------------------------------------------
// Runtime window/dock icon swap
// ---------------------------------------------------------------------------

/// Variant of the wis owl shown as the window/dock icon. `"closed"` for
/// private/reader contexts; `"open"` (or anything else) for the default state.
/// Frontend hovers and private-mode toggles invoke this to keep the OS-level
/// brand surface in sync with the in-app state.
#[tauri::command]
pub async fn set_window_icon(
    app: AppHandle,
    window_label: String,
    variant: String,
) -> WisResult<()> {
    let file = match variant.as_str() {
        "closed" => "icons/runtime/mark-closed-512.png",
        _ => "icons/runtime/mark-open-512.png",
    };
    let path = app
        .path()
        .resolve(file, BaseDirectory::Resource)
        .map_err(|err| WisError::InvalidInput {
            message: format!("resolve icon {file}: {err}"),
        })?;
    let icon = Image::from_path(&path).map_err(|err| WisError::InvalidInput {
        message: format!("load icon {file}: {err}"),
    })?;
    let window = app
        .get_webview_window(&window_label)
        .ok_or_else(|| WisError::InvalidInput {
            message: format!("no window labelled {window_label}"),
        })?;

    window.set_icon(icon).map_err(|err| WisError::InvalidInput {
        message: format!("set_icon failed: {err}"),
    })
}

/// Set the macOS **Dock** icon (the application icon, distinct from the window
/// icon `set_window_icon` touches). `"closed"` overrides it with the
/// closed-eye owl for private browsing; any other value clears the override so
/// the system restores the bundled app icon (which keeps its Liquid Glass
/// treatment on macOS 26+). No-op on non-macOS platforms.
#[tauri::command]
pub async fn set_dock_icon(app: AppHandle, variant: String) -> WisResult<()> {
    #[cfg(target_os = "macos")]
    {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        use std::ffi::CString;

        let image: *mut AnyObject = if variant == "closed" {
            let path = app
                .path()
                .resolve("icons/runtime/mark-closed-512.png", BaseDirectory::Resource)
                .map_err(|err| WisError::InvalidInput {
                    message: format!("resolve dock icon: {err}"),
                })?;
            let path_str = path.to_string_lossy().to_string();
            let c = CString::new(path_str).map_err(|err| WisError::InvalidInput {
                message: format!("dock icon path: {err}"),
            })?;

            unsafe {
                let ns_path: *mut AnyObject =
                    msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()];
                let alloc: *mut AnyObject = msg_send![class!(NSImage), alloc];
                msg_send![alloc, initWithContentsOfFile: ns_path]
            }
        } else {
            // Null restores the default (bundled) application icon.
            std::ptr::null_mut()
        };

        unsafe {
            let ns_app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            let _: () = msg_send![ns_app, setApplicationIconImage: image];
        }
    }

    #[cfg(not(target_os = "macos"))]
    let _ = (app, variant);

    Ok(())
}

// ---------------------------------------------------------------------------
// Filter list refresh
// ---------------------------------------------------------------------------

/// Refresh the EasyPrivacy filter list from upstream, replace the running
/// tracker engine, and re-attach the new rules to every existing tab.
/// Returns the number of bytes in the new filter text so the UI can confirm
/// the swap happened.
#[tauri::command]
pub async fn refresh_filter_lists(app: AppHandle, state: State<'_, AppState>) -> WisResult<u32> {
    let cache_dir = state.filter_cache_dir.clone();
    let text = wis_network::filter_lists::fetch::refresh_easyprivacy(&cache_dir)
        .await
        .map_err(|err| WisError::InvalidInput {
            message: format!("refresh: {err}"),
        })?;
    let size = state
        .rebuild_filter_engine(text)
        .await
        .map_err(wis_core::WisError::from)?;

    // Walk every live tab webview and re-attach the new specs so the change
    // takes effect without a navigation.
    let tabs: Vec<(wis_core::TabId, wis_core::PrivacyMode)> = {
        let browser = state.browser.lock().await;
        browser
            .tabs
            .iter()
            .filter_map(|tab| {
                browser
                    .workspace(tab.workspace_id)
                    .ok()
                    .map(|w| (tab.id, w.privacy_mode))
            })
            .collect()
    };
    for (tab_id, mode) in tabs {
        let label = format!("tab-{}", tab_id.as_uuid());
        let Some(webview) = app.webviews().get(&label).cloned() else {
            continue;
        };
        let _ = crate::content_filter::clear_rule_lists(&webview);
        if let Some(spec) = state.content_rule_spec(mode) {
            if let Err(err) = crate::content_filter::attach_rule_list(&webview, &spec) {
                log::warn!("content filter: re-attach for {label} skipped: {err}");
            }
        }
    }

    Ok(size as u32)
}
