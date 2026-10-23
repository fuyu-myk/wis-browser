//! Where extensions may run and what they were allowed while running: the
//! commands that change it, and saving what the main thread decided.

use tauri::{AppHandle, State};
use wis_core::extensions::access::SiteAccess;
#[cfg(target_os = "macos")]
use wis_core::extensions::installed::InstalledExtension;
#[cfg(target_os = "macos")]
use wis_core::WisError;
use wis_core::WisResult;

use crate::state::AppState;

#[cfg(not(target_os = "macos"))]
use super::unavailable;
#[cfg(target_os = "macos")]
use super::{grants, host, install, on_main, prompts, APP};

#[tauri::command]
pub async fn extension_set_site_access(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    access: SiteAccess,
) -> WisResult<()> {
    #[cfg(target_os = "macos")]
    return install::change(app, state, move |list| {
        wis_core::extensions::access::with_site_access(list, &id, access)
    })
    .await;
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, state, id, access);
        Err(unavailable())
    }
}

/// Allows or stops an extension on a host in the saved settings.
#[tauri::command]
pub async fn extension_set_site(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    host: String,
    allowed: bool,
) -> WisResult<()> {
    #[cfg(target_os = "macos")]
    return install::change(app, state, move |list| {
        wis_core::extensions::access::with_site(list, &id, &host, allowed)
    })
    .await;
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, state, id, host, allowed);
        Err(unavailable())
    }
}

/// Allows or stops an extension on a host from the toolbar: saved, except in a
/// private workspace, where it lasts as long as the workspace.
#[tauri::command]
pub async fn extension_set_site_here(
    app: AppHandle,
    window: tauri::Window,
    state: State<'_, AppState>,
    id: String,
    host: String,
    allowed: bool,
) -> WisResult<()> {
    #[cfg(target_os = "macos")]
    {
        let workspace =
            state
                .window_workspace(window.label())
                .ok_or_else(|| WisError::InvalidInput {
                    message: "this window has no workspace".into(),
                })?;
        let host = wis_core::extensions::access::plain_host(&host).ok_or_else(|| {
            WisError::InvalidInput {
                message: "not a site".into(),
            }
        })?;
        let (private, saved) = {
            let browser = state.browser.lock().await;
            (
                browser
                    .workspaces
                    .iter()
                    .any(|w| w.id == workspace && w.ephemeral),
                browser
                    .settings
                    .extensions
                    .iter()
                    .any(|e| e.id == id && e.allowed_sites.contains(&host)),
            )
        };
        if !private || (saved && !allowed) {
            return extension_set_site(app, state, id, host, allowed).await;
        }

        on_main(&window, move |_| match host::context_of(workspace, &id) {
            Some(context) => grants::set_session_site(&context, &id, &host, allowed),
            None => log::warn!("extension {id} is not running in this workspace"),
        })
        .await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, window, state, id, host, allowed);
        Err(unavailable())
    }
}

/// Answers a running extension's request for more access.
#[tauri::command]
pub async fn extension_answer_permission(
    window: tauri::Window,
    state: State<'_, AppState>,
    request: u64,
    allow: bool,
) -> WisResult<()> {
    #[cfg(target_os = "macos")]
    {
        let workspace =
            state
                .window_workspace(window.label())
                .ok_or_else(|| WisError::InvalidInput {
                    message: "this window has no workspace".into(),
                })?;
        on_main(&window, move |_| prompts::answer(request, workspace, allow)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, state, request, allow);
        Err(unavailable())
    }
}

/// What an extension was allowed while running, in words.
#[tauri::command]
pub async fn extension_granted_abilities(
    state: State<'_, AppState>,
    id: String,
) -> WisResult<Vec<String>> {
    let browser = state.browser.lock().await;
    Ok(browser
        .settings
        .extensions
        .iter()
        .find(|e| e.id == id)
        .map(wis_core::extensions::access::granted_abilities)
        .unwrap_or_default())
}

/// Takes back everything an extension was allowed while running.
#[tauri::command]
pub async fn extension_revoke_granted(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> WisResult<()> {
    #[cfg(target_os = "macos")]
    return install::change(app, state, move |list| {
        let all = list
            .iter()
            .find(|e| e.id == id)
            .map(|e| e.granted.clone())
            .unwrap_or_default();
        wis_core::extensions::access::without_granted(list, &id, &all)
    })
    .await;
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, state, id);
        Err(unavailable())
    }
}

/// Where each extension may run and what it was allowed while running, by id.
#[cfg(target_os = "macos")]
pub(super) fn access_by_extension(
    list: &[InstalledExtension],
) -> std::collections::HashMap<String, grants::Access> {
    list.iter()
        .map(|e| (e.id.clone(), grants::Access::of(e)))
        .collect()
}

#[cfg(target_os = "macos")]
pub(super) fn remember_granted(id: String, items: Vec<String>) {
    edit_later(move |list| wis_core::extensions::access::with_granted(list, &id, &items));
}

#[cfg(target_os = "macos")]
pub(super) fn forget_granted(id: String, items: Vec<String>) {
    edit_later(move |list| wis_core::extensions::access::without_granted(list, &id, &items));
}

/// Saves a change to the installed extensions made from the main thread.
#[cfg(target_os = "macos")]
fn edit_later(
    edit: impl FnOnce(&[InstalledExtension]) -> WisResult<Vec<InstalledExtension>> + Send + 'static,
) {
    use tauri::Manager;

    let Some(app) = APP.get().cloned() else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        if let Err(err) = install::change(app.clone(), state, edit).await {
            log::warn!("an extension's access was not saved: {err}");
        }
    });
}
