//! Workspace hardening WebKit only reads from the configuration a webview is
//! created with: Lockdown Mode (macOS 14+) and fingerprinting protection
//! (macOS 13.3+). Each creation marks its label first and the configuration
//! hook, which runs later on the main thread, reads the mark. Every creation
//! sets or clears its label's mark, so the latest creation's intent wins.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};

use tauri::{AppHandle, Emitter, State};
use wis_core::{BrowserState, PrivacyMode, TabId, WisError, WisResult, Workspace, WorkspaceId};

use crate::state::AppState;

/// Tabs to rebuild through the sleep path.
pub const REBUILD_TABS_EVENT: &str = "wis-rebuild-tabs";
/// Hides the Peek overlay in the focused window.
const PEEK_CLOSE_EVENT: &str = "wis-peek-close";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Hardening {
    pub lockdown: bool,
    pub fingerprinting: bool,
}

impl Hardening {
    pub fn of(workspace: &Workspace) -> Self {
        if !cfg!(target_os = "macos") {
            return Self::default();
        }
        Self {
            lockdown: workspace.lockdown,
            fingerprinting: workspace.privacy_mode != PrivacyMode::Permissive,
        }
    }
}

static MARKED: LazyLock<Mutex<HashMap<String, Hardening>>> = LazyLock::new(Default::default);

fn marked() -> MutexGuard<'static, HashMap<String, Hardening>> {
    MARKED.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Read right before creating, after the creation's last await.
pub async fn wanted(state: &AppState, workspace: WorkspaceId) -> Hardening {
    state
        .browser
        .lock()
        .await
        .workspace(workspace)
        .map_or_else(|_| Hardening::default(), Hardening::of)
}

/// Before creating the webview labelled `label`.
pub fn mark(label: &str, hardening: Hardening) {
    if hardening == Hardening::default() {
        marked().remove(label);
    } else {
        marked().insert(label.to_owned(), hardening);
    }
}

/// For a label no later creation will reuse.
pub fn forget(label: &str) {
    marked().remove(label);
}

fn marked_for(label: &str) -> Hardening {
    marked().get(label).copied().unwrap_or_default()
}

/// For the configuration hook.
///
/// # Safety
///
/// Main thread only, with the configuration the webview is about to be
/// created from.
#[cfg(target_os = "macos")]
pub unsafe fn apply(label: &str, config: &objc2_web_kit::WKWebViewConfiguration) {
    let hardening = marked_for(label);
    if hardening == Hardening::default() {
        return;
    }
    let preferences = config.defaultWebpagePreferences();
    if hardening.lockdown {
        preferences.setLockdownModeEnabled(true);
    }
    if hardening.fingerprinting {
        let raw = objc2_06::rc::Retained::as_ptr(&preferences) as *mut objc2::runtime::AnyObject;
        crate::protection::site_protection::imp::set_fingerprinting_protection(raw, true);
    }
}

pub fn web_pages(state: &BrowserState, workspace: WorkspaceId) -> Vec<TabId> {
    state
        .tabs
        .iter()
        .filter(|tab| tab.workspace_id == workspace)
        .filter(|tab| matches!(tab.url.scheme(), "http" | "https" | "file"))
        .map(|tab| tab.id)
        .collect()
}

/// Closes the workspace's Peek and widget panels, which reopen under the new
/// hardening, and rebuilds its open pages.
pub fn rebuild_workspace(app: &AppHandle, state: &AppState, id: WorkspaceId, pages: Vec<TabId>) {
    let windows: Vec<String> = state
        .window_workspaces
        .lock()
        .expect("window_workspaces poisoned")
        .iter()
        .filter(|(_, ws)| **ws == id)
        .map(|(label, _)| label.clone())
        .collect();
    for window in &windows {
        crate::chrome::widget_panel::close_window_panels(app, window);
        crate::chrome::hover_guard::set_window_tabs_suppressed(window, false);
        if let Err(err) = crate::chrome::peek::close_window_peek(app, window) {
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
            log::warn!("hardening rebuild event failed: {err}");
        }
    }
}

/// Switches the workspace and rebuilds it under the new mode.
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
        let pages = web_pages(&next, id);
        Ok((next, (workspace, changed, pages)))
    })
    .await?;
    if changed {
        rebuild_workspace(&app, &state, id, pages);
    }

    Ok(workspace)
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use wis_core::AccentColor;

    const LOCKDOWN: Hardening = Hardening {
        lockdown: true,
        fingerprinting: false,
    };

    #[test]
    fn a_marked_label_stays_marked_until_a_creation_clears_it() {
        let label = "tab-00000000-0000-4000-8000-00000000lock";
        mark(label, LOCKDOWN);
        assert_eq!(marked_for(label), LOCKDOWN);
        assert_eq!(marked_for(label), LOCKDOWN);
        mark(label, Hardening::default());
        assert_eq!(marked_for(label), Hardening::default());
    }

    #[test]
    fn a_one_off_label_can_be_forgotten() {
        let label = "wis-fetch-00000000-0000-4000-8000-000000000001";
        mark(label, LOCKDOWN);
        forget(label);
        assert_eq!(marked_for(label), Hardening::default());
    }

    #[test]
    fn every_privacy_mode_but_permissive_resists_fingerprinting() {
        let workspace = Workspace::new("w".into(), AccentColor::Primary);
        let resists = |mode| Hardening::of(&workspace.with_privacy_mode(mode)).fingerprinting;
        assert!(resists(PrivacyMode::Strict));
        assert!(resists(PrivacyMode::Balanced));
        assert!(!resists(PrivacyMode::Permissive));
    }

    #[test]
    fn lockdown_follows_the_workspace_switch() {
        let workspace = Workspace::new("w".into(), AccentColor::Primary);
        assert!(!Hardening::of(&workspace).lockdown);
        assert!(Hardening::of(&workspace.with_lockdown(true)).lockdown);
    }
}
