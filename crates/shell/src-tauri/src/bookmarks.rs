//! Bookmark IPC commands.
//!
//! Thin wrappers over the [`wis_storage`] bookmarks store, run off the async
//! runtime via `spawn_blocking`. Writes are rejected for ephemeral (private)
//! workspaces, which never persist. Each mutation broadcasts `bookmarks-changed`
//! so every webview (main chrome, popup, picker; each with its own React Query
//! cache) refetches and stays in sync.

use tauri::{AppHandle, Emitter, State};
use wis_core::{WisError, WisResult, WorkspaceId};
use wis_storage::{Bookmark, BookmarkFolder, BookmarkImport, BookmarkImportStats};

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
pub async fn delete_bookmark(app: AppHandle, state: State<'_, AppState>, id: i64) -> WisResult<()> {
    let storage = state.storage.clone();
    run(move || storage.delete_bookmark(id)).await?;
    notify(&app);

    Ok(())
}

fn suggested_export_name() -> String {
    let stamp = chrono::Local::now().format("%Y-%m-%d");
    format!(
        "{}.html",
        wis_core::sanitize_filename(&format!("Wis Bookmarks {stamp}"))
    )
}

/// Export the workspace's bookmarks as a Netscape `bookmarks.html` (the
/// interchange format every browser imports) via a save panel. Resolves
/// `false` when the user cancels.
#[tauri::command]
pub async fn export_bookmarks_html(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
) -> WisResult<bool> {
    let storage = state.storage.clone();
    let (folders, bookmarks) = run(move || {
        Ok((
            storage.list_bookmark_folders(workspace_id)?,
            storage.list_bookmarks(workspace_id)?,
        ))
    })
    .await?;

    let folder_index: std::collections::HashMap<i64, usize> = folders
        .iter()
        .enumerate()
        .map(|(index, folder)| (folder.id, index))
        .collect();
    let export = wis_core::ParsedBookmarks {
        folders: folders.into_iter().map(|f| f.name).collect(),
        bookmarks: bookmarks
            .into_iter()
            .map(|b| wis_core::ParsedBookmark {
                folder: b.folder_id.and_then(|id| folder_index.get(&id).copied()),
                url: b.url,
                title: b.title,
                added_at: Some(b.created_at.timestamp()),
            })
            .collect(),
    };
    let html = wis_core::to_netscape(&export);

    let Some(path) = crate::panels::run_save_panel(&app, &suggested_export_name())? else {
        return Ok(false);
    };
    std::fs::write(&path, html).map_err(|err| WisError::InvalidInput {
        message: format!("write {}: {err}", path.display()),
    })?;

    Ok(true)
}

/// Pick a bookmarks HTML file and import it into the workspace: folders are
/// reused by name or created, duplicate rows skipped, everything appended
/// unpinned. Resolves `None` when the user cancels.
#[tauri::command]
pub async fn import_bookmarks_html(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
) -> WisResult<Option<BookmarkImportStats>> {
    ensure_persistable(&state, workspace_id).await?;

    let Some(path) = crate::panels::run_open_panel(&app, &["html", "htm"])? else {
        return Ok(None);
    };

    // Bound the read itself rather than pre-checking metadata (which bounds
    // neither FIFOs nor a file grown after the check).
    let parsed = tokio::task::spawn_blocking(move || -> WisResult<wis_core::ParsedBookmarks> {
        use std::io::Read;

        let mut source = String::new();
        std::fs::File::open(&path)
            .map_err(|err| WisError::InvalidInput {
                message: format!("read {}: {err}", path.display()),
            })?
            .take(wis_core::netscape::MAX_BOOKMARKS_FILE_BYTES as u64 + 1)
            .read_to_string(&mut source)
            .map_err(|err| WisError::InvalidInput {
                message: format!("read {}: {err}", path.display()),
            })?;
        wis_core::parse_netscape(&source)
    })
    .await
    .map_err(|err| WisError::InvalidInput {
        message: format!("import task failed: {err}"),
    })??;

    let folders = parsed.folders;
    let entries: Vec<BookmarkImport> = parsed
        .bookmarks
        .into_iter()
        .map(|b| BookmarkImport {
            folder: b.folder,
            url: b.url,
            title: b.title,
        })
        .collect();
    let storage = state.storage.clone();
    let stats = run(move || storage.import_bookmarks(workspace_id, &folders, &entries)).await?;
    notify(&app);

    Ok(Some(stats))
}
