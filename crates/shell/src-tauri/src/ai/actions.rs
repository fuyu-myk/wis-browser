//! The assistant's tab actions over the asking window's own workspace: open,
//! switch, close and group tabs.
//!
//! The chrome shows the user exactly what a call will do and runs it only
//! once they accept. These commands still re-check both settings and resolve
//! every tab within the window's workspace, so a call the user never saw
//! cannot reach further. Closing goes through `close_tab`, so closed tabs
//! stay reopenable.

use serde::Serialize;
use tauri::{AppHandle, State};
use wis_core::{AccentColor, BrowserState, Tab, TabFolder, WisError, WisResult, WorkspaceId};

use crate::ai::local::{clip, redact_url, resolve_tab, AiTabRead, MAX_TITLE_CHARS};
use crate::commands::{mutate, mutate_unit};
use crate::state::AppState;

const MAX_OPEN: usize = 5;
const MAX_OPEN_URL_CHARS: usize = 2048;
const MAX_TABS: usize = 20;
const MAX_FOLDER_CHARS: usize = 60;
/// The order the sidebar cycles new folders' accents in.
const FOLDER_ACCENTS: [AccentColor; 8] = [
    AccentColor::Primary,
    AccentColor::Mint,
    AccentColor::Rose,
    AccentColor::Sun,
    AccentColor::Lila,
    AccentColor::Gray,
    AccentColor::Ink,
    AccentColor::Azur,
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AiGrouped {
    pub folder: String,
    pub created: bool,
    pub tabs: Vec<AiTabRead>,
}

fn invalid(message: impl Into<String>) -> WisError {
    WisError::InvalidInput {
        message: message.into(),
    }
}

fn read_of(tab: &Tab) -> AiTabRead {
    AiTabRead {
        id: tab.id,
        pinned: tab.pinned,
        title: clip(&tab.title, MAX_TITLE_CHARS),
        url: redact_url(tab.url.as_str()),
    }
}

/// Every wanted tab once, in the order given; the first id that matches no
/// tab fails the whole call.
fn resolve_tabs<'a>(tabs: &[&'a Tab], wanted: &[String]) -> WisResult<Vec<&'a Tab>> {
    if wanted.is_empty() || wanted.len() > MAX_TABS {
        return Err(invalid(format!("give 1 to {MAX_TABS} tab ids")));
    }

    let mut found: Vec<&Tab> = Vec::new();
    for id in wanted {
        let tab = resolve_tab(tabs, id).ok_or_else(|| {
            invalid(format!(
                "no open tab in this workspace has the id {}; call list_tabs first",
                clip(id.trim(), 40)
            ))
        })?;
        if !found.iter().any(|t| t.id == tab.id) {
            found.push(tab);
        }
    }

    Ok(found)
}

/// The URLs to open, parsed: what the approval card shows and what opens.
/// Credentials and very long addresses are refused.
fn checked_urls(urls: &[String]) -> WisResult<Vec<url::Url>> {
    if urls.is_empty() || urls.len() > MAX_OPEN {
        return Err(invalid(format!("give 1 to {MAX_OPEN} URLs")));
    }

    let mut checked: Vec<url::Url> = Vec::new();
    for raw in urls {
        let refused = || {
            invalid(format!(
                "this URL cannot be opened: {}",
                clip(raw.trim(), 80)
            ))
        };
        let url = url::Url::parse(raw.trim())
            .ok()
            .filter(|url| matches!(url.scheme(), "http" | "https"))
            .filter(|url| url.username().is_empty() && url.password().is_none())
            .filter(|url| url.as_str().len() <= MAX_OPEN_URL_CHARS)
            .ok_or_else(refused)?;
        if !checked.contains(&url) {
            checked.push(url);
        }
    }

    Ok(checked)
}

fn checked_folder_name(name: &str) -> WisResult<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_FOLDER_CHARS {
        return Err(invalid(format!(
            "a folder name must be 1 to {MAX_FOLDER_CHARS} characters"
        )));
    }
    Ok(name.to_string())
}

fn folder_named<'a>(folders: &'a [TabFolder], name: &str) -> Option<&'a TabFolder> {
    folders
        .iter()
        .find(|folder| folder.name.trim().eq_ignore_ascii_case(name.trim()))
}

fn next_accent(folders: usize) -> AccentColor {
    FOLDER_ACCENTS[folders % FOLDER_ACCENTS.len()]
}

