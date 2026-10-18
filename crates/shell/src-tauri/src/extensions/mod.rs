//! Browser extensions (macOS 15.4+): installed from a folder or `.zip` after a
//! review, and run in every regular workspace's tabs through WebKit's
//! extension controller. Elsewhere the commands say they are unavailable.

#[cfg(target_os = "macos")]
mod adapters;
#[cfg(target_os = "macos")]
mod host;
#[cfg(target_os = "macos")]
mod install;
#[cfg(target_os = "macos")]
mod stage;

use serde::Serialize;
use tauri::{AppHandle, State};
use wis_core::extensions::ExtensionReview;
use wis_core::{WisError, WisResult};

use crate::state::AppState;

/// An extension copied in and waiting on its review.
#[derive(Debug, Clone, Serialize)]
pub struct StagedExtension {
    pub token: String,
    pub review: ExtensionReview,
}

#[cfg(not(target_os = "macos"))]
fn unavailable() -> WisError {
    WisError::InvalidInput {
        message: "extensions are not available on this platform yet".into(),
    }
}

#[cfg(target_os = "macos")]
fn require_support() -> WisResult<()> {
    if host::supported() {
        Ok(())
    } else {
        Err(WisError::InvalidInput {
            message: "extensions need macOS 15.4 or later".into(),
        })
    }
}

#[tauri::command]
pub fn extensions_supported() -> bool {
    #[cfg(target_os = "macos")]
    return host::supported();
    #[cfg(not(target_os = "macos"))]
    false
}

/// Asks for a folder or `.zip`, copies it in and returns its review; `None`
/// when the panel is cancelled.
#[tauri::command]
pub async fn extension_choose(app: AppHandle) -> WisResult<Option<StagedExtension>> {
    #[cfg(target_os = "macos")]
    {
        require_support()?;
        install::choose(app).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Err(unavailable())
    }
}

#[tauri::command]
pub async fn extension_install(
    app: AppHandle,
    state: State<'_, AppState>,
    token: String,
) -> WisResult<()> {
    #[cfg(target_os = "macos")]
    {
        require_support()?;
        install::install(app, state, token).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, state, token);
        Err(unavailable())
    }
}

#[tauri::command]
pub async fn extension_discard(token: String) -> WisResult<()> {
    #[cfg(target_os = "macos")]
    return install::discard(token).await;
    #[cfg(not(target_os = "macos"))]
    {
        let _ = token;
        Err(unavailable())
    }
}

#[tauri::command]
pub async fn extension_remove(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> WisResult<()> {
    #[cfg(target_os = "macos")]
    return install::remove(app, state, id).await;
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, state, id);
        Err(unavailable())
    }
}

/// The installed extensions' ids, never trusting the stored list for a path.
#[cfg(target_os = "macos")]
fn valid_ids(browser: &wis_core::state::BrowserState) -> Vec<String> {
    browser
        .settings
        .extensions
        .iter()
        .map(|e| e.id.clone())
        .filter(|id| wis_core::extensions::is_extension_id(id))
        .collect()
}

#[cfg(target_os = "macos")]
async fn installed_ids(state: &AppState) -> Vec<String> {
    valid_ids(&*state.browser.lock().await)
}

#[cfg(target_os = "macos")]
type Builder = tauri::webview::WebviewBuilder<tauri::Wry>;

#[cfg(any(target_os = "macos", test))]
fn tab_label(id: wis_core::TabId) -> String {
    format!("tab-{}", id.as_uuid())
}

