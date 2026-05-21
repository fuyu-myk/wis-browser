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

use tauri::State;
use wis_core::{
    AccentColor, BrowserState, Tab, TabId, UserSettings, UserSettingsPatch, WisError, WisResult,
    Workspace, WorkspaceId,
};

use crate::state::AppState;

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
pub async fn set_active_workspace(state: State<'_, AppState>, id: WorkspaceId) -> WisResult<()> {
    mutate_unit(&state, |s| s.with_active_workspace(id)).await
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
