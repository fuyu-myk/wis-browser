//! Bookmark IPC commands.
//!
//! Thin wrappers over the [`wis_storage`] bookmarks store, run off the async
//! runtime via `spawn_blocking`. Writes are rejected for ephemeral (private)
//! workspaces, which never persist. Each mutation broadcasts `bookmarks-changed`
//! so every webview (main chrome, popup, picker; each with its own React Query
//! cache) refetches and stays in sync.

use tauri::{AppHandle, Emitter, State};
use wis_core::{WisError, WisResult, WorkspaceId};
use wis_storage::{Bookmark, BookmarkFolder};

use crate::state::AppState;

/// Event broadcast to every webview after any bookmark change.
const BOOKMARKS_CHANGED_EVENT: &str = "bookmarks-changed";

fn notify(app: &AppHandle) {
    let _ = app.emit(BOOKMARKS_CHANGED_EVENT, ());
}

/// Run a blocking storage call off the async runtime.
async fn run<T, F>(op: F) -> WisResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> wis_storage::StorageResult<T> + Send + 'static,
{
    tokio::task::spawn_blocking(op)
        .await
        .map_err(|err| WisError::PersistenceFailed {
            reason: format!("bookmarks task panicked: {err}"),
        })?
        .map_err(|err| WisError::PersistenceFailed {
            reason: err.to_string(),
        })
}

/// Reject writes to ephemeral (private) workspaces, which are never persisted.
async fn ensure_persistable(state: &AppState, workspace_id: WorkspaceId) -> WisResult<()> {
    let browser = state.browser.lock().await;
    let ephemeral = browser
        .workspace(workspace_id)
        .map(|w| w.ephemeral)
        .unwrap_or(false);
    if ephemeral {
        return Err(WisError::InvalidInput {
            message: "bookmarks are disabled in private workspaces".into(),
        });
    }

    Ok(())
}

#[tauri::command]
pub async fn list_bookmark_folders(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
) -> WisResult<Vec<BookmarkFolder>> {
    let storage = state.storage.clone();
    run(move || storage.list_bookmark_folders(workspace_id)).await
}

#[tauri::command]
pub async fn list_bookmarks(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
) -> WisResult<Vec<Bookmark>> {
    let storage = state.storage.clone();
    run(move || storage.list_bookmarks(workspace_id)).await
}

#[tauri::command]
pub async fn create_bookmark_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    name: String,
) -> WisResult<BookmarkFolder> {
    ensure_persistable(&state, workspace_id).await?;
    let storage = state.storage.clone();
    let folder = run(move || storage.create_bookmark_folder(workspace_id, &name)).await?;
    notify(&app);

    Ok(folder)
}

#[tauri::command]
pub async fn rename_bookmark_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    name: String,
) -> WisResult<()> {
    let storage = state.storage.clone();
    run(move || storage.rename_bookmark_folder(id, &name)).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn reorder_bookmark_folders(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<i64>,
) -> WisResult<()> {
    let storage = state.storage.clone();
    run(move || storage.reorder_bookmark_folders(&ids)).await?;
    notify(&app);

    Ok(())
}

#[tauri::command]
pub async fn delete_bookmark_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> WisResult<()> {
    let storage = state.storage.clone();
    run(move || storage.delete_bookmark_folder(id)).await?;
    notify(&app);

    Ok(())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn add_bookmark(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    folder_id: Option<i64>,
    url: String,
    title: String,
    favicon_hash: Option<String>,
    pinned: bool,
) -> WisResult<Bookmark> {
    ensure_persistable(&state, workspace_id).await?;
    let storage = state.storage.clone();
    let bm = run(move || {
        storage.add_bookmark(
            workspace_id,
            folder_id,
            &url,
            &title,
            favicon_hash.as_deref(),
            pinned,
        )
    })
    .await?;
    notify(&app);

    Ok(bm)
}

#[tauri::command]
pub async fn move_bookmark(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    folder_id: Option<i64>,
) -> WisResult<()> {
    let storage = state.storage.clone();
    run(move || storage.move_bookmark(id, folder_id)).await?;
    notify(&app);

    Ok(())
}

#[tauri::command]
pub async fn set_bookmark_pinned(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    pinned: bool,
) -> WisResult<()> {
    let storage = state.storage.clone();
    run(move || storage.set_bookmark_pinned(id, pinned)).await?;
    notify(&app);

    Ok(())
}

#[tauri::command]
pub async fn reorder_pinned_bookmarks(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<i64>,
) -> WisResult<()> {
    let storage = state.storage.clone();
    run(move || storage.reorder_pinned_bookmarks(&ids)).await?;
    notify(&app);

    Ok(())
}

#[tauri::command]
pub async fn reorder_bookmarks(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<i64>,
) -> WisResult<()> {
    let storage = state.storage.clone();
    run(move || storage.reorder_bookmarks(&ids)).await?;
    notify(&app);

    Ok(())
}

#[tauri::command]
pub async fn delete_bookmark(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> WisResult<()> {
    let storage = state.storage.clone();
    run(move || storage.delete_bookmark(id)).await?;
    notify(&app);

    Ok(())
}