/// Gives a regular workspace's new tab its extensions, when any are
/// installed; true when it did. Anything going wrong opens the tab without
/// them, short of losing the builder to a main thread that is gone.
#[cfg(target_os = "macos")]
pub(crate) async fn with_extensions(
    app: &AppHandle,
    state: &AppState,
    workspace: wis_core::WorkspaceId,
    proxy_port: u16,
    builder: Builder,
) -> WisResult<(Builder, bool)> {
    if !host::supported() {
        return Ok((builder, false));
    }

    let (wanted, active) = {
        let browser = state.browser.lock().await;
        let active = browser
            .workspaces
            .iter()
            .find(|w| w.id == workspace)
            .and_then(|w| w.active_tab_id)
            .map(tab_label);
        (valid_ids(&browser), active)
    };
    if wanted.is_empty() {
        return Ok((builder, false));
    }
    let Some(dir) = install::extensions_path(app) else {
        return Ok((builder, false));
    };

    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let ensured = app.run_on_main_thread(move || {
        host::ensure(workspace, proxy_port, dir, wanted, active, ready_tx);
    });
    if let Err(err) = ensured {
        log::warn!("extensions skipped for a tab: {err}");
        return Ok((builder, false));
    }
    let ready = tokio::time::timeout(std::time::Duration::from_secs(5), ready_rx).await;
    if matches!(ready, Ok(Ok(false)) | Ok(Err(_))) {
        return Ok((builder, false));
    }

    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let config = objc2_06::MainThreadMarker::new()
            .and_then(|mtm| host::tab_configuration(workspace, mtm));
        let _ = tx.send(match config {
            Some(config) => (builder.with_webview_configuration(config), true),
            None => (builder, false),
        });
    })
    .map_err(|err| WisError::InvalidInput {
        message: format!("run_on_main_thread: {err}"),
    })?;
    rx.await.map_err(|_| WisError::InvalidInput {
        message: "the tab builder was dropped".into(),
    })
}

/// Tells the workspace's extensions about a tab once its webview exists.
#[cfg(target_os = "macos")]
pub(crate) fn tab_opened(webview: &tauri::Webview, workspace: wis_core::WorkspaceId) {
    if !host::supported() {
        return;
    }
    let label = webview.label().to_string();
    if let Err(err) = webview.with_webview(move |w| host::open_tab(workspace, label, w.inner())) {
        log::warn!("extensions could not see a new tab: {err}");
    }
}

/// Called on the main thread as a tab's webview is torn down.
#[cfg(target_os = "macos")]
pub(crate) fn tab_closed(label: &str) {
    if host::supported() {
        host::close_tab(label);
    }
}

/// Stops a workspace's extensions with its window.
#[cfg(target_os = "macos")]
pub(crate) fn release(app: &AppHandle, workspace: wis_core::WorkspaceId) {
    if !host::LIVE.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    if let Err(err) = app.run_on_main_thread(move || host::release(workspace)) {
        log::warn!("extensions of {workspace} were not stopped: {err}");
    }
}

/// Each workspace's active tab, when an extension host is running and some
/// workspace's active tab changed.
#[cfg(target_os = "macos")]
pub(crate) fn changed_active_tabs(
    previous: &wis_core::state::BrowserState,
    next: &wis_core::state::BrowserState,
) -> Option<Vec<(wis_core::WorkspaceId, Option<String>)>> {
    if !host::LIVE.load(std::sync::atomic::Ordering::Relaxed) {
        return None;
    }
    active_tab_changes(previous, next)
}

#[cfg(any(target_os = "macos", test))]
fn active_tab_changes(
    previous: &wis_core::state::BrowserState,
    next: &wis_core::state::BrowserState,
) -> Option<Vec<(wis_core::WorkspaceId, Option<String>)>> {
    let before = |id| {
        previous
            .workspaces
            .iter()
            .find(|w| w.id == id)
            .map(|w| w.active_tab_id)
    };
    let changed: Vec<_> = next
        .workspaces
        .iter()
        .filter(|w| before(w.id) != Some(w.active_tab_id))
        .map(|w| (w.id, w.active_tab_id.map(tab_label)))
        .collect();
    (!changed.is_empty()).then_some(changed)
}

#[cfg(target_os = "macos")]
pub(crate) fn sync_active(app: &AppHandle, actives: Vec<(wis_core::WorkspaceId, Option<String>)>) {
    if let Err(err) = app.run_on_main_thread(move || host::set_active(actives)) {
        log::warn!("extensions could not follow the active tab: {err}");
    }
}

#[cfg(test)]
mod tests {
    use wis_core::state::BrowserState;

    use super::active_tab_changes;

    #[test]
    fn only_workspaces_whose_active_tab_moved_are_passed_on() {
        let before = BrowserState::new_first_run();
        assert_eq!(active_tab_changes(&before, &before), None);

        let mut after = before.clone();
        let workspace = after.workspaces[0].id;
        let tab = wis_core::TabId::new();
        after.workspaces[0].active_tab_id = Some(tab);
        assert_eq!(
            active_tab_changes(&before, &after),
            Some(vec![(workspace, Some(format!("tab-{}", tab.as_uuid())))])
        );
    }
}
