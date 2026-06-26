//! Per-tab audio state: which tabs are audibly playing, and page-level mute.
//!
//! Detection polls WebKit's `_isPlayingAudio` (guarded SPI, the same signal
//! Safari's tab audio icon uses) every couple of seconds across live tab
//! webviews: unlike script-side media tracking it also catches iframe
//! players and Web Audio. Muting uses `_setPageMuted:` (guarded SPI), which
//! silences the whole page engine-side, so a site cannot un-mute itself.
//!
//! Mute state is session-scoped and lives on [`AppState`] keyed by tab uuid
//! (and widget id for widget panels), so it survives the webview teardown
//! and recreation of tab sleep/wake and widget panel reclaim:
//! `create_tab_webview` / `open_widget_panel` re-apply it at creation.
//! On any change the full snapshot is broadcast as [`AUDIO_EVENT`] and the
//! chrome re-renders its tab audio buttons.

use std::collections::HashSet;

use tauri::{AppHandle, Emitter, Manager, State};
use wis_core::{TabId, WisResult};

use crate::state::AppState;

/// Broadcast whenever a tab's audibility or mute state changes:
/// `{ audible: [uuid], muted: [uuid] }`.
const AUDIO_EVENT: &str = "wis-tab-audio";

const TAB_LABEL_PREFIX: &str = "tab-";

/// How often live tab webviews are asked whether they are playing audio.
const POLL_SECS: u64 = 2;

#[derive(serde::Serialize)]
pub struct AudioState {
    pub audible: Vec<String>,
    pub muted: Vec<String>,
}

/// Mute or unmute every sound a webview's page can make (media elements,
/// iframes, Web Audio). Silently skipped when the SPI is unavailable.
pub fn apply_page_muted<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>, muted: bool) {
    #[cfg(target_os = "macos")]
    {
        let _ = webview.with_webview(move |w| unsafe {
            use objc2::runtime::AnyObject;
            use objc2::{msg_send, sel};

            let wk = w.inner() as *mut AnyObject;
            if wk.is_null() {
                return;
            }
            let responds: bool = msg_send![wk, respondsToSelector: sel!(_setPageMuted:)];
            if !responds {
                log::warn!("audio: _setPageMuted: unavailable; mute skipped");
                return;
            }
            // _WKMediaMutedState: bit 0 mutes audio output.
            let mask: usize = if muted { 1 } else { 0 };
            let _: () = msg_send![wk, _setPageMuted: mask];
        });
    }

    // Engine-side mute via WebView2 (media elements, iframes, Web Audio);
    // a page cannot un-mute itself.
    #[cfg(windows)]
    crate::win_webview2::with_core_webview2(webview, move |core| {
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_8;
        use windows::core::Interface;
        if let Ok(c8) = core.cast::<ICoreWebView2_8>() {
            // SAFETY: runs on the UI thread via `with_core_webview2`.
            let _ = unsafe { c8.SetIsMuted(muted) };
        }
    });

    #[cfg(not(any(target_os = "macos", windows)))]
    let _ = (webview, muted);
}

fn emit_snapshot(app: &AppHandle) {
    let state = app.state::<AppState>();
    let audible: Vec<String> = {
        let guard = state.audible_tabs.lock().expect("audible_tabs poisoned");
        guard.iter().cloned().collect()
    };
    let muted: Vec<String> = {
        let guard = state.muted_tabs.lock().expect("muted_tabs poisoned");
        guard.iter().cloned().collect()
    };
    let _ = app.emit(
        AUDIO_EVENT,
        serde_json::json!({ "audible": audible, "muted": muted }),
    );
}

/// Mute or unmute a tab (page-level). Applies to the live webview when one
/// exists; the state is re-applied on webview recreation (sleep wake).
#[tauri::command]
pub async fn set_tab_muted(
    app: AppHandle,
    state: State<'_, AppState>,
    id: TabId,
    muted: bool,
) -> WisResult<()> {
    let uuid = id.as_uuid().to_string();
    {
        let mut set = state.muted_tabs.lock().expect("muted_tabs poisoned");
        if muted {
            set.insert(uuid.clone());
        } else {
            set.remove(&uuid);
        }
    }

    if let Some(webview) = app
        .webviews()
        .get(&format!("{TAB_LABEL_PREFIX}{uuid}"))
        .cloned()
    {
        apply_page_muted(&webview, muted);
    }
    emit_snapshot(&app);

    Ok(())
}

