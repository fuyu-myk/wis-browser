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
    AccentColor, BrowserState, PrivacyMode, Tab, TabId, UserSettings, UserSettingsPatch, WisError,
    WisResult, Workspace, WorkspaceId,
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
/// (or revert on persist failure).
///
/// `transition` is a pure function from the current state to `(next_state,
/// return_value)`. It must not perform I/O.
async fn mutate<R, F>(state: &State<'_, AppState>, transition: F) -> WisResult<R>
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

    Ok(value)
}

/// Convenience for transitions that don't have an additional return value.
async fn mutate_unit<F>(state: &State<'_, AppState>, transition: F) -> WisResult<()>
where
    F: FnOnce(&BrowserState) -> WisResult<BrowserState>,
{
    mutate(state, |s| transition(s).map(|next| (next, ()))).await
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
    state: State<'_, AppState>,
    name: String,
    accent: AccentColor,
) -> WisResult<Workspace> {
    mutate(&state, |s| Ok(s.with_new_workspace(name, accent))).await
}

#[tauri::command]
pub async fn rename_workspace(
    state: State<'_, AppState>,
    id: WorkspaceId,
    name: String,
) -> WisResult<Workspace> {
    mutate(&state, |s| {
        let next = s.with_renamed_workspace(id, name)?;
        let renamed = next.workspace(id)?.clone();

        Ok((next, renamed))
    })
    .await
}

#[tauri::command]
pub async fn delete_workspace(state: State<'_, AppState>, id: WorkspaceId) -> WisResult<()> {
    mutate_unit(&state, |s| s.with_workspace_removed(id)).await
}

#[tauri::command]
pub async fn recolor_workspace(
    state: State<'_, AppState>,
    id: WorkspaceId,
    accent: AccentColor,
) -> WisResult<Workspace> {
    mutate(&state, |s| {
        let next = s.with_recoloured_workspace(id, accent)?;
        let recoloured = next.workspace(id)?.clone();

        Ok((next, recoloured))
    })
    .await
}

#[tauri::command]
pub async fn set_active_workspace(state: State<'_, AppState>, id: WorkspaceId) -> WisResult<()> {
    mutate_unit(&state, |s| s.with_active_workspace(id)).await
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
    let workspace = mutate(&state, |s| {
        let next = s.with_workspace_privacy_mode(id, mode)?;
        let updated = next.workspace(id)?.clone();
        Ok((next, updated))
    })
    .await?;

    state.clear_proxies().await;

    let spec = state.content_rule_spec(mode).cloned();
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
    for label in tab_labels {
        let Some(webview) = app.webviews().get(&label).cloned() else {
            continue;
        };
        let _ = crate::content_filter::clear_rule_lists(&webview);
        if let Some(spec) = &spec {
            if let Err(err) = crate::content_filter::attach_rule_list(&webview, spec) {
                log::warn!("content filter: re-attach for {label} skipped: {err}");
            }
        }
    }

    Ok(workspace)
}

// ---------------------------------------------------------------------------
// Tab mutations
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn create_tab(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    url: String,
) -> WisResult<Tab> {
    mutate(&state, |s| s.with_new_tab(workspace_id, &url)).await
}

#[tauri::command]
pub async fn close_tab(state: State<'_, AppState>, id: TabId) -> WisResult<()> {
    mutate_unit(&state, |s| s.with_tab_closed(id)).await
}

#[tauri::command]
pub async fn reorder_tabs(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    order: Vec<TabId>,
) -> WisResult<()> {
    mutate_unit(&state, |s| s.with_tab_order(workspace_id, order)).await
}

#[tauri::command]
pub async fn pin_tab(state: State<'_, AppState>, id: TabId, pinned: bool) -> WisResult<Tab> {
    mutate(&state, |s| s.with_tab_pinned(id, pinned)).await
}

#[tauri::command]
pub async fn set_active_tab(state: State<'_, AppState>, id: TabId) -> WisResult<()> {
    mutate_unit(&state, |s| s.with_active_tab(id)).await
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
    mutate_unit(&state, |s| {
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

    let bytes = workspace_id_to_data_store_bytes(workspace_id);

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
    builder = builder.data_store_identifier(bytes);

    // Document title changes: fires when the page sets `<title>` or
    // mutates `document.title` from JS.
    {
        let app = app.clone();
        builder = builder.on_document_title_changed(move |_webview, title: String| {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let state = app.state::<AppState>();
                let result = mutate_unit(&state, |s| s.with_tab_title(tab_id, title.clone())).await;
                match result {
                    Ok(()) => emit_state_changed(&app),
                    Err(err) => log::warn!("title update for {tab_id} failed: {err}"),
                }
            });
        });
    }

    {
        let app = app.clone();
        builder = builder.on_page_load(move |_webview, payload: PageLoadPayload<'_>| {
            let app = app.clone();
            let loading = matches!(payload.event(), PageLoadEvent::Started);
            let url_string = payload.url().to_string();
            tauri::async_runtime::spawn(async move {
                let state = app.state::<AppState>();
                let result = mutate_unit(&state, |s| {
                    s.with_tab_loading(tab_id, loading)
                        .and_then(|next| next.with_tab_url_synced(tab_id, &url_string))
                })
                .await;
                match result {
                    Ok(()) => emit_state_changed(&app),
                    Err(err) => log::warn!("page load update for {tab_id} failed: {err}"),
                }
            });
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

    let privacy_mode = {
        let browser = state.browser.lock().await;
        browser
            .workspace(workspace_id)
            .map(|w| w.privacy_mode)
            .unwrap_or_default()
    };

    // Engine-level filtering is best-effort: a failure must not block the tab.
    if let Some(spec) = state.content_rule_spec(privacy_mode) {
        if let Err(err) = crate::content_filter::attach_rule_list(&child, spec) {
            log::warn!("content filter: attach skipped: {err}");
        }
    }

    Ok(())
}

fn workspace_id_to_data_store_bytes(workspace_id: WorkspaceId) -> [u8; 16] {
    *workspace_id.as_uuid().as_bytes()
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
pub async fn sync_tab_url(state: State<'_, AppState>, id: TabId, url: String) -> WisResult<()> {
    mutate_unit(&state, |s| s.with_tab_url_synced(id, &url)).await
}

// ---------------------------------------------------------------------------
// Navigation event bridge IPC + nav controls
// ---------------------------------------------------------------------------

/// Set the tab's title from a programmatic source (e.g. a chrome-injected
/// rename).
#[tauri::command]
pub async fn update_tab_title(
    state: State<'_, AppState>,
    id: TabId,
    title: String,
) -> WisResult<()> {
    mutate_unit(&state, |s| s.with_tab_title(id, title)).await
}

/// Set a tab's `loading` flag explicitly.
#[tauri::command]
pub async fn update_tab_loading(
    state: State<'_, AppState>,
    id: TabId,
    loading: bool,
) -> WisResult<()> {
    mutate_unit(&state, |s| s.with_tab_loading(id, loading)).await
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

    webview.reload().map_err(|err| WisError::InvalidInput {
        message: format!("reload failed: {err}"),
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
    state: State<'_, AppState>,
    patch: UserSettingsPatch,
) -> WisResult<UserSettings> {
    mutate(&state, |s| {
        let next = s.with_settings(patch);
        let settings = next.settings.clone();

        Ok((next, settings))
    })
    .await
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
