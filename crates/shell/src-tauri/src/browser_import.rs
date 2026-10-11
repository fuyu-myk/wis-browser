//! Import from other browsers' profiles on this machine (`wis_import`).

use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use wis_core::{WisError, WisResult, WorkspaceId};
use wis_storage::BookmarkImportStats;

use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct ImportSource {
    id: String,
    browser: String,
    profile: String,
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
    crate::bookmarks::ensure_persistable(&state, workspace_id).await?;

    let roots = roots(&app)?;
    let parsed = blocking(move || {
        let source =
            wis_import::find(&roots, &source_id).ok_or_else(|| WisError::InvalidInput {
                message: "that browser profile is no longer available".to_string(),
            })?;
        let tree = wis_import::read_bookmarks(&source).map_err(|err| WisError::InvalidInput {
            message: err.to_string(),
        })?;
        wis_core::bookmark_tree::flatten(&tree)
    })
    .await?;

    crate::bookmarks::import_parsed(&app, &state, workspace_id, parsed).await
}

fn roots(app: &AppHandle) -> WisResult<wis_import::Roots> {
    let paths = app.path();
    let located = |dir: tauri::Result<PathBuf>| {
        dir.map_err(|err| WisError::InvalidInput {
            message: format!("cannot locate browser profiles: {err}"),
        })
    };

    Ok(wis_import::Roots {
        home: located(paths.home_dir())?,
        config: located(paths.config_dir())?,
        local_data: located(paths.local_data_dir())?,
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
