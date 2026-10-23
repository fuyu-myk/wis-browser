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
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};

#[cfg(target_os = "macos")]
use tauri::TitleBarStyle;
use wis_core::{AccentColor, WisError, WisResult, WorkspaceId};

use crate::state::AppState;

/// How long to wait for the main thread to finish building the window before
/// giving up.
const BUILD_TIMEOUT: Duration = Duration::from_secs(10);

/// Label prefix of secondary chrome windows. The `win-*` capability glob is
/// keyed off this, so it must stay in sync with `capabilities/default.json`.
const WINDOW_PREFIX: &str = "win-";

/// The chrome window's glass treatment, per platform. macOS uses the HUD
/// material (the dark vibrancy the liquid-glass chrome is tuned against);
/// Windows 11 uses Mica (Windows 10 silently gets no glass, since
/// `window-vibrancy`'s `apply_mica` errors there and Tauri swallows it); other
/// platforms get no effect, leaving the transparent window to show the chrome's
/// own backdrop. `state`/`radius` are macOS knobs and are ignored elsewhere.
pub fn chrome_window_effects() -> WindowEffectsConfig {
    #[cfg(target_os = "macos")]
    let effects = vec![WindowEffect::HudWindow];
    #[cfg(windows)]
    let effects = vec![WindowEffect::Mica];
    #[cfg(not(any(target_os = "macos", windows)))]
    let effects: Vec<WindowEffect> = Vec::new();

    WindowEffectsConfig {
        effects,
        state: Some(WindowEffectState::Active),
        radius: Some(14.0),
        color: None,
    }
}

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
    // Restore this workspace's last window geometry, if any (else default size,
    // OS-placed).
    let geo = state.window_geometry_for(workspace_id);
    let dispatch = app.run_on_main_thread(move || {
        let mut builder = WebviewWindowBuilder::new(
            &app_for_build,
            &label_for_build,
            WebviewUrl::App("index.html".into()),
        )
        .title("Wis")
        .min_inner_size(800.0, 600.0)
        .resizable(true)
        .transparent(true)
        .decorations(cfg!(target_os = "macos"))
        .accept_first_mouse(true)
        .disable_drag_drop_handler()
        .effects(chrome_window_effects());

        #[cfg(target_os = "macos")]
        {
            builder = builder
                .title_bar_style(TitleBarStyle::Overlay)
                .hidden_title(true);
        }
        builder = match geo {
            Some(g) => builder.inner_size(g.width, g.height).position(g.x, g.y),
            None => builder.inner_size(1280.0, 800.0),
        };
        let built = builder.build();
        match built {
            Ok(window) => {
                #[cfg(target_os = "macos")]
                crate::disable_chrome_window_shadow(&window);

                #[cfg(windows)]
                if let Err(err) = window.hide_menu() {
                    log::warn!("hide native menu bar for {label_for_build} failed: {err}");
                }

                // Frame-level focus repair; see `win_focus`. Runs on the main
                // thread (this closure is a main-thread dispatch).
                #[cfg(windows)]
                if let Some(win) = app_for_build.get_window(&label_for_build) {
                    crate::platform::win_focus::install(&win);
                }

                let app_for_event = app_for_build.clone();
                let label_for_event = label_for_build.clone();
                window.on_window_event(move |event| match event {
                    tauri::WindowEvent::CloseRequested { .. } => {
                        crate::platform::webview_close::terminate_window_pages(
                            &app_for_event,
                            &label_for_event,
                        );
                    }
                    tauri::WindowEvent::Destroyed => {
                        let shown = app_for_event
                            .state::<AppState>()
                            .clear_window_workspace(&label_for_event);
                        if let Some(id) = shown {
                            crate::protection::private_store::release(&app_for_event, id);
                            crate::protection::uncloak::clear(&app_for_event.state(), id);
                            #[cfg(target_os = "macos")]
                            crate::extensions::release(&app_for_event, id);
                        }
                    }
                    tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => {
                        record_window_geometry(&app_for_event, &label_for_event);
                    }
                    _ => {}
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

/// Bring an existing window to the front (deminiaturize, then focus). Used when
/// an action would otherwise duplicate a workspace a window already shows.
///
/// Resolved via [`get_window`](Manager::get_window): these windows host child
/// webviews (tabs/overlays/panels added with `add_child`), so they are
/// multi-webview windows that [`get_webview_window`](Manager::get_webview_window)
/// does not return.
pub fn focus_window(app: &AppHandle, label: &str) {
    if let Some(win) = app.get_window(label) {
        let _ = win.unminimize();
        let _ = win.set_focus();
    } else {
        log::warn!("focus_window: no window labelled {label}");
    }
}

/// Snapshot a window's geometry (logical px) into the per-workspace cache and
/// schedule a debounced persist. Called from move/resize events (which fire in
/// bursts during a drag), so the write is coalesced via a generation counter:
/// only the last event of a burst reaches storage.
pub fn record_window_geometry(app: &AppHandle, label: &str) {
    use std::sync::atomic::Ordering;

    let state = app.state::<AppState>();
    let Some(workspace_id) = state.window_workspace(label) else {
        return;
    };

    let Some(win) = app.get_window(label) else {
        return;
    };

    let (Ok(scale), Ok(pos), Ok(size)) =
        (win.scale_factor(), win.outer_position(), win.inner_size())
    else {
        return;
    };

    let pos = pos.to_logical::<f64>(scale);
    let size = size.to_logical::<f64>(scale);
    state.set_window_geometry(
        workspace_id,
        crate::state::WindowGeometry {
            x: pos.x,
            y: pos.y,
            width: size.width,
            height: size.height,
        },
    );

    let generation = state.geometry_gen.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(700)).await;
        let state = app.state::<AppState>();
        if state.geometry_gen.load(Ordering::SeqCst) == generation {
            state.persist_window_geometry();
        }
    });
}

/// Open a new OS window showing an existing (free) workspace. Returns the new
/// window's label. One workspace per window: if another window already shows
/// `workspace_id` (e.g. the picker raced a sibling that just claimed it), focus
/// that window and return its label rather than duplicating the workspace.
#[tauri::command]
pub async fn new_window(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
) -> WisResult<String> {
    if let Some(owner) = state.window_for_workspace(workspace_id) {
        focus_window(&app, &owner);
        return Ok(owner);
    }

    spawn_window(&app, &state, workspace_id).await
}

/// Open `url` in a brand-new window of its own workspace. The workspace and
/// its first tab are created in one transition. A window that fails to build
/// will be removed by the rollback.
///
/// `private` makes the workspace ephemeral; a link from a private workspace
/// should pass `true` so it does not escape into a persisted one.
#[tauri::command]
pub async fn open_url_in_new_window(
    app: AppHandle,
    state: State<'_, AppState>,
    url: String,
    name: String,
    accent: AccentColor,
    private: bool,
) -> WisResult<String> {
    let workspace_id = crate::commands::mutate(&app, &state, |s| {
        let (next, workspace) = if private {
            s.with_new_private_workspace(name.clone(), accent)
        } else {
            s.with_new_workspace(name.clone(), accent)
        };
        let (next, _tab) = next.with_new_loose_tab(workspace.id, &url)?;
        Ok((next, workspace.id))
    })
    .await?;

    match spawn_window(&app, &state, workspace_id).await {
        Ok(label) => Ok(label),
        Err(err) => {
            if let Err(rollback) = crate::commands::mutate_unit_immediate(&app, &state, |s| {
                s.with_workspace_removed(workspace_id)
            })
            .await
            {
                log::warn!(
                    "open_url_in_new_window: rolling back {workspace_id} failed: {rollback:?}"
                );
            }
            Err(err)
        }
    }
}

/// Reopen the multi-window layout saved at the previous exit: a separate window
/// for each persisted workspace that had one, minus whatever a window already
/// shows (the main window's own, primarily). Runs **once per process** (guarded
/// by `session_restored`) so a chrome reload never re-spawns; the main window
/// calls it on boot. Ephemeral/private and deleted workspaces resolve to nothing
/// and are skipped.
#[tauri::command]
pub async fn restore_session_windows(app: AppHandle, state: State<'_, AppState>) -> WisResult<()> {
    use std::sync::atomic::Ordering;

    if state
        .session_restored
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(());
    }

    let saved = state.storage.load_windowed_workspaces().unwrap_or_default();

    for id in saved {
        if state.window_for_workspace(id).is_some() {
            continue;
        }

        let exists = {
            let browser = state.browser.lock().await;
            browser.workspace(id).map(|w| !w.ephemeral).unwrap_or(false)
        };
        if !exists {
            continue;
        }

        if let Err(err) = spawn_window(&app, &state, id).await {
            log::warn!("session restore: window for {id} failed: {err}");
        }
    }

    Ok(())
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
