//! Site usage: a once-a-second ticker that feeds [`wis_core::usage::UsageLedger`]
//! and flushes its credits to storage.

use std::collections::HashSet;
use std::time::Duration;

use chrono::{Local, NaiveDate, Timelike};
use tauri::{AppHandle, Emitter, Manager};
use wis_core::usage::{site_of, summarize, Focus, Observation, UsageSummary, PRESENT};
use wis_core::{BrowserState, WisError, WisResult, WorkspaceId};

use crate::state::AppState;

pub const USAGE_CHANGED_EVENT: &str = "usage-changed";
const FLUSH_EVERY_TICKS: u32 = 15;
const MAX_SUMMARY_DAYS: i64 = 400;

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut ticks = 0u32;
        loop {
            tick.tick().await;
            let observed = observe(&app).await;
            {
                let state = app.state::<AppState>();
                let mut ledger = state.usage.lock().expect("usage poisoned");
                match observed {
                    Some((observation, timeout)) => {
                        ledger.set_timeout(timeout);
                        ledger.record(&observation);
                    }
                    None => ledger.pause(),
                }
            }

            ticks = ticks.wrapping_add(1);
            if ticks % FLUSH_EVERY_TICKS == 0 {
                flush(&app).await;
            }
        }
    });
}

/// Totals for local days `from..=to` (`YYYY-MM-DD`) across workspaces.
#[tauri::command]
pub async fn usage_summary(
    state: tauri::State<'_, AppState>,
    from: String,
    to: String,
) -> WisResult<UsageSummary> {
    let parse = |raw: &str| {
        NaiveDate::parse_from_str(raw, "%Y-%m-%d").map_err(|_| WisError::InvalidInput {
            message: format!("not a date: {raw}"),
        })
    };
    let (from, to) = (parse(&from)?, parse(&to)?);
    let span = (to - from).num_days();
    if !(0..MAX_SUMMARY_DAYS).contains(&span) {
        return Err(WisError::InvalidInput {
            message: "usage range must run forward and span at most a year".to_string(),
        });
    }

    let storage = state.storage.clone();
    let rows = tokio::task::spawn_blocking(move || storage.usage(None, from, to))
        .await
        .map_err(|err| WisError::PersistenceFailed {
            reason: format!("usage read panicked: {err}"),
        })?
        .map_err(|err| WisError::PersistenceFailed {
            reason: err.to_string(),
        })?;

    Ok(summarize(&rows, from, to))
}

pub fn window_focused(app: &AppHandle, label: &str, focused: bool) {
    let state = app.state::<AppState>();
    let mut current = state
        .focused_window
        .lock()
        .expect("focused_window poisoned");
    if focused {
        *current = Some(label.to_string());
    } else if current.as_deref() == Some(label) {
        *current = None;
    }
}

/// Drops held, pending and stored usage a clear covers. Holding the flush
/// lock keeps an in-flight write from landing after the clear.
pub async fn clear(
    state: &AppState,
    workspace_id: Option<WorkspaceId>,
    from: Option<(NaiveDate, u8)>,
) -> WisResult<()> {
    let _io = state.usage_io.lock().await;
    state
        .usage
        .lock()
        .expect("usage poisoned")
        .forget(|credit| workspace_id.is_none_or(|ws| credit.workspace == ws));
    let storage = state.storage.clone();
    tokio::task::spawn_blocking(move || storage.clear_usage(workspace_id, from))
        .await
        .map_err(|err| WisError::PersistenceFailed {
            reason: format!("usage clear panicked: {err}"),
        })?
        .map_err(|err| WisError::PersistenceFailed {
            reason: err.to_string(),
        })
}

/// Synchronous, for app exit. A flush already writing covers the rest.
pub fn flush_now(state: &AppState) {
    let Ok(_io) = state.usage_io.try_lock() else {
        return;
    };
    let credits = state.usage.lock().expect("usage poisoned").take();
    if !credits.is_empty() {
        if let Err(err) = state.storage.add_usage(&credits) {
            log::warn!("usage flush on exit: {err}");
        }
    }
}

async fn flush(app: &AppHandle) {
    let state = app.state::<AppState>();
    let _io = state.usage_io.lock().await;
    let credits = state.usage.lock().expect("usage poisoned").take();
    if credits.is_empty() {
        return;
    }

    let storage = state.storage.clone();
    let batch = credits.clone();
    let failure = match tokio::task::spawn_blocking(move || storage.add_usage(&batch)).await {
        Ok(Ok(())) => {
            let _ = app.emit(USAGE_CHANGED_EVENT, ());
            return;
        }
        Ok(Err(err)) => err.to_string(),
        Err(err) => err.to_string(),
    };

    log::warn!("usage flush: {failure}");
    state.usage.lock().expect("usage poisoned").restore(credits);
}

