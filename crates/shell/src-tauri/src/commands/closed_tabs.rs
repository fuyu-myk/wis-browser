use std::collections::HashSet;

use super::tab::run_storage;
use super::*;

pub const CLOSED_TABS_CHANGED_EVENT: &str = "closed-tabs-changed";

/// After rows were recorded, reopened, removed or cleared, for pages listing
/// them.
pub(crate) fn closed_tabs_changed(app: &AppHandle) {
    if let Err(err) = app.emit(CLOSED_TABS_CHANGED_EVENT, ()) {
        log::warn!("closed tabs event failed: {err}");
    }
}

/// Drops one entry from Recently closed.
#[tauri::command]
pub async fn forget_closed_tab(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> WisResult<()> {
    let storage = state.storage.clone();
    run_storage(move || storage.take_closed_tab(id).map(drop)).await?;
    state.closed_tab_sessions.lock().await.remove(&id);
    closed_tabs_changed(&app);
    Ok(())
}

/// Empties Recently closed, for one workspace when given.
#[tauri::command]
pub async fn clear_closed_tabs(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: Option<WorkspaceId>,
) -> WisResult<()> {
    let storage = state.storage.clone();
    let left: HashSet<i64> = run_storage(move || {
        storage.clear_closed_tabs(workspace_id, None)?;
        Ok(storage
            .list_closed_tabs(CLOSED_TAB_CAP)?
            .into_iter()
            .map(|entry| entry.id)
            .collect())
    })
    .await?;
    state
        .closed_tab_sessions
        .lock()
        .await
        .retain(|row, _| left.contains(row));
    closed_tabs_changed(&app);
    Ok(())
}
