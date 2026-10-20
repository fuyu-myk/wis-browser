//! Browser extensions (macOS 15.4+): installed from a folder or `.zip` after a
//! review, and run in every regular workspace's tabs through WebKit's
//! extension controller. Elsewhere the commands say they are unavailable.

pub mod access;
#[cfg(target_os = "macos")]
mod actions;
#[cfg(target_os = "macos")]
mod adapters;
#[cfg(target_os = "macos")]
mod crx;
#[cfg(target_os = "macos")]
mod grants;
#[cfg(target_os = "macos")]
mod host;
#[cfg(target_os = "macos")]
mod install;
#[cfg(target_os = "macos")]
mod prompts;
#[cfg(target_os = "macos")]
mod stage;
pub mod store;

use serde::Serialize;
use tauri::{AppHandle, State};
use wis_core::extensions::ExtensionReview;
use wis_core::{WisError, WisResult};

use crate::state::AppState;

/// An extension's toolbar action for the active tab.
#[derive(Debug, Clone, Serialize)]
pub struct ExtensionAction {
    pub id: String,
    pub name: String,
    pub label: String,
    /// Empty when there is no badge.
    pub badge: String,
    pub enabled: bool,
    pub has_options: bool,
    /// A PNG data URL.
    pub icon: Option<String>,
    /// The active tab's host, when the extension runs only on allowed sites
    /// and wants this one.
    pub site: Option<SiteState>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SiteState {
    pub host: String,
    pub allowed: bool,
}

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

#[tauri::command]
pub async fn extension_set_workspace(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    workspace_id: wis_core::WorkspaceId,
    enabled: bool,
) -> WisResult<()> {
    #[cfg(target_os = "macos")]
    return install::change(app, state, move |list| {
        wis_core::extensions::with_workspace_switch(list, &id, workspace_id, enabled)
    })
    .await;
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, state, id, workspace_id, enabled);
        Err(unavailable())
    }
}

#[tauri::command]
pub async fn extension_set_pinned(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    pinned: bool,
) -> WisResult<()> {
    #[cfg(target_os = "macos")]
    return install::change(app, state, move |list| {
        wis_core::extensions::with_pinned(list, &id, pinned)
    })
    .await;
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, state, id, pinned);
        Err(unavailable())
    }
}

#[tauri::command]
pub async fn extension_set_private(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    allowed: bool,
) -> WisResult<()> {
    #[cfg(target_os = "macos")]
    return install::change(app, state, move |list| {
        wis_core::extensions::with_private_allowed(list, &id, allowed)
    })
    .await;
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, state, id, allowed);
        Err(unavailable())
    }
}

/// The actions of the extensions running in the calling window's workspace,
/// for its active tab, in installed order.
#[tauri::command]
pub async fn extension_actions(
    app: AppHandle,
    window: tauri::Window,
    state: State<'_, AppState>,
) -> WisResult<Vec<ExtensionAction>> {
    #[cfg(target_os = "macos")]
    {
        if !host::supported() {
            return Ok(Vec::new());
        }
        let Some(workspace) = state.window_workspace(window.label()) else {
            return Ok(Vec::new());
        };
        let ids: Vec<String> = {
            let browser = state.browser.lock().await;
            match browser.workspaces.iter().find(|w| w.id == workspace) {
                Some(w) => wis_core::extensions::wanted_in(&browser.settings.extensions, w),
                None => Vec::new(),
            }
        };
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let proxy_port = state
            .ensure_proxy_for(workspace)
            .await
            .map_err(WisError::from)?;
        ensure_host(&app, &state, workspace, proxy_port).await;
        on_main(&window, move |_| actions::list(workspace, &ids)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, window, state);
        Ok(Vec::new())
    }
}

