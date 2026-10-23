//! Hover tab previews: downscaled page snapshots served to the preview card.
//!
//! Capture strategy (all three desktop engines; see
//! [`crate::pages::screenshot::preview_snapshot`] for the per-engine capture): the
//! reliable backbone is capture-on-switch-away (`stash`), mirroring
//! Chrome/Safari. On hover, a cached shot is served when present; otherwise
//! a live capture is attempted, which is exact for the active tab and
//! best-effort for hidden webviews.
//!
//! Shots also persist to `<app data>/previews/<tab-uuid>.png` so tabs
//! restored asleep from an earlier session still preview; orphaned files
//! are purged against the restored tab set at startup ([`purge_except`],
//! mirroring the tab-session purge).

use std::path::PathBuf;

#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
use tauri::Manager;
use tauri::{AppHandle, State};
use wis_core::{TabId, WisResult};

use crate::state::AppState;

/// Snapshot width in points (macOS scales at capture; Windows/Linux capture
/// then box-filter down).
#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
const PREVIEW_WIDTH: f64 = 320.0;

/// Cached previews beyond this count evict oldest-first (mirrors the
/// interaction-state sleep cache cap). Disk files are not evicted by count;
/// the startup purge bounds them to live tabs.
const CACHE_CAP: usize = 30;

/// On-disk preview directory (`<app data>/previews`).
pub fn previews_dir(app: &AppHandle) -> Option<PathBuf> {
    use tauri::Manager;
    app.path().app_data_dir().ok().map(|d| d.join("previews"))
}

fn preview_path(app: &AppHandle, id: TabId) -> Option<PathBuf> {
    previews_dir(app).map(|d| d.join(format!("{}.png", id.as_uuid())))
}

/// Delete preview files whose tab no longer exists (startup, against the
/// restored tab set).
pub fn purge_except(dir: &std::path::Path, keep: &[TabId]) {
    let keep: std::collections::HashSet<String> = keep
        .iter()
        .map(|id| format!("{}.png", id.as_uuid()))
        .collect();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if !keep.contains(name) {
            if let Err(err) = std::fs::remove_file(entry.path()) {
                log::debug!("preview purge {name}: {err}");
            }
        }
    }
}

/// Whether a tab may mirror its preview to disk: never for tabs in
/// ephemeral (private) workspaces, whose page content must not leave
/// memory. Unknown tabs default to not persisting.
async fn may_persist(state: &AppState, id: TabId) -> bool {
    let browser = state.browser.lock().await;
    browser
        .tab(id)
        .ok()
        .and_then(|tab| browser.workspace(tab.workspace_id).ok())
        .map(|ws| !ws.ephemeral)
        .unwrap_or(false)
}

fn store(app: &AppHandle, state: &AppState, id: TabId, png: Vec<u8>, persist: bool) {
    {
        let mut cache = state.tab_previews.lock().expect("tab_previews poisoned");
        cache.retain(|(cached, _)| *cached != id);
        cache.push((id, png.clone()));

        if cache.len() > CACHE_CAP {
            cache.remove(0);
        }
    }
    if !persist {
        return;
    }

    // Best-effort disk mirror so restored-asleep tabs preview next session.
    if let Some(path) = preview_path(app, id) {
        tauri::async_runtime::spawn_blocking(move || {
            if let Some(dir) = path.parent() {
                if let Err(err) = std::fs::create_dir_all(dir) {
                    log::debug!("preview dir: {err}");
                    return;
                }
            }

            if let Err(err) = std::fs::write(&path, &png) {
                log::debug!("preview write {}: {err}", path.display());
            }
        });
    }
}

fn cached(state: &AppState, id: TabId) -> Option<Vec<u8>> {
    let cache = state.tab_previews.lock().expect("tab_previews poisoned");
    cache
        .iter()
        .find(|(cached, _)| *cached == id)
        .map(|(_, png)| png.clone())
}

