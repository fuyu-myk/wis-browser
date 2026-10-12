//! The assistant's read-only tools over the asking window's own workspace:
//! its open tabs, history and bookmarks.
//!
//! Every command re-checks `ai.local_data` and resolves the workspace from
//! the calling window, so neither a streamed tool call the chrome did not
//! declare nor a forged argument can skip the switch or reach another
//! workspace. Results are data for the model, capped and never markup.

use chrono::{DateTime, Utc};
use serde::Serialize;
use tauri::State;
use wis_core::{BrowserState, Tab, TabId, WisError, WisResult, WorkspaceId};
use wis_storage::{Bookmark, BookmarkFolder, HistoryEntry, HistoryQuery};

use crate::state::AppState;

const MAX_TABS: usize = 200;
const MAX_HITS: usize = 20;
/// History rows matching the longest word, before every word is required.
const HISTORY_SCAN: u32 = 400;
const SHORT_ID_LEN: usize = 8;
const MAX_QUERY_CHARS: usize = 200;
pub(crate) const MAX_TITLE_CHARS: usize = 200;
const MAX_URL_CHARS: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AiTab {
    pub id: String,
    pub title: String,
    pub url: String,
    pub active: bool,
    pub pinned: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AiHistoryHit {
    pub title: String,
    pub url: String,
    pub last_visited: DateTime<Utc>,
    pub visits: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AiBookmarkHit {
    pub title: String,
    pub url: String,
    pub folder: Option<String>,
}

/// A tab the chrome may read for the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AiTabRead {
    pub id: TabId,
    pub title: String,
    pub url: String,
    pub pinned: bool,
}

fn short_id(id: TabId) -> String {
    id.as_uuid().simple().to_string()[..SHORT_ID_LEN].to_string()
}

pub(crate) fn clip(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// A URL without its query, fragment or credentials, which can carry login
/// and reset tokens, clipped for the model.
pub(crate) fn redact_url(raw: &str) -> String {
    let redacted = match url::Url::parse(raw) {
        Ok(mut url) => {
            url.set_query(None);
            url.set_fragment(None);
            let _ = url.set_username("");
            let _ = url.set_password(None);
            url.to_string()
        }
        Err(_) => raw.split(['?', '#']).next().unwrap_or_default().to_string(),
    };
    clip(&redacted, MAX_URL_CHARS)
}

fn words(query: &str) -> Vec<String> {
    query.split_whitespace().map(str::to_lowercase).collect()
}

fn contains_all(text: &str, words: &[String]) -> bool {
    let text = text.to_lowercase();
    words.iter().all(|word| text.contains(word.as_str()))
}

/// The one tab whose id starts with `wanted` (hyphens and case ignored).
pub(crate) fn resolve_tab<'a>(tabs: &[&'a Tab], wanted: &str) -> Option<&'a Tab> {
    let wanted = wanted.trim().to_lowercase().replace('-', "");
    if wanted.is_empty() {
        return None;
    }

    let mut found = tabs
        .iter()
        .filter(|tab| tab.id.as_uuid().simple().to_string().starts_with(&wanted));
    match (found.next(), found.next()) {
        (Some(tab), None) => Some(tab),
        _ => None,
    }
}

fn list_tabs(tabs: &[&Tab], active: Option<TabId>) -> Vec<AiTab> {
    tabs.iter()
        .take(MAX_TABS)
        .map(|tab| AiTab {
            id: short_id(tab.id),
            title: clip(&tab.title, MAX_TITLE_CHARS),
            url: redact_url(tab.url.as_str()),
            active: Some(tab.id) == active,
            pinned: tab.pinned,
        })
        .collect()
}

fn history_hits(rows: Vec<HistoryEntry>, words: &[String]) -> Vec<AiHistoryHit> {
    rows.into_iter()
        .filter(|row| contains_all(&format!("{} {}", row.title, row.url), words))
        .take(MAX_HITS)
        .map(|row| AiHistoryHit {
            title: clip(&row.title, MAX_TITLE_CHARS),
            url: redact_url(&row.url),
            last_visited: row.visited_at,
            visits: row.visit_count,
        })
        .collect()
}

fn bookmark_hits(
    bookmarks: &[Bookmark],
    folders: &[BookmarkFolder],
    words: &[String],
) -> Vec<AiBookmarkHit> {
    let folder_name = |id: Option<i64>| {
        id.and_then(|id| folders.iter().find(|f| f.id == id))
            .map(|f| f.name.clone())
    };
    bookmarks
        .iter()
        .map(|mark| (mark, folder_name(mark.folder_id)))
        .filter(|(mark, folder)| {
            let text = format!(
                "{} {} {}",
                mark.title,
                mark.url,
                folder.as_deref().unwrap_or("")
            );
            contains_all(&text, words)
        })
        .take(MAX_HITS)
        .map(|(mark, folder)| AiBookmarkHit {
            title: clip(&mark.title, MAX_TITLE_CHARS),
            url: redact_url(&mark.url),
            folder,
        })
        .collect()
}