/// Current audible + muted tab uuids, for the chrome's initial render.
#[tauri::command]
pub async fn tab_audio_state(state: State<'_, AppState>) -> WisResult<AudioState> {
    let audible = {
        let guard = state.audible_tabs.lock().expect("audible_tabs poisoned");
        guard.iter().cloned().collect()
    };
    let muted = {
        let guard = state.muted_tabs.lock().expect("muted_tabs poisoned");
        guard.iter().cloned().collect()
    };

    Ok(AudioState { audible, muted })
}

/// Forget a closed tab's audio bookkeeping.
pub fn forget_tab(app: &AppHandle, id: TabId) {
    let uuid = id.as_uuid().to_string();
    let state = app.state::<AppState>();
    let removed_muted = state
        .muted_tabs
        .lock()
        .expect("muted_tabs poisoned")
        .remove(&uuid);
    let removed_audible = state
        .audible_tabs
        .lock()
        .expect("audible_tabs poisoned")
        .remove(&uuid);

    if removed_muted || removed_audible {
        emit_snapshot(app);
    }
}

/// Start the audibility poll. Each tick prunes entries whose webview is gone
/// (closed or slept) and asks every live tab webview whether it is playing
/// audio, broadcasting a snapshot only when something changed.
pub fn start_poll(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(POLL_SECS));
        tick.tick().await; // consume the immediate first tick

        loop {
            tick.tick().await;
            poll_once(&app);
        }
    });
}

fn poll_once(app: &AppHandle) {
    let labels: Vec<String> = app
        .webviews()
        .keys()
        .filter(|l| l.starts_with(TAB_LABEL_PREFIX))
        .cloned()
        .collect();

    // A slept or closed tab cannot be audible.
    let live: HashSet<&str> = labels
        .iter()
        .map(|l| &l[TAB_LABEL_PREFIX.len()..])
        .collect();
    let pruned = {
        let state = app.state::<AppState>();
        let mut audible = state.audible_tabs.lock().expect("audible_tabs poisoned");
        let before = audible.len();
        audible.retain(|id| live.contains(id.as_str()));
        audible.len() != before
    };
    if pruned {
        emit_snapshot(app);
    }

    #[cfg(target_os = "macos")]
    for label in labels {
        let Some(webview) = app.webviews().get(&label).cloned() else {
            continue;
        };
        let uuid = label[TAB_LABEL_PREFIX.len()..].to_string();
        let app = app.clone();
        let _ = webview.with_webview(move |w| {
            let playing = unsafe {
                use objc2::runtime::AnyObject;
                use objc2::{msg_send, sel};

                let wk = w.inner() as *mut AnyObject;
                if wk.is_null() {
                    return;
                }
                let responds: bool = msg_send![wk, respondsToSelector: sel!(_isPlayingAudio)];
                if !responds {
                    return;
                }
                let playing: bool = msg_send![wk, _isPlayingAudio];
                playing
            };

            let state = app.state::<AppState>();
            let changed = {
                let mut audible = state.audible_tabs.lock().expect("audible_tabs poisoned");
                if playing {
                    audible.insert(uuid.clone())
                } else {
                    audible.remove(&uuid)
                }
            };
            if changed {
                log::debug!("audio: tab {uuid} audible={playing}");
                emit_snapshot(&app);
            }
        });
    }

    #[cfg(windows)]
    for label in labels {
        let Some(webview) = app.webviews().get(&label).cloned() else {
            continue;
        };
        let uuid = label[TAB_LABEL_PREFIX.len()..].to_string();
        let app = app.clone();
        crate::win_webview2::with_core_webview2(&webview, move |core| {
            use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_8;
            use windows::core::{Interface, BOOL};

            let playing = core
                .cast::<ICoreWebView2_8>()
                .ok()
                .and_then(|c8| {
                    let mut b = BOOL(0);
                    // SAFETY: runs on the UI thread via `with_core_webview2`.
                    unsafe { c8.IsDocumentPlayingAudio(&mut b) }
                        .ok()
                        .map(|()| b.as_bool())
                })
                .unwrap_or(false);

            let state = app.state::<AppState>();
            let changed = {
                let mut audible = state.audible_tabs.lock().expect("audible_tabs poisoned");
                if playing {
                    audible.insert(uuid.clone())
                } else {
                    audible.remove(&uuid)
                }
            };

            if changed {
                log::debug!("audio: tab {uuid} audible={playing}");
                emit_snapshot(&app);
            }
        });
    }
}