/// Memory cache first, then the on-disk mirror (hydrating memory on a hit).
async fn cached_or_disk(app: &AppHandle, state: &AppState, id: TabId) -> Option<Vec<u8>> {
    if let Some(png) = cached(state, id) {
        return Some(png);
    }

    let path = preview_path(app, id)?;
    let png = tauri::async_runtime::spawn_blocking(move || std::fs::read(&path).ok())
        .await
        .ok()
        .flatten()?;
    {
        let mut cache = state.tab_previews.lock().expect("tab_previews poisoned");
        cache.retain(|(cached, _)| *cached != id);
        cache.push((id, png.clone()));

        if cache.len() > CACHE_CAP {
            cache.remove(0);
        }
    }

    Some(png)
}

fn data_url(png: &[u8]) -> String {
    use base64::Engine;
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    )
}

/// Capture a tab into the preview cache. Fired just before its webview is
/// hidden (switch-away) or closed (sleep); best-effort.
#[tauri::command]
pub async fn stash_tab_preview(
    app: AppHandle,
    state: State<'_, AppState>,
    id: TabId,
) -> WisResult<()> {
    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    {
        let label = format!("tab-{}", id.as_uuid());
        let Some(webview) = app.webviews().get(&label).cloned() else {
            return Ok(());
        };
        let persist = may_persist(&state, id).await;
        match crate::pages::screenshot::preview_snapshot(&webview, PREVIEW_WIDTH).await {
            Ok(png) => store(&app, &state, id, png, persist),
            Err(err) => log::debug!("tab preview stash for {id} failed: {err}"),
        }
    }

    #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
    let _ = (app, state, id);

    Ok(())
}

/// The preview to show on hover, as a PNG data URL; `None` renders the
/// placeholder card. Active tab: always a fresh capture. Hidden tab: the
/// cached shot (memory, then disk), else a best-effort live capture
/// (outcome logged).
#[tauri::command]
pub async fn capture_tab_preview(
    app: AppHandle,
    state: State<'_, AppState>,
    id: TabId,
) -> WisResult<Option<String>> {
    #[cfg(any(target_os = "macos", windows, target_os = "linux"))]
    {
        let (is_active, persist) = {
            let browser = state.browser.lock().await;
            let is_active = browser.active_workspace().active_tab_id == Some(id);
            let persist = browser
                .tab(id)
                .ok()
                .and_then(|tab| browser.workspace(tab.workspace_id).ok())
                .map(|ws| !ws.ephemeral)
                .unwrap_or(false);
            (is_active, persist)
        };

        let label = format!("tab-{}", id.as_uuid());
        let webview = app.webviews().get(&label).cloned();

        if is_active {
            if let Some(webview) = &webview {
                if let Ok(png) =
                    crate::pages::screenshot::preview_snapshot(webview, PREVIEW_WIDTH).await
                {
                    let url = data_url(&png);
                    store(&app, &state, id, png, persist);
                    return Ok(Some(url));
                }
            }
        }

        if let Some(png) = cached_or_disk(&app, &state, id).await {
            return Ok(Some(data_url(&png)));
        }

        if let Some(webview) = &webview {
            match crate::pages::screenshot::preview_snapshot(webview, PREVIEW_WIDTH).await {
                Ok(png) => {
                    let url = data_url(&png);
                    store(&app, &state, id, png, persist);
                    return Ok(Some(url));
                }
                Err(err) => log::info!("hidden tab preview capture for {id} failed: {err}"),
            }
        }

        Ok(None)
    }

    #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
    {
        let _ = (app, state, id);
        Ok(None)
    }
}

/// Drop a closed tab's cached preview (memory + disk).
pub fn forget(app: &AppHandle, state: &AppState, id: TabId) {
    {
        let mut cache = state.tab_previews.lock().expect("tab_previews poisoned");
        cache.retain(|(cached, _)| *cached != id);
    }
    if let Some(path) = preview_path(app, id) {
        tauri::async_runtime::spawn_blocking(move || {
            if let Err(err) = std::fs::remove_file(&path) {
                log::debug!("preview remove {}: {err}", path.display());
            }
        });
    }
}