/// `None` while tracking is off; otherwise the observation and the idle
/// timeout to apply.
async fn observe(app: &AppHandle) -> Option<(Observation, Duration)> {
    let state = app.state::<AppState>();
    let idle = crate::idle::since_input();
    let focused_label = if wis_in_front() {
        state
            .focused_window
            .lock()
            .expect("focused_window poisoned")
            .clone()
    } else {
        None
    };
    let audible = unmuted_audible(&state);
    let windows: Vec<(String, WorkspaceId)> = state
        .window_workspaces
        .lock()
        .expect("window_workspaces poisoned")
        .iter()
        .map(|(label, ws)| (label.clone(), *ws))
        .collect();
    let workspace_of = |label: &str| {
        windows
            .iter()
            .find(|(known, _)| known == label)
            .map(|(_, ws)| *ws)
    };

    let (timeout, focused) = {
        let browser = state.browser.lock().await;
        if !browser.settings.usage_tracking {
            return None;
        }

        let timeout = Duration::from_secs(browser.settings.usage_idle_timeout_secs.into());
        let focused = focused_label
            .as_deref()
            .and_then(workspace_of)
            .and_then(|ws| focused_site(&browser, ws, &audible));

        (timeout, focused)
    };

    let present = idle.is_none_or(|idle| idle <= PRESENT);
    let focused_counts = focused
        .as_ref()
        .is_some_and(|focus| focus.audible || present);
    let playing = if audible.is_empty() || focused_counts {
        None
    } else {
        let visible = visible_windows(
            app,
            windows.iter().map(|(label, _)| label.clone()).collect(),
        )
        .await;
        let browser = state.browser.lock().await;
        windows
            .iter()
            .filter(|(label, _)| visible.contains(label))
            .find_map(|(_, ws)| playing_site(&browser, *ws, &audible))
    };

    let now = Local::now();
    Some((
        Observation {
            day: now.date_naive(),
            hour: now.hour() as u8,
            idle,
            focused,
            playing,
        },
        timeout,
    ))
}

fn unmuted_audible(state: &AppState) -> HashSet<String> {
    let mut audible = state
        .audible_tabs
        .lock()
        .expect("audible_tabs poisoned")
        .clone();
    let muted = state.muted_tabs.lock().expect("muted_tabs poisoned");
    audible.retain(|tab| !muted.contains(tab));
    audible
}

/// The site in a workspace's focused pane.
fn focused_site(
    browser: &BrowserState,
    workspace: WorkspaceId,
    audible: &HashSet<String>,
) -> Option<Focus> {
    let ws = browser
        .workspace(workspace)
        .ok()
        .filter(|ws| !ws.ephemeral)?;
    tab_site(browser, workspace, ws.active_tab_id?, audible)
}

/// An audible site in any pane a workspace shows.
fn playing_site(
    browser: &BrowserState,
    workspace: WorkspaceId,
    audible: &HashSet<String>,
) -> Option<Focus> {
    let ws = browser
        .workspace(workspace)
        .ok()
        .filter(|ws| !ws.ephemeral)?;
    ws.displayed_tab_ids()
        .into_iter()
        .filter_map(|tab| tab_site(browser, workspace, tab, audible))
        .find(|focus| focus.audible)
}

/// Never for Wis pages or tabs showing an error page.
fn tab_site(
    browser: &BrowserState,
    workspace: WorkspaceId,
    tab: wis_core::TabId,
    audible: &HashSet<String>,
) -> Option<Focus> {
    let tab = browser.tab(tab).ok()?;
    if tab.load_error.is_some() {
        return None;
    }

    Some(Focus {
        workspace,
        site: site_of(&tab.url)?,
        audible: audible.contains(&tab.id.as_uuid().to_string()),
    })
}

/// Labels of windows that are shown and not minimised, asked on the main
/// thread so the runtime never blocks on it.
async fn visible_windows(app: &AppHandle, labels: Vec<String>) -> HashSet<String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let handle = app.clone();
    let asked = app.run_on_main_thread(move || {
        let visible = labels
            .into_iter()
            .filter(|label| {
                handle.get_window(label).is_some_and(|window| {
                    window.is_visible().unwrap_or(false) && !window.is_minimized().unwrap_or(true)
                })
            })
            .collect();
        let _ = tx.send(visible);
    });
    if asked.is_err() {
        return HashSet::new();
    }

    rx.await.unwrap_or_default()
}

/// Windows can miss a window's blur when a child webview held focus, so the
/// OS foreground window has the final say there.
#[cfg(windows)]
fn wis_in_front() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    let mut pid = 0u32;
    // SAFETY: plain window queries; `pid` is a valid out pointer for the call.
    unsafe {
        GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid));
    }
    pid == std::process::id()
}

#[cfg(not(windows))]
fn wis_in_front() -> bool {
    true
}