/// The calling window's workspace, once both settings allow tab actions.
fn action_workspace(
    state: &AppState,
    window: &tauri::Window,
    browser: &BrowserState,
) -> WisResult<WorkspaceId> {
    let ai = &browser.settings.ai;
    if !(ai.local_data && ai.tab_actions) {
        return Err(invalid("tab actions are turned off in settings"));
    }
    state
        .window_workspace(window.label())
        .ok_or_else(|| invalid("this window is not showing a workspace"))
}

/// The tabs a pending action names, for the user to approve.
#[tauri::command]
pub async fn ai_preview_tabs(
    window: tauri::Window,
    state: State<'_, AppState>,
    tabs: Vec<String>,
) -> WisResult<Vec<AiTabRead>> {
    let browser = state.browser.lock().await;
    let ws = action_workspace(&state, &window, &browser)?;
    let open = browser.tabs_for(ws)?;
    Ok(resolve_tabs(&open, &tabs)?
        .into_iter()
        .map(read_of)
        .collect())
}

/// The URLs a pending `open_tabs` names, in the form they would open.
#[tauri::command]
pub async fn ai_preview_urls(
    window: tauri::Window,
    state: State<'_, AppState>,
    urls: Vec<String>,
) -> WisResult<Vec<String>> {
    let checked = checked_urls(&urls)?;
    let browser = state.browser.lock().await;
    action_workspace(&state, &window, &browser)?;
    Ok(checked.iter().map(|url| url.as_str().to_string()).collect())
}

/// Open web pages in the window's workspace: one comes to the front, several
/// open behind the current tab.
#[tauri::command]
pub async fn ai_open_tabs(
    window: tauri::Window,
    app: AppHandle,
    state: State<'_, AppState>,
    urls: Vec<String>,
) -> WisResult<Vec<String>> {
    let urls = checked_urls(&urls)?;
    let ws = {
        let browser = state.browser.lock().await;
        action_workspace(&state, &window, &browser)?
    };
    let single = urls.len() == 1;
    for url in &urls {
        if single {
            mutate(&app, &state, |s| s.with_new_tab(ws, url.as_str())).await?;
        } else {
            mutate(&app, &state, |s| {
                s.with_new_background_tab(ws, url.as_str())
            })
            .await?;
        }
    }

    Ok(urls.iter().map(|url| redact_url(url.as_str())).collect())
}

#[tauri::command]
pub async fn ai_switch_tab(
    window: tauri::Window,
    app: AppHandle,
    state: State<'_, AppState>,
    tab: String,
) -> WisResult<AiTabRead> {
    let read = {
        let browser = state.browser.lock().await;
        let ws = action_workspace(&state, &window, &browser)?;
        let open = browser.tabs_for(ws)?;
        read_of(resolve_tabs(&open, &[tab])?[0])
    };
    mutate_unit(&app, &state, |s| s.with_active_tab(read.id)).await?;
    Ok(read)
}

#[tauri::command]
pub async fn ai_close_tabs(
    window: tauri::Window,
    app: AppHandle,
    state: State<'_, AppState>,
    tabs: Vec<String>,
) -> WisResult<Vec<AiTabRead>> {
    let reads: Vec<AiTabRead> = {
        let browser = state.browser.lock().await;
        let ws = action_workspace(&state, &window, &browser)?;
        let open = browser.tabs_for(ws)?;
        resolve_tabs(&open, &tabs)?
            .into_iter()
            .map(read_of)
            .collect()
    };
    for read in &reads {
        crate::commands::close_tab(app.clone(), state.clone(), read.id, None, None).await?;
    }
    Ok(reads)
}

