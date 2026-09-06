//! URL-bar suggestion commands: combine remembered searches with ranked site
//! recommendations from history, and let the user forget either.

use serde::Serialize;
use tauri::State;
use wis_core::{WisError, WisResult, WorkspaceId};

use crate::state::AppState;

const SEARCH_LIMIT: u32 = 6;
/// A generous site pull so the frontend has enough candidates to find an
/// inline-completion prefix match and fill the recommendations section.
const SITE_LIMIT: u32 = 8;

#[derive(Debug, Serialize)]
pub struct UrlSuggestions {
    pub searches: Vec<SearchSuggestion>,
    pub sites: Vec<SiteSuggestion>,
}

#[derive(Debug, Serialize)]
pub struct SearchSuggestion {
    pub query: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
pub struct SiteSuggestion {
    pub id: i64,
    pub url: String,
    pub title: String,
}

/// Suggestions for the address bar.
///
/// With an empty `query` (the user just focused the bar): the most-recent
/// searches plus the most-visited sites. While typing: searches matching the
/// query plus visit-ranked sites matching it (the frontend uses those for both
/// inline completion and the recommendations section).
#[tauri::command]
pub async fn get_url_suggestions(
    state: State<'_, AppState>,
    query: String,
    workspace_id: WorkspaceId,
) -> WisResult<UrlSuggestions> {
    let empty = query.trim().is_empty();

    let storage = state.storage.clone();
    let q = query.clone();
    let searches = run_blocking(move || {
        if empty {
            storage.recent_searches(SEARCH_LIMIT)
        } else {
            storage.suggest_searches(&q, SEARCH_LIMIT)
        }
    })
    .await?;

    let storage = state.storage.clone();
    let sites =
        run_blocking(move || storage.recommend_sites(workspace_id, &query, SITE_LIMIT)).await?;

    Ok(UrlSuggestions {
        searches: searches
            .into_iter()
            .map(|s| SearchSuggestion {
                query: s.query,
                count: s.count,
            })
            .collect(),
        sites: sites
            .into_iter()
            .map(|h| SiteSuggestion {
                id: h.id,
                url: h.url,
                title: h.title,
            })
            .collect(),
    })
}

/// Remember an address-bar search (skipped when `workspace_id` is a
/// private/ephemeral workspace).
#[tauri::command]
pub async fn record_search(
    state: State<'_, AppState>,
    query: String,
    workspace_id: WorkspaceId,
) -> WisResult<()> {
    let trimmed = query.trim().to_string();
    if trimmed.is_empty() {
        return Ok(());
    }

    let ephemeral = state
        .browser
        .lock()
        .await
        .workspace(workspace_id)?
        .ephemeral;
    if ephemeral {
        return Ok(());
    }

    let storage = state.storage.clone();
    run_blocking(move || storage.record_search(&trimmed)).await
}

/// Forget a remembered search so it stops appearing in suggestions.
#[tauri::command]
pub async fn delete_search(state: State<'_, AppState>, query: String) -> WisResult<()> {
    let storage = state.storage.clone();
    run_blocking(move || storage.delete_search(&query)).await
}

async fn run_blocking<T, F>(op: F) -> WisResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> wis_storage::StorageResult<T> + Send + 'static,
{
    tokio::task::spawn_blocking(op)
        .await
        .map_err(|err| WisError::PersistenceFailed {
            reason: format!("suggestion task panicked: {err}"),
        })?
        .map_err(|err| WisError::PersistenceFailed {
            reason: err.to_string(),
        })
}
