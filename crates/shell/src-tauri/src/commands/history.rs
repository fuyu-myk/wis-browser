use super::*;

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
    run_history(move || {
        let urls = storage.history_urls(&ids)?;
        storage.delete_history_entries(&ids)?;
        storage.delete_closed_tabs_by_urls(&urls)
    })
    .await
}

#[tauri::command]
pub async fn clear_history(
    state: State<'_, AppState>,
    workspace_id: Option<WorkspaceId>,
    since_unix: Option<i64>,
) -> WisResult<()> {
    let since = wis_core::clear_since(since_unix, chrono::Utc::now())?;
    let storage = state.storage.clone();

    run_history(move || {
        storage.clear_history(workspace_id, since)?;
        storage.clear_closed_tabs(workspace_id, since)
    })
    .await?;
    let usage_from = since.map(|instant| instant.with_timezone(&chrono::Local).date_naive());
    crate::usage::clear(&state, workspace_id, usage_from).await?;
    state.closed_tab_sessions.lock().await.clear();

    Ok(())
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
