//! Multi-window: one OS window per workspace.
//!
//! New Window / New Private Window spawn a separate top-level `WebviewWindow`
//! (`win-<uuid>`) that loads the same chrome bundle as the `main` window. The
//! window is bound to its workspace in [`AppState::window_workspaces`] *before*
//! it is built, so the chrome's first `get_browser_state` already resolves to
//! that workspace (see [`commands::get_browser_state`](crate::commands::get_browser_state)).
//! Secondary windows match the `default` capability via the `win-*` glob
//! in `capabilities/default.json`.
//!
//! The chrome bundle is window-agnostic: it reads its own label from
//! `getCurrentWindow()` and routes tab/overlay/peek/widget creation to that
//! window, so a freshly opened window behaves exactly like `main` over its own workspace.

use std::time::Duration;

use tauri::utils::config::WindowEffectsConfig;
use tauri::utils::{WindowEffect, WindowEffectState};
use tauri::{AppHandle, Manager, State, TitleBarStyle, WebviewUrl, WebviewWindowBuilder};
use wis_core::{WisError, WisResult, WorkspaceId};

use crate::state::AppState;

/// How long to wait for the main thread to finish building the window before
/// giving up.
const BUILD_TIMEOUT: Duration = Duration::from_secs(10);

/// Label prefix of secondary chrome windows. The `win-*` capability glob is
/// keyed off this, so it must stay in sync with `capabilities/default.json`.
const WINDOW_PREFIX: &str = "win-";

/// Spawn a chrome window bound to `workspace_id` and return its label. The bind
/// is registered before the build so the chrome's first state read resolves to
/// this workspace; a failed build rolls the bind back.
pub async fn spawn_window(
    app: &AppHandle,
    state: &AppState,
    workspace_id: WorkspaceId,
) -> WisResult<String> {
    {
        let browser = state.browser.lock().await;
        if browser.workspace(workspace_id).is_err() {
            return Err(WisError::InvalidInput {
                message: format!("cannot open a window for unknown workspace {workspace_id}"),
            });
        }
    }

    let label = format!("{WINDOW_PREFIX}{}", uuid::Uuid::new_v4());
    state.set_window_workspace(&label, workspace_id);

    let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
    let app_for_build = app.clone();
    let label_for_build = label.clone();
    let dispatch = app.run_on_main_thread(move || {
        let built = WebviewWindowBuilder::new(
            &app_for_build,
            &label_for_build,
            WebviewUrl::App("index.html".into()),
        )
        .title("Wis")
        .inner_size(1280.0, 800.0)
        .min_inner_size(800.0, 600.0)
        .resizable(true)
        .transparent(true)
        .decorations(true)
        .title_bar_style(TitleBarStyle::Overlay)
        .hidden_title(true)
        .accept_first_mouse(true)
        .disable_drag_drop_handler()
        .effects(WindowEffectsConfig {
            effects: vec![WindowEffect::HudWindow],
            state: Some(WindowEffectState::Active),
            radius: Some(14.0),
            color: None,
        })
        .build();
        match built {
            Ok(window) => {
                #[cfg(target_os = "macos")]
                crate::disable_chrome_window_shadow(&window);

                #[cfg(not(target_os = "macos"))]
                let _ = &window;

                let app_for_event = app_for_build.clone();
                let label_for_event = label_for_build.clone();
                window.on_window_event(move |event| {
                    if matches!(event, tauri::WindowEvent::Destroyed) {
                        app_for_event
                            .state::<AppState>()
                            .clear_window_workspace(&label_for_event);
                    }
                });
                let _ = tx.send(Ok(()));
            }
            Err(err) => {
                let _ = tx.send(Err(err.to_string()));
            }
        }
    });

    let outcome = dispatch
        .map_err(|err| err.to_string())
        .and_then(|()| {
            rx.recv_timeout(BUILD_TIMEOUT)
                .map_err(|err| err.to_string())
        })
        .and_then(|res| res);

    if let Err(message) = outcome {
        state.clear_window_workspace(&label);
        return Err(WisError::InvalidInput {
            message: format!("new window build failed: {message}"),
        });
    }

    log::info!("opened window {label} for workspace {workspace_id}");
    Ok(label)
}

/// Open a new OS window showing an existing (free) workspace. Returns the new
/// window's label.
#[tauri::command]
pub async fn new_window(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
) -> WisResult<String> {
    spawn_window(&app, &state, workspace_id).await
}

/// Persistent workspaces that no window is currently showing, for the New Window
/// picker (one workspace per window, so an occupied workspace is excluded; the
/// calling window's own workspace is occupied too). Ephemeral/private workspaces
/// are never offered.
#[tauri::command]
pub async fn free_workspaces(state: State<'_, AppState>) -> WisResult<Vec<wis_core::Workspace>> {
    let occupied = state.occupied_workspaces();
    let browser = state.browser.lock().await;
    Ok(browser
        .workspaces
        .iter()
        .filter(|w| !w.ephemeral && !occupied.contains(&w.id))
        .cloned()
        .collect())
}
