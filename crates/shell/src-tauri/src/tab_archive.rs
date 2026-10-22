//! Closes tabs left idle past `UserSettings::tab_archive_after` into Recently
//! closed ([`wis_core::tab_archive`]), sparing tabs playing audio or popped out.

use std::collections::HashSet;
use std::time::Duration;

use tauri::{AppHandle, Manager};
use wis_core::TabId;

use crate::state::AppState;

const FIRST_SWEEP: Duration = Duration::from_secs(60);
const SWEEP_EVERY: Duration = Duration::from_secs(10 * 60);

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_SWEEP).await;
        loop {
            sweep(&app).await;
            tokio::time::sleep(SWEEP_EVERY).await;
        }
    });
}

/// One transition closes every tab still idle under the lock, so a tab shown
/// meanwhile stays, and the windows redraw once.
async fn sweep(app: &AppHandle) {
    let state = app.state::<AppState>();
    let spared = spared(&state);
    let now = chrono::Utc::now();
    let any = {
        let browser = state.browser.lock().await;
        browser
            .idle_tabs(now, browser.settings.tab_archive_after)
            .iter()
            .any(|id| !spared.contains(id))
    };
    if !any {
        return;
    }

    let archived = crate::commands::mutate_immediate(app, &state, |s| {
        s.with_idle_tabs_closed(now, s.settings.tab_archive_after, &spared)
    })
    .await;
    let archived = match archived {
        Ok(archived) if !archived.is_empty() => archived,
        Ok(_) => return,
        Err(err) => {
            log::warn!("archiving idle tabs failed: {err}");
            return;
        }
    };
    crate::commands::record_archived(app, &state, &archived).await;

    for tab in &archived {
        crate::commands::forget_tab_state(app, &state, tab.tab.id).await;
    }
    log::info!("archived {} idle tabs", archived.len());
}

fn spared(state: &AppState) -> HashSet<TabId> {
    let popped = state
        .video_popout
        .lock()
        .expect("video_popout poisoned")
        .active
        .as_ref()
        .and_then(|popout| popout.label.strip_prefix("tab-").map(str::to_owned));
    state
        .audible_tabs
        .lock()
        .expect("audible_tabs poisoned")
        .iter()
        .cloned()
        .chain(popped)
        .filter_map(|uuid| uuid::Uuid::parse_str(&uuid).ok())
        .map(TabId::from_uuid)
        .collect()
}