/// Put tabs in the workspace folder named `folder`, creating it when absent.
#[tauri::command]
pub async fn ai_group_tabs(
    window: tauri::Window,
    app: AppHandle,
    state: State<'_, AppState>,
    tabs: Vec<String>,
    folder: String,
) -> WisResult<AiGrouped> {
    let name = checked_folder_name(&folder)?;
    let (ws, reads, existing, folder_count) = {
        let browser = state.browser.lock().await;
        let ws = action_workspace(&state, &window, &browser)?;
        let open = browser.tabs_for(ws)?;
        let reads: Vec<AiTabRead> = resolve_tabs(&open, &tabs)?
            .into_iter()
            .map(read_of)
            .collect();
        let folders = &browser.workspace(ws)?.tab_folders;
        (
            ws,
            reads,
            folder_named(folders, &name).map(|f| f.id),
            folders.len(),
        )
    };
    let ids: Vec<_> = reads.iter().map(|read| read.id).collect();
    mutate(&app, &state, |s| {
        let (next, folder_id) = match existing {
            Some(id) => (s.clone(), id),
            None => {
                let (next, created) =
                    s.with_new_tab_folder(ws, name.clone(), next_accent(folder_count))?;
                (next, created.id)
            }
        };
        let grouped = ids.iter().try_fold(next, |acc, id| {
            acc.with_tab_assigned_folder(*id, Some(folder_id))
        })?;
        Ok((grouped, ()))
    })
    .await?;

    Ok(AiGrouped {
        folder: name,
        created: existing.is_none(),
        tabs: reads,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(ws: WorkspaceId, url: &str, title: &str) -> Tab {
        Tab::new(ws, url::Url::parse(url).unwrap()).with_title(title.to_string())
    }

    fn short(tab: &Tab) -> String {
        tab.id.as_uuid().simple().to_string()[..8].to_string()
    }

    #[test]
    fn wanted_tabs_resolve_once_each_in_order_or_fail_on_an_unknown_id() {
        let ws = WorkspaceId::new();
        let a = tab(ws, "https://a.example/", "A");
        let b = tab(ws, "https://b.example/", "B");
        let tabs = vec![&a, &b];

        let found = resolve_tabs(&tabs, &[short(&b), short(&a), short(&b)]).unwrap();
        assert_eq!(found.iter().map(|t| t.id).collect::<Vec<_>>(), [b.id, a.id]);

        let err = resolve_tabs(&tabs, &[short(&a), "ffffffff".into()]).unwrap_err();
        assert!(err.to_string().contains("ffffffff"));
        assert!(resolve_tabs(&tabs, &[]).is_err());
        assert!(resolve_tabs(&tabs, &vec![short(&a); MAX_TABS + 1]).is_err());
    }

    #[test]
    fn read_tabs_carry_a_redacted_url_and_a_clipped_title() {
        let ws = WorkspaceId::new();
        let t = tab(ws, "https://a.example/p?token=1", &"t".repeat(300));
        let read = read_of(&t);
        assert_eq!(read.id, t.id);
        assert_eq!(read.url, "https://a.example/p");
        assert_eq!(read.title.chars().count(), MAX_TITLE_CHARS);
    }

    #[test]
    fn only_a_few_web_urls_can_be_opened_each_once() {
        let opened = checked_urls(&[
            " https://a.example/x ".into(),
            "http://b.example/".into(),
            "https://a.example/x".into(),
        ])
        .unwrap();
        assert_eq!(
            opened.iter().map(url::Url::as_str).collect::<Vec<_>>(),
            ["https://a.example/x", "http://b.example/"]
        );
        let long = format!("https://a.example/{}", "x".repeat(MAX_OPEN_URL_CHARS));
        for bad in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "about:settings",
            "nope",
            "https://user:pw@a.example/",
            long.as_str(),
        ] {
            assert!(checked_urls(&[bad.into()]).is_err(), "{bad}");
        }
        assert!(checked_urls(&[]).is_err());
        let many: Vec<String> = (0..=MAX_OPEN)
            .map(|i| format!("https://e{i}.example/"))
            .collect();
        assert!(checked_urls(&many).is_err());
    }

    #[test]
    fn urls_are_shown_and_opened_in_their_parsed_form() {
        let opened = checked_urls(&[
            "HTTPS://Example.COM/a b".into(),
            "https://good.example\t.evil.example/".into(),
        ])
        .unwrap();
        assert_eq!(
            opened.iter().map(url::Url::as_str).collect::<Vec<_>>(),
            [
                "https://example.com/a%20b",
                "https://good.example.evil.example/"
            ]
        );
    }

    #[test]
    fn previewed_tabs_say_whether_they_are_pinned() {
        let ws = WorkspaceId::new();
        let pinned = tab(ws, "https://a.example/", "A").with_pinned(true);
        assert!(read_of(&pinned).pinned);
    }

    #[test]
    fn folder_names_are_trimmed_bounded_and_matched_ignoring_case() {
        assert_eq!(checked_folder_name("  Research ").unwrap(), "Research");
        assert!(checked_folder_name("   ").is_err());
        assert!(checked_folder_name(&"x".repeat(MAX_FOLDER_CHARS + 1)).is_err());

        let folders = vec![TabFolder::new("Research".into(), AccentColor::Mint, 0)];
        assert_eq!(
            folder_named(&folders, "research").map(|f| f.id),
            Some(folders[0].id)
        );
        assert!(folder_named(&folders, "Reading").is_none());
    }

    #[test]
    fn new_folders_cycle_the_sidebar_accents() {
        assert_eq!(next_accent(0), AccentColor::Primary);
        assert_eq!(next_accent(2), AccentColor::Rose);
        assert_eq!(next_accent(9), AccentColor::Mint);
    }
}
