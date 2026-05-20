//! Tauri IPC commands exposing the browser state model to the frontend.
//!
//! Every command is a thin wrapper over an immutable transition on
//! [`BrowserState`]. The lock is held for the duration of the operation, the
//! new state replaces the old, and a typed value is returned. Errors are
//! [`WisError`] from `wis-core` and serialise across IPC unchanged.

use crate::state::AppState;
use tauri::State;
use wis_core::{
    AccentColor, BrowserState, Tab, TabId, UserSettings, UserSettingsPatch, WisResult, Workspace,
    WorkspaceId,
};

fn read_state(state: &State<'_, AppState>) -> BrowserState {
    state
        .browser
        .read()
        .expect("AppState lock poisoned")
        .clone()
}

fn replace_state(state: &State<'_, AppState>, next: BrowserState) {
    let mut guard = state.browser.write().expect("AppState lock poisoned");
    *guard = next;
}

// -- queries ----------------------------------------------------------------

#[tauri::command]
pub fn get_browser_state(state: State<'_, AppState>) -> BrowserState {
    read_state(&state)
}

#[tauri::command]
pub fn get_workspace(state: State<'_, AppState>, id: WorkspaceId) -> WisResult<Workspace> {
    read_state(&state).workspace(id).cloned()
}

#[tauri::command]
pub fn get_tab(state: State<'_, AppState>, id: TabId) -> WisResult<Tab> {
    read_state(&state).tab(id).cloned()
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> UserSettings {
    read_state(&state).settings.clone()
}

// -- workspace mutations ----------------------------------------------------

#[tauri::command]
pub fn create_workspace(
    state: State<'_, AppState>,
    name: String,
    accent: AccentColor,
) -> Workspace {
    let current = read_state(&state);
    let (next, workspace) = current.with_new_workspace(name, accent);
    replace_state(&state, next);

    workspace
}

#[tauri::command]
pub fn rename_workspace(
    state: State<'_, AppState>,
    id: WorkspaceId,
    name: String,
) -> WisResult<Workspace> {
    let current = read_state(&state);
    let next = current.with_renamed_workspace(id, name)?;
    let renamed = next.workspace(id)?.clone();

    replace_state(&state, next);
    Ok(renamed)
}

#[tauri::command]
pub fn delete_workspace(state: State<'_, AppState>, id: WorkspaceId) -> WisResult<()> {
    let current = read_state(&state);
    let next = current.with_workspace_removed(id)?;

    replace_state(&state, next);
    Ok(())
}

#[tauri::command]
pub fn set_active_workspace(state: State<'_, AppState>, id: WorkspaceId) -> WisResult<()> {
    let current = read_state(&state);
    let next = current.with_active_workspace(id)?;

    replace_state(&state, next);
    Ok(())
}

// -- tab mutations ----------------------------------------------------------

#[tauri::command]
pub fn create_tab(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    url: String,
) -> WisResult<Tab> {
    let current = read_state(&state);
    let (next, tab) = current.with_new_tab(workspace_id, &url)?;

    replace_state(&state, next);
    Ok(tab)
}

#[tauri::command]
pub fn close_tab(state: State<'_, AppState>, id: TabId) -> WisResult<()> {
    let current = read_state(&state);
    let next = current.with_tab_closed(id)?;

    replace_state(&state, next);
    Ok(())
}

#[tauri::command]
pub fn reorder_tabs(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    order: Vec<TabId>,
) -> WisResult<()> {
    let current = read_state(&state);
    let next = current.with_tab_order(workspace_id, order)?;

    replace_state(&state, next);
    Ok(())
}

#[tauri::command]
pub fn pin_tab(state: State<'_, AppState>, id: TabId, pinned: bool) -> WisResult<Tab> {
    let current = read_state(&state);
    let (next, tab) = current.with_tab_pinned(id, pinned)?;

    replace_state(&state, next);
    Ok(tab)
}

#[tauri::command]
pub fn set_active_tab(state: State<'_, AppState>, id: TabId) -> WisResult<()> {
    let current = read_state(&state);
    let next = current.with_active_tab(id)?;

    replace_state(&state, next);
    Ok(())
}

// -- settings ---------------------------------------------------------------

#[tauri::command]
pub fn update_settings(state: State<'_, AppState>, patch: UserSettingsPatch) -> UserSettings {
    let current = read_state(&state);
    let next = current.with_settings(patch);
    let settings = next.settings.clone();

    replace_state(&state, next);
    settings
}

#[cfg(test)]
mod tests {
    use super::*;
    use wis_core::{LayoutMode, ThemeMode};

    fn state() -> AppState {
        AppState::new_with_defaults()
    }

    fn snapshot(s: &AppState) -> BrowserState {
        s.browser.read().unwrap().clone()
    }

    #[test]
    fn creating_and_listing_a_workspace_round_trips_through_state() {
        let s = state();
        let before = snapshot(&s);
        assert_eq!(before.workspaces.len(), 1);

        let (next, _ws) = before.with_new_workspace("Side".into(), AccentColor::Sun);
        *s.browser.write().unwrap() = next;

        let after = snapshot(&s);
        assert_eq!(after.workspaces.len(), 2);
    }

    #[test]
    fn settings_patch_is_applied_through_state() {
        let s = state();
        let current = snapshot(&s);
        let next = current.with_settings(UserSettingsPatch {
            layout: Some(LayoutMode::TopTabs),
            theme: Some(ThemeMode::Dark),
            ..Default::default()
        });

        *s.browser.write().unwrap() = next;
        
        let after = snapshot(&s);
        assert_eq!(after.settings.layout, LayoutMode::TopTabs);
        assert_eq!(after.settings.theme, ThemeMode::Dark);
    }
}
