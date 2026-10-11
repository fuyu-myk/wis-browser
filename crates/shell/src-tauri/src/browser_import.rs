//! Import from other browsers' profiles on this machine (`wis_import`).

use std::path::PathBuf;

use chrono::{Duration, Utc};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use wis_core::{WisError, WisResult, WorkspaceId};
use wis_storage::{BookmarkImportStats, HistoryImport, HistoryImportStats};

use crate::state::AppState;

const HISTORY_DAYS: i64 = 180;

#[derive(Debug, Serialize)]
pub struct ImportSource {
    id: String,
    browser: String,
    profile: String,
    bookmarks: bool,
    history: bool,
}

#[tauri::command]
pub async fn list_import_sources(app: AppHandle) -> WisResult<Vec<ImportSource>> {
    let roots = roots(&app)?;
    blocking(move || {
        Ok(wis_import::discover(&roots)
            .into_iter()
            .map(|source| ImportSource {
                browser: source.browser_label(),
                id: source.id,
                profile: source.profile,
                bookmarks: source.bookmarks,
                history: source.history,
            })
            .collect())
    })
    .await
}

#[tauri::command]
pub async fn import_browser_bookmarks(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    source_id: String,
) -> WisResult<BookmarkImportStats> {
    ensure_not_private(&state, workspace_id).await?;

    let roots = roots(&app)?;
    let parsed = blocking(move || {
        let tree = wis_import::read_bookmarks(&find(&roots, &source_id)?).map_err(user_error)?;
        wis_core::bookmark_tree::flatten(&tree)
    })
    .await?;

    crate::bookmarks::import_parsed(&app, &state, workspace_id, parsed).await
}

#[tauri::command]
pub async fn import_browser_history(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    source_id: String,
) -> WisResult<HistoryImportStats> {
    ensure_not_private(&state, workspace_id).await?;

    let roots = roots(&app)?;
    let scratch = located(app.path().app_cache_dir())?.join("browser-import");
    let since = Utc::now() - Duration::days(HISTORY_DAYS);
    let entries = blocking(move || {
        let items = wis_import::read_history(&find(&roots, &source_id)?, &scratch, since)
            .map_err(user_error)?;
        Ok(items
            .into_iter()
            .map(|item| HistoryImport {
                url: item.url,
                title: item.title,
                visited_at: item.visited_at,
                visit_count: item.visit_count,
            })
            .collect::<Vec<_>>())
    })
    .await?;

    let storage = state.storage.clone();
    let stats = blocking(move || {
        storage
            .import_history(workspace_id, &entries)
            .map_err(|err| WisError::PersistenceFailed {
                reason: err.to_string(),
            })
    })
    .await?;
    let _ = app.emit(crate::commands::HISTORY_CHANGED_EVENT, ());

    Ok(stats)
}

async fn ensure_not_private(state: &AppState, workspace_id: WorkspaceId) -> WisResult<()> {
    if state
        .browser
        .lock()
        .await
        .workspace(workspace_id)?
        .ephemeral
    {
        return Err(WisError::InvalidInput {
            message: "importing is disabled in private workspaces".to_string(),
        });
    }
    Ok(())
}

fn find(roots: &wis_import::Roots, source_id: &str) -> WisResult<wis_import::Source> {
    wis_import::find(roots, source_id).ok_or_else(|| WisError::InvalidInput {
        message: "that browser profile is no longer available".to_string(),
    })
}

fn user_error(err: wis_import::ImportError) -> WisError {
    WisError::InvalidInput {
        message: err.to_string(),
    }
}

fn roots(app: &AppHandle) -> WisResult<wis_import::Roots> {
    let paths = app.path();
    Ok(wis_import::Roots {
        home: located(paths.home_dir())?,
        config: located(paths.config_dir())?,
        local_data: located(paths.local_data_dir())?,
    })
}

fn located(dir: tauri::Result<PathBuf>) -> WisResult<PathBuf> {
    dir.map_err(|err| WisError::InvalidInput {
        message: format!("cannot locate browser profiles: {err}"),
    })
}

async fn blocking<T: Send + 'static>(
    op: impl FnOnce() -> WisResult<T> + Send + 'static,
) -> WisResult<T> {
    tokio::task::spawn_blocking(op)
        .await
        .map_err(|err| WisError::InvalidInput {
            message: format!("import task failed: {err}"),
        })?
}
