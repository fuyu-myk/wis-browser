//! Debounced, per-tab persistence of WKWebView `interactionState` to disk.
//!
//! Mirrors [`crate::save_coalescer`] but keyed per tab: a finished page load
//! marks the tab dirty via [`TabSessionSaver::schedule`]; after a short quiet
//! period the dirty tabs' sessions (back/forward history + scroll + form state)
//! are read off the main thread and upserted. The seeded blob is replayed by
//! `create_tab_webview` (through [`AppState::pending_restore_states`]) on the next
//! launch, so a tab restores its full session across a full restart, not just
//! sleep/wake.
//!
//! Privacy: ephemeral (private) tabs never persist here. A tab that has
//! closed when its capture fires has any stale blob deleted instead, and
//! startup purges orphaned rows wholesale (see `lib.rs`).

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Manager, State};
use wis_core::{TabId, WisResult};

use crate::state::AppState;

/// Quiet period before a dirty tab's session is captured + written. Long enough
/// that a burst of in-page navigations coalesces to one read+write.
const DEBOUNCE: Duration = Duration::from_millis(1200);

/// Per-tab debounce: the set of tabs awaiting a session capture, plus a flag
/// guarding the single drain task. Lives on [`AppState`].
#[derive(Default)]
pub struct TabSessionSaver {
    dirty: Mutex<HashSet<TabId>>,
    scheduled: AtomicBool,
}

impl TabSessionSaver {
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark `tab_id` dirty and ensure the drain task is running. Cheap and
    /// idempotent: repeated calls within the quiet period coalesce.
    pub fn schedule(&self, app: AppHandle, tab_id: TabId) {
        self.dirty
            .lock()
            .expect("tab_session dirty poisoned")
            .insert(tab_id);
        if self.scheduled.swap(true, Ordering::SeqCst) {
            return;
        }

        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(DEBOUNCE).await;
                let state = app.state::<AppState>();
                let batch: Vec<TabId> = {
                    let mut guard = state
                        .tab_session_saver
                        .dirty
                        .lock()
                        .expect("tab_session dirty poisoned");
                    guard.drain().collect()
                };

                if batch.is_empty() {
                    state
                        .tab_session_saver
                        .scheduled
                        .store(false, Ordering::SeqCst);
                    // A writer may have queued between the drain and the store
                    // (it saw `scheduled` still true and didn't spawn); reclaim
                    // the drain if so.
                    let stranded = !state
                        .tab_session_saver
                        .dirty
                        .lock()
                        .expect("tab_session dirty poisoned")
                        .is_empty();
                    if stranded
                        && !state
                            .tab_session_saver
                            .scheduled
                            .swap(true, Ordering::SeqCst)
                    {
                        continue;
                    }

                    break;
                }

                for tab_id in batch {
                    capture_and_store(&app, state.inner(), tab_id).await;
                }
            }
        });
    }
}

/// Read a live tab's interaction state and upsert it, or delete the row when
/// the tab is gone or private. Best-effort: every failure path is a no-op.
async fn capture_and_store(app: &AppHandle, state: &AppState, tab_id: TabId) {
    let ephemeral = {
        let browser = state.browser.lock().await;
        match browser.tab(tab_id) {
            Ok(tab) => Some(
                browser
                    .workspace(tab.workspace_id)
                    .map(|w| w.ephemeral)
                    .unwrap_or(false),
            ),
            Err(_) => None,
        }
    };

    let storage = state.storage.clone();
    match ephemeral {
        // Closed or private: ensure no stale blob lingers on disk.
        None | Some(true) => {
            let _ = tokio::task::spawn_blocking(move || storage.delete_tab_session(tab_id)).await;
        }
        Some(false) => {
            let label = format!("tab-{}", tab_id.as_uuid());
            let Some(webview) = app.webviews().get(&label).cloned() else {
                return;
            };
            let Some(blob) = crate::interaction_state::read(&webview) else {
                return;
            };
            let _ =
                tokio::task::spawn_blocking(move || storage.save_tab_session(tab_id, &blob)).await;
        }
    }
}

/// Persist a session blob the chrome already read (just before discarding a
/// background webview on sleep), so a slept-then-quit tab restores its scrolled
/// position + history on next launch. Private/unknown tabs are skipped and any
/// stale blob dropped. Best-effort.
#[tauri::command]
pub async fn persist_tab_session(
    state: State<'_, AppState>,
    tab_id: TabId,
    state_b64: String,
) -> WisResult<()> {
    let ephemeral = {
        let browser = state.browser.lock().await;
        match browser.tab(tab_id) {
            Ok(tab) => browser
                .workspace(tab.workspace_id)
                .map(|w| w.ephemeral)
                .unwrap_or(false),
            Err(_) => true,
        }
    };

    let storage = state.storage.clone();
    let _ = tokio::task::spawn_blocking(move || {
        if ephemeral {
            storage.delete_tab_session(tab_id)
        } else {
            storage.save_tab_session(tab_id, &state_b64)
        }
    })
    .await;

    Ok(())
}