fn checked_query(query: &str) -> WisResult<Vec<String>> {
    if query.chars().count() > MAX_QUERY_CHARS {
        return Err(WisError::InvalidInput {
            message: format!("search query must be at most {MAX_QUERY_CHARS} characters"),
        });
    }
    Ok(words(query))
}

/// The calling window's workspace, once the setting allows local reads. A
/// window with no workspace bound fails rather than borrowing the active one,
/// which may be another window's private workspace.
fn local_workspace(
    state: &AppState,
    window: &tauri::Window,
    browser: &BrowserState,
) -> WisResult<WorkspaceId> {
    if !browser.settings.ai.local_data {
        return Err(WisError::InvalidInput {
            message: "reading tabs, history and bookmarks is turned off in settings".into(),
        });
    }
    state
        .window_workspace(window.label())
        .ok_or_else(|| WisError::InvalidInput {
            message: "this window is not showing a workspace".into(),
        })
}

fn storage_failed(err: impl std::fmt::Display) -> WisError {
    WisError::PersistenceFailed {
        reason: err.to_string(),
    }
}

#[tauri::command]
pub async fn ai_list_tabs(
    window: tauri::Window,
    state: State<'_, AppState>,
) -> WisResult<Vec<AiTab>> {
    let browser = state.browser.lock().await;
    let ws = local_workspace(&state, &window, &browser)?;
    let active = browser.workspace(ws)?.active_tab_id;
    Ok(list_tabs(&browser.tabs_for(ws)?, active))
}

#[tauri::command]
pub async fn ai_search_history(
    window: tauri::Window,
    state: State<'_, AppState>,
    query: String,
) -> WisResult<Vec<AiHistoryHit>> {
    let words = checked_query(&query)?;
    let ws = {
        let browser = state.browser.lock().await;
        local_workspace(&state, &window, &browser)?
    };
    let longest = words
        .iter()
        .max_by_key(|word| word.chars().count())
        .cloned()
        .unwrap_or_default();
    let storage = state.storage.clone();
    let rows = tokio::task::spawn_blocking(move || {
        storage.query_history(&HistoryQuery {
            search: longest,
            workspace_id: Some(ws),
            limit: HISTORY_SCAN,
            offset: 0,
        })
    })
    .await
    .map_err(storage_failed)?
    .map_err(storage_failed)?;

    Ok(history_hits(rows, &words))
}

#[tauri::command]
pub async fn ai_search_bookmarks(
    window: tauri::Window,
    state: State<'_, AppState>,
    query: String,
) -> WisResult<Vec<AiBookmarkHit>> {
    let words = checked_query(&query)?;
    let ws = {
        let browser = state.browser.lock().await;
        local_workspace(&state, &window, &browser)?
    };
    let storage = state.storage.clone();
    let (bookmarks, folders) = tokio::task::spawn_blocking(move || {
        Ok::<_, wis_storage::StorageError>((
            storage.list_bookmarks(ws)?,
            storage.list_bookmark_folders(ws)?,
        ))
    })
    .await
    .map_err(storage_failed)?
    .map_err(storage_failed)?;

    Ok(bookmark_hits(&bookmarks, &folders, &words))
}