/// Runs an extension's action on the active tab; a popup it opens anchors to
/// the button at `x, y, width, height` in window points.
#[tauri::command]
pub async fn extension_run(
    window: tauri::Window,
    state: State<'_, AppState>,
    id: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> WisResult<()> {
    #[cfg(target_os = "macos")]
    {
        require_support()?;
        if !wis_core::extensions::is_extension_id(&id)
            || ![x, y, width, height].iter().all(|n| n.is_finite())
        {
            return Err(WisError::InvalidInput {
                message: "not an extension action".into(),
            });
        }
        let workspace =
            state
                .window_workspace(window.label())
                .ok_or_else(|| WisError::InvalidInput {
                    message: "this window has no workspace".into(),
                })?;
        let dark = match state.browser.lock().await.settings.theme {
            wis_core::ThemeMode::Dark => Some(true),
            wis_core::ThemeMode::Light => Some(false),
            wis_core::ThemeMode::System => None,
        };
        let ran = on_main(&window, move |window| {
            window
                .ns_view()
                .is_ok_and(|view| actions::run(workspace, &id, view, (x, y, width, height), dark))
        })
        .await?;
        if ran {
            Ok(())
        } else {
            Err(WisError::InvalidInput {
                message: "that extension is not running in this workspace".into(),
            })
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, state, id, x, y, width, height);
        Err(unavailable())
    }
}

/// Closes an extension popup on screen. Names the extension whose popup the
/// click found open or just closed, so its button does not reopen it.
#[tauri::command]
pub async fn extension_dismiss_popup(window: tauri::Window) -> WisResult<Option<String>> {
    #[cfg(target_os = "macos")]
    {
        if !host::supported() {
            return Ok(None);
        }
        on_main(&window, |_| actions::dismiss_popup()).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        Ok(None)
    }
}

#[tauri::command]
pub async fn extension_open_options(
    window: tauri::Window,
    state: State<'_, AppState>,
    id: String,
) -> WisResult<()> {
    #[cfg(target_os = "macos")]
    {
        require_support()?;
        let workspace =
            state
                .window_workspace(window.label())
                .ok_or_else(|| WisError::InvalidInput {
                    message: "this window has no workspace".into(),
                })?;
        on_main(&window, move |_| match host::context_of(workspace, &id) {
            Some(context) => actions::open_options(&context),
            None => Err("turn the extension on in this workspace to open its options".into()),
        })
        .await?
        .map_err(|message| WisError::InvalidInput { message })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, state, id);
        Err(unavailable())
    }
}

/// Runs `work` on the main thread with the window, and waits for its answer.
#[cfg(target_os = "macos")]
async fn on_main<T: Send + 'static>(
    window: &tauri::Window,
    work: impl FnOnce(&tauri::Window) -> T + Send + 'static,
) -> WisResult<T> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let target = window.clone();
    window
        .run_on_main_thread(move || {
            let _ = tx.send(work(&target));
        })
        .map_err(|err| WisError::InvalidInput {
            message: format!("run_on_main_thread: {err}"),
        })?;
    rx.await.map_err(|_| WisError::InvalidInput {
        message: "the main thread dropped the request".into(),
    })
}

/// The extensions that run in each workspace, by id.
#[cfg(target_os = "macos")]
fn wanted_by_workspace(
    state: &wis_core::state::BrowserState,
) -> std::collections::HashMap<wis_core::WorkspaceId, Vec<String>> {
    state
        .workspaces
        .iter()
        .map(|w| {
            (
                w.id,
                wis_core::extensions::wanted_in(&state.settings.extensions, w),
            )
        })
        .collect()
}

#[cfg(target_os = "macos")]
type Builder = tauri::webview::WebviewBuilder<tauri::Wry>;

/// Tells each window which tabs to rebuild after the installed extensions
/// change, with the session each had.
#[cfg(target_os = "macos")]
pub const EXTENSIONS_CHANGED_EVENT: &str = "wis-extensions-changed";

/// Tabs to rebuild when the installed extensions change: the web pages of
/// the workspaces whose set of running extensions changed.
#[cfg(any(target_os = "macos", test))]
fn rebuild_targets(
    before: &[wis_core::extensions::InstalledExtension],
    state: &wis_core::state::BrowserState,
) -> Vec<wis_core::TabId> {
    use wis_core::extensions::wanted_in;
    let changed: std::collections::HashSet<_> = state
        .workspaces
        .iter()
        .filter(|w| wanted_in(before, w) != wanted_in(&state.settings.extensions, w))
        .map(|w| w.id)
        .collect();
    state
        .tabs
        .iter()
        .filter(|t| changed.contains(&t.workspace_id))
        .filter(|t| matches!(t.url.scheme(), "http" | "https"))
        .map(|t| t.id)
        .collect()
}

#[cfg(any(target_os = "macos", test))]
fn tab_label(id: wis_core::TabId) -> String {
    format!("tab-{}", id.as_uuid())
}

