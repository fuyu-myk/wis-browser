//! Lockdown Mode for a workspace's pages (macOS 14+). WebKit reads it from the
//! configuration a webview is created with, so each creation marks its label
//! first and the configuration hook, which runs later on the main thread,
//! reads the mark. Every creation sets or clears its label's mark, so the
//! latest creation's intent wins.

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};

use tauri::{AppHandle, Emitter, State};
use wis_core::{TabId, WisError, WisResult, Workspace, WorkspaceId};

use crate::state::AppState;

/// Tabs to rebuild through the sleep path.
pub const REBUILD_TABS_EVENT: &str = "wis-rebuild-tabs";
/// Hides the Peek overlay in the focused window.
const PEEK_CLOSE_EVENT: &str = "wis-peek-close";

static MARKED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);

fn marked() -> MutexGuard<'static, HashSet<String>> {
    MARKED.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Read right before creating, after the creation's last await.
pub async fn wanted(state: &AppState, workspace: WorkspaceId) -> bool {
    state
        .browser
        .lock()
        .await
        .workspace(workspace)
        .is_ok_and(|w| w.lockdown)
}

/// Before creating the webview labelled `label`.
pub fn mark(label: &str, lockdown: bool) {
    if !cfg!(target_os = "macos") {
        return;
    }
    if lockdown {
        marked().insert(label.to_owned());
    } else {
        marked().remove(label);
    }
}

/// For a label no later creation will reuse.
pub fn forget(label: &str) {
    marked().remove(label);
}

fn is_marked(label: &str) -> bool {
    marked().contains(label)
}

/// For the configuration hook.
///
/// # Safety
///
/// Main thread only, with the configuration the webview is about to be
/// created from.
#[cfg(target_os = "macos")]
pub unsafe fn apply(label: &str, config: &objc2_web_kit::WKWebViewConfiguration) {
    if is_marked(label) {
        config
            .defaultWebpagePreferences()
            .setLockdownModeEnabled(true);
    }
}

/// Switches the workspace, rebuilds its open pages and closes its Peek and
/// widget panels, which reopen under the new mode.
#[tauri::command]
pub async fn set_workspace_lockdown(
    app: AppHandle,
    state: State<'_, AppState>,
    id: WorkspaceId,
    enabled: bool,
) -> WisResult<Workspace> {
    if !cfg!(target_os = "macos") {
        return Err(WisError::InvalidInput {
            message: "Lockdown Mode needs macOS".into(),
        });
    }

    let (workspace, changed, pages) = crate::commands::mutate(&app, &state, |s| {
        let changed = s.workspace(id)?.lockdown != enabled;
        let next = s.with_workspace_lockdown(id, enabled)?;
        let workspace = next.workspace(id)?.clone();
        let pages: Vec<TabId> = next
            .tabs
            .iter()
            .filter(|tab| tab.workspace_id == id)
            .filter(|tab| matches!(tab.url.scheme(), "http" | "https" | "file"))
            .map(|tab| tab.id)
            .collect();
        Ok((next, (workspace, changed, pages)))
    })
    .await?;
    if !changed {
        return Ok(workspace);
    }

    let windows: Vec<String> = state
        .window_workspaces
        .lock()
        .expect("window_workspaces poisoned")
        .iter()
        .filter(|(_, ws)| **ws == id)
        .map(|(label, _)| label.clone())
        .collect();
    for window in &windows {
        crate::widget_panel::close_window_panels(&app, window);
        crate::hover_guard::set_window_tabs_suppressed(window, false);
        if let Err(err) = crate::peek::close_window_peek(&app, window) {
            log::warn!("{err}");
        }
    }

    if !windows.is_empty() {
        if let Err(err) = app.emit(PEEK_CLOSE_EVENT, ()) {
            log::warn!("peek close event failed: {err}");
        }
    }
    if !pages.is_empty() {
        if let Err(err) = app.emit(REBUILD_TABS_EVENT, pages) {
            log::warn!("lockdown rebuild event failed: {err}");
        }
    }

    Ok(workspace)
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn a_marked_label_stays_marked_until_a_creation_clears_it() {
        let label = "tab-00000000-0000-4000-8000-00000000lock";
        mark(label, true);
        assert!(is_marked(label));
        assert!(is_marked(label));
        mark(label, false);
        assert!(!is_marked(label));
    }

    #[test]
    fn a_one_off_label_can_be_forgotten() {
        let label = "wis-fetch-00000000-0000-4000-8000-000000000001";
        mark(label, true);
        forget(label);
        assert!(!is_marked(label));
    }
}