/// Resolve a model's tab id within the workspace to a tab that can be read.
/// The chrome then asks that tab for its text the same way it reads the
/// page a user attaches, so the text arrives as a `wis-page-text` event.
#[tauri::command]
pub async fn ai_resolve_tab(
    window: tauri::Window,
    state: State<'_, AppState>,
    tab: String,
) -> WisResult<AiTabRead> {
    let read = {
        let browser = state.browser.lock().await;
        let ws = local_workspace(&state, &window, &browser)?;
        let tabs = browser.tabs_for(ws)?;
        let found = resolve_tab(&tabs, &tab).ok_or_else(|| WisError::InvalidInput {
            message: "no open tab in this workspace has that id; call list_tabs first".into(),
        })?;
        AiTabRead {
            id: found.id,
            pinned: found.pinned,
            title: clip(&found.title, MAX_TITLE_CHARS),
            url: redact_url(found.url.as_str()),
        }
    };
    if !matches!(read.url.split(':').next(), Some("http" | "https")) {
        return Err(WisError::InvalidInput {
            message: "only web pages can be read, not Wis pages".into(),
        });
    }

    let label = format!("tab-{}", read.id.as_uuid());
    if tauri::Manager::get_webview(&window, &label).is_none() {
        return Err(WisError::InvalidInput {
            message: "that tab is asleep, so it cannot be read until the user opens it".into(),
        });
    }

    Ok(read)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn tab(ws: WorkspaceId, url: &str, title: &str) -> Tab {
        Tab::new(ws, url::Url::parse(url).unwrap()).with_title(title.to_string())
    }

    fn at(day: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, day, 12, 0, 0).unwrap()
    }

    #[test]
    fn urls_lose_their_query_fragment_and_credentials() {
        assert_eq!(
            redact_url("https://user:pw@x.example/reset/path?token=abc#frag"),
            "https://x.example/reset/path"
        );
        assert_eq!(redact_url("https://x.example/"), "https://x.example/");
        assert_eq!(redact_url("about:settings"), "about:settings");
        assert_eq!(redact_url("not a url?secret=1#x"), "not a url");
    }

    #[test]
    fn returned_urls_are_redacted_but_matching_sees_the_whole_url() {
        let ws = WorkspaceId::new();
        let tab = tab(ws, "https://x.example/a?session=1", "A");
        assert_eq!(list_tabs(&[&tab], None)[0].url, "https://x.example/a");

        let row = HistoryEntry {
            id: 1,
            workspace_id: ws,
            url: "https://search.example/?q=rust+async".into(),
            title: "Search".into(),
            visited_at: at(10),
            visit_count: 1,
        };
        let hits = history_hits(vec![row], &words("async"));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].url, "https://search.example/");
    }

    #[test]
    fn short_ids_are_the_first_hex_digits() {
        let id = TabId::new();
        let short = short_id(id);
        assert_eq!(short.len(), SHORT_ID_LEN);
        assert!(id.as_uuid().to_string().starts_with(&short));
    }

    #[test]
    fn tabs_resolve_by_a_unique_prefix_within_the_given_tabs() {
        let ws = WorkspaceId::new();
        let a = tab(ws, "https://a.example/", "A");
        let b = tab(ws, "https://b.example/", "B");
        let tabs = vec![&a, &b];

        let wanted = short_id(b.id).to_uppercase();
        assert_eq!(
            resolve_tab(&tabs, &format!(" {wanted} ")).map(|t| t.id),
            Some(b.id)
        );
        assert_eq!(
            resolve_tab(&tabs, &a.id.as_uuid().to_string()).map(|t| t.id),
            Some(a.id)
        );
        assert!(resolve_tab(&tabs, "").is_none());
        assert!(resolve_tab(&tabs, "zzzzzzzz").is_none());

        let elsewhere = tab(WorkspaceId::new(), "https://c.example/", "C");
        assert!(resolve_tab(&tabs, &short_id(elsewhere.id)).is_none());
    }

    #[test]
    fn listed_tabs_mark_the_active_and_pinned_ones_and_clip_long_text() {
        let ws = WorkspaceId::new();
        let a = tab(ws, "https://a.example/", &"x".repeat(400));
        let b = tab(ws, "https://b.example/", "B").with_pinned(true);

        let listed = list_tabs(&[&a, &b], Some(a.id));
        assert_eq!(listed.len(), 2);
        assert!(listed[0].active && !listed[0].pinned);
        assert_eq!(listed[0].title.chars().count(), MAX_TITLE_CHARS);
        assert!(!listed[1].active && listed[1].pinned);
        assert_eq!(listed[1].id, short_id(b.id));
    }

    #[test]
    fn every_word_must_appear_case_insensitively() {
        let query = words("  Rust  ASYNC ");
        assert_eq!(query, ["rust", "async"]);
        assert!(contains_all(
            "The Async Rust Book https://rust-lang.github.io",
            &query
        ));
        assert!(!contains_all("Rust by example", &query));
        assert!(contains_all("anything", &words("")));
    }

    #[test]
    fn history_hits_keep_rows_with_every_word_up_to_the_cap() {
        let ws = WorkspaceId::new();
        let row = |i: i64, url: &str, title: &str| HistoryEntry {
            id: i,
            workspace_id: ws,
            url: url.into(),
            title: title.into(),
            visited_at: at(10),
            visit_count: i,
        };
        let rows = vec![
            row(1, "https://rust-lang.github.io/async-book/", "Async Rust"),
            row(2, "https://rust-lang.org/", "Rust"),
            row(3, "https://example.com/async", "Async JS"),
        ];

        let hits = history_hits(rows.clone(), &words("rust async"));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "Async Rust");
        assert_eq!(hits[0].visits, 1);

        let many: Vec<HistoryEntry> = (0..50).map(|i| row(i, "https://e.example/", "E")).collect();
        assert_eq!(history_hits(many, &words("")).len(), MAX_HITS);
    }

    #[test]
    fn bookmark_hits_match_the_folder_name_too() {
        let ws = WorkspaceId::new();
        let folder = BookmarkFolder {
            id: 7,
            workspace_id: ws,
            name: "Reading list".into(),
            position: 0,
            created_at: at(1),
        };
        let mark = |id: i64, folder_id: Option<i64>, url: &str, title: &str| Bookmark {
            id,
            workspace_id: ws,
            folder_id,
            url: url.into(),
            title: title.into(),
            favicon_hash: None,
            position: id,
            pinned: false,
            pin_position: 0,
            created_at: at(1),
        };
        let marks = vec![
            mark(1, Some(7), "https://essay.example/", "An essay"),
            mark(2, None, "https://reading.example/", "Speed reading"),
            mark(3, None, "https://other.example/", "Other"),
        ];

        let hits = bookmark_hits(&marks, &[folder], &words("reading"));
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].folder.as_deref(), Some("Reading list"));
        assert_eq!(hits[1].folder, None);
    }
}