/// Starts the extensions that run in `workspace`, unless they are running;
/// true once they have loaded or are still loading.
#[cfg(target_os = "macos")]
async fn ensure_host(
    app: &AppHandle,
    state: &AppState,
    workspace: wis_core::WorkspaceId,
    proxy_port: u16,
) -> bool {
    if !host::supported() {
        return false;
    }

    let found = {
        let browser = state.browser.lock().await;
        browser
            .workspaces
            .iter()
            .find(|w| w.id == workspace)
            .map(|w| {
                (
                    wis_core::extensions::wanted_in(&browser.settings.extensions, w),
                    w.ephemeral,
                    w.active_tab_id.map(tab_label),
                    access::access_by_extension(&browser.settings.extensions),
                )
            })
    };
    let Some((wanted, private, active, grants)) = found else {
        return false;
    };
    if wanted.is_empty() {
        return false;
    }
    let Some(dir) = install::extensions_path(app) else {
        return false;
    };
    if !private && state.restored_cookies.lock().await.insert(workspace) {
        crate::cookies::restore_session_cookies(app, workspace).await;
    }

    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let ensured = app.run_on_main_thread(move || {
        grants::init(grants);
        host::ensure(
            workspace, private, proxy_port, dir, wanted, active, ready_tx,
        );
    });
    if let Err(err) = ensured {
        log::warn!("extensions not started: {err}");
        return false;
    }

    let ready = tokio::time::timeout(std::time::Duration::from_secs(5), ready_rx).await;
    !matches!(ready, Ok(Ok(false)) | Ok(Err(_)))
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
    if !ensure_host(app, state, workspace, proxy_port).await {
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

/// What changed about a tab, for the extensions watching it.
#[cfg(target_os = "macos")]
pub(crate) enum TabChange {
    Title,
    Started,
    Finished,
}

#[cfg(target_os = "macos")]
pub(crate) fn tab_changed(app: &AppHandle, label: &str, change: TabChange) {
    if !host::LIVE.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let label = label.to_string();
    if let Err(err) = app.run_on_main_thread(move || host::tab_changed(&label, change)) {
        log::warn!("extensions missed a tab change: {err}");
    }
}

#[cfg(target_os = "macos")]
static APP: std::sync::OnceLock<AppHandle> = std::sync::OnceLock::new();

/// Tells the windows an extension's action changed in a workspace, such as
/// its badge, so pinned buttons can redraw.
#[cfg(target_os = "macos")]
pub const ACTIONS_CHANGED_EVENT: &str = "wis-extension-actions-changed";

#[cfg(target_os = "macos")]
fn notify_actions_changed(workspace: wis_core::WorkspaceId) {
    use tauri::Emitter;

    if let Some(app) = APP.get() {
        if let Err(err) = app.emit(ACTIONS_CHANGED_EVENT, workspace) {
            log::warn!("extension action change not sent: {err}");
        }
    }
}

/// At launch: keeps the app for events, and removes extension folders no
/// longer installed, such as one whose removal could not delete it.
#[cfg(target_os = "macos")]
pub(crate) fn start(app: &AppHandle) {
    use tauri::Manager;

    let _ = APP.set(app.clone());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let installed = {
            let state = app.state::<AppState>();
            let browser = state.browser.lock().await;
            browser
                .settings
                .extensions
                .iter()
                .map(|e| e.id.clone())
                .collect()
        };
        install::sweep_orphans(app, installed).await;
    });
}

/// Clears a deleted regular workspace's extension data, when any extension
/// was ever installed.
#[cfg(target_os = "macos")]
pub(crate) fn forget_workspace(app: &AppHandle, workspace: wis_core::WorkspaceId) {
    if !host::supported() || !install::extensions_path(app).is_some_and(|d| d.is_dir()) {
        return;
    }
    let keep = std::collections::HashSet::new();
    if let Err(err) = app.run_on_main_thread(move || host::clear_data(vec![workspace], keep)) {
        log::warn!("extension data of {workspace} was not cleared: {err}");
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
    use wis_core::extensions::InstalledExtension;
    use wis_core::state::BrowserState;
    use wis_core::workspace::AccentColor;

    use super::{active_tab_changes, rebuild_targets};

    #[test]
    fn only_the_web_tabs_of_changed_workspaces_are_rebuilt() {
        let state = BrowserState::new_first_run();
        let regular = state.workspaces[0].id;
        let (state, web) = state.with_new_tab(regular, "https://example.com/").unwrap();
        let (state, chrome) = state.with_new_tab(regular, "about:settings").unwrap();
        let (state, canvas) = state
            .with_new_tab(regular, "wis-canvas://localhost/")
            .unwrap();
        let (state, private) =
            state.with_new_private_workspace("Private".into(), AccentColor::default());
        let (state, hidden) = state
            .with_new_tab(private.id, "https://example.org/")
            .unwrap();

        let ext = InstalledExtension {
            id: "0123456789abcdef0123456789abcdef".into(),
            ..Default::default()
        };
        let installed = state.with_extensions(vec![ext.clone()]);
        let targets = rebuild_targets(&[], &installed);
        assert!(targets.contains(&web.id));
        for skipped in [chrome.id, canvas.id, hidden.id] {
            assert!(!targets.contains(&skipped));
        }

        let allowed = installed.with_extensions(vec![InstalledExtension {
            allow_private: true,
            ..ext.clone()
        }]);
        assert_eq!(rebuild_targets(&[ext], &allowed), vec![hidden.id]);
    }

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
