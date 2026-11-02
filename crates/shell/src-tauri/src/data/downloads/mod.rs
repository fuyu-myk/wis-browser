//! Download manager: the imperative shell around [`wis_core::library::download`].
//!
//! Routes wry download events from remote webviews (tabs, Peek, widget
//! panels) into tracked state. Engine-created popups (`window.open` allowed
//! via `on_new_window`) carry no delegates in wry, so downloads inside them
//! are not intercepted.
//!
//! Security model:
//! - By default the destination is always `<downloads dir>/<leaf>`, where the
//!   leaf has passed [`wis_core::sanitize_filename`] +
//!   [`wis_core::unique_filename`]. The server-suggested name can never
//!   influence the directory, hide the file (leading dot), or overwrite an
//!   existing one. With the ask-where setting on, the destination is instead
//!   whatever the user picks in the native save panel (an explicit user
//!   gesture; the server still only influences the prefilled, sanitised name).
//! - Completed files get the `com.apple.quarantine` xattr, so Gatekeeper
//!   treats them like any browser download (scans executables on first open).
//!
//! macOS API limits (WKDownload via wry): the finish event carries only
//! `url + success` (no path), so completion is matched to the oldest
//! in-flight download of that URL; there is no native progress or cancel
//! hook, so progress is the destination file's growing size, polled.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::webview::{DownloadEvent, WebviewBuilder};
use tauri::{AppHandle, Emitter, Manager, State, Wry};
use url::Url;
use wis_core::{DownloadId, DownloadStatus, UserSettings, UserSettingsPatch, WisError, WisResult};
use wis_storage::DownloadRecord;

use crate::state::AppState;

mod os;

/// Sync mirror of the download-related settings, readable from wry's
/// synchronous main-thread download callback (which cannot await the async
/// `BrowserState` lock without risking a deadlock against commands that
/// dispatch to the main thread while holding it). Seeded at startup, kept
/// fresh by `update_settings` and by panel picks.
#[derive(Debug, Clone)]
pub struct DownloadPrefs {
    pub ask_where: bool,
    pub last_dir: Option<PathBuf>,
}

impl DownloadPrefs {
    pub fn from_settings(settings: &UserSettings) -> Self {
        Self {
            ask_where: settings.download_ask_where,
            last_dir: settings.download_last_dir.as_ref().map(PathBuf::from),
        }
    }
}

/// Emitted (payload-less) whenever any download starts, progresses, or ends;
/// the chrome refetches via [`list_downloads`].
pub const DOWNLOADS_CHANGED_EVENT: &str = "downloads-changed";

/// How often an in-flight download's on-disk size is polled for progress.
const PROGRESS_POLL_MS: u64 = 500;

/// Terminal items kept in memory beyond this count are trimmed oldest-first
/// (persistent history is the storage layer's job, not this list's).
const MAX_TRACKED: usize = 100;

/// One tracked download, in-memory. Serialized to the chrome as camelCase.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadItem {
    pub id: DownloadId,
    pub url: String,
    pub filename: String,
    pub path: PathBuf,
    pub received_bytes: u64,
    /// Expected total from the response's Content-Length; `None` when the
    /// server sent no length (chunked), or for an engine download off macOS.
    /// Drives the determinate pie in the chrome.
    pub total_bytes: Option<u64>,
    pub status: DownloadStatus,
    /// Requested from an ephemeral (private) context: still saved to disk at
    /// the user's request, but never recorded in persistent history.
    pub private: bool,
    pub started_at_ms: u64,
    /// Fetched by Wis rather than written by the engine, so no engine event
    /// may finish it.
    #[serde(skip)]
    pub fetched: bool,
}

/// Wire download handling onto a remote webview builder. `private` marks
/// downloads from ephemeral contexts (private workspaces, Peek).
pub fn attach_download_handler(builder: WebviewBuilder<Wry>, private: bool) -> WebviewBuilder<Wry> {
    builder.on_download(move |webview, event| match event {
        DownloadEvent::Requested { url, destination } => {
            crate::pages::print_export::intercept(&url, destination)
                || on_requested(webview.app_handle(), &url, destination, private)
        }
        DownloadEvent::Finished { url, success, .. } => {
            if !crate::pages::print_export::finish(&webview, &url, success) {
                on_finished(webview.app_handle(), &url, success);
            }
            true
        }
        _ => true,
    })
}

/// Rows of persisted history surfaced alongside the session's downloads.
const HISTORY_LIMIT: u32 = 200;

/// Downloads newest first: this session's tracked items (all statuses),
/// then persisted completed history from earlier sessions (session items
/// that already wrote their history row are deduplicated by id).
#[tauri::command]
pub async fn list_downloads(state: State<'_, AppState>) -> WisResult<Vec<DownloadItem>> {
    let mut merged: Vec<DownloadItem> = {
        let items = state.downloads.lock().expect("downloads poisoned");
        items.iter().rev().cloned().collect()
    };
    let history =
        state
            .storage
            .list_downloads(HISTORY_LIMIT)
            .map_err(|err| WisError::PersistenceFailed {
                reason: err.to_string(),
            })?;
    let seen: std::collections::HashSet<DownloadId> = merged.iter().map(|item| item.id).collect();
    merged.extend(
        history
            .into_iter()
            .filter(|record| !seen.contains(&record.id))
            .map(DownloadItem::from_record),
    );

    Ok(merged)
}

/// Cancel an in-flight download: stop the engine's WKDownload (macOS, via
/// the vendored wry's fifth patch), mark the item cancelled, and best-effort
/// remove the partial file. The cancel affordance is macOS-only until the
/// other engines expose a cancel path.
#[tauri::command]
pub async fn cancel_download(
    app: AppHandle,
    state: State<'_, AppState>,
    id: DownloadId,
) -> WisResult<()> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, state, id);
        Err(WisError::InvalidInput {
            message: "download cancel is not available on this platform".to_string(),
        })
    }

    #[cfg(target_os = "macos")]
    {
        let path = {
            let mut items = state.downloads.lock().expect("downloads poisoned");
            let Some(item) = items
                .iter_mut()
                .find(|item| item.id == id && item.status == DownloadStatus::InProgress)
            else {
                return Ok(());
            };
            item.status = DownloadStatus::Cancelled;
            item.path.clone()
        };

        let path_str = path.display().to_string();
        let result = app.run_on_main_thread(move || {
            if !wry::cancel_download_to(&path_str) {
                log::warn!("cancel_download: no live engine download for {path_str}");
            }
            // POSIX unlink: removal completes once the engine closes its
            // handle. May already be gone if nothing was written yet.
            if let Err(err) = std::fs::remove_file(&path_str) {
                log::debug!("cancel_download: partial file remove: {err}");
            }
        });
        if let Err(err) = result {
            log::warn!("cancel_download: main-thread hop failed: {err}");
        }

        emit_changed(&app);
        Ok(())
    }
}

/// Remove a download from the list and persisted history (the file itself is
/// untouched). In-flight items are kept for cancel, not removal.
#[tauri::command]
pub async fn remove_download(
    app: AppHandle,
    state: State<'_, AppState>,
    id: DownloadId,
) -> WisResult<()> {
    {
        let mut items = state.downloads.lock().expect("downloads poisoned");
        items.retain(|item| item.id != id || !item.status.is_terminal());
    }
    state
        .storage
        .delete_download(id)
        .map_err(|err| WisError::PersistenceFailed {
            reason: err.to_string(),
        })?;
    emit_changed(&app);

    Ok(())
}

/// Remove several downloads at once (select-mode delete). Same semantics as
/// [`remove_download`], one event at the end.
#[tauri::command]
pub async fn remove_downloads(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<DownloadId>,
) -> WisResult<()> {
    {
        let mut items = state.downloads.lock().expect("downloads poisoned");
        items.retain(|item| !ids.contains(&item.id) || !item.status.is_terminal());
    }
    for id in ids {
        state
            .storage
            .delete_download(id)
            .map_err(|err| WisError::PersistenceFailed {
                reason: err.to_string(),
            })?;
    }
    emit_changed(&app);

    Ok(())
}

/// Open a completed download with its default app. Returns `false` when the
/// file is no longer at its recorded path (moved or deleted).
#[tauri::command]
pub async fn open_download(state: State<'_, AppState>, id: DownloadId) -> WisResult<bool> {
    let Some(path) = completed_download_path(&state, id)? else {
        return Ok(false);
    };
    let Some(valid) = validate_on_disk(&path)? else {
        return Ok(false);
    };
    spawn_open(os::open_path_command(&valid))?;

    Ok(true)
}

/// Reveal a completed download in the file manager. Same missing-file
/// semantics as [`open_download`].
#[tauri::command]
pub async fn reveal_download(state: State<'_, AppState>, id: DownloadId) -> WisResult<bool> {
    let Some(path) = completed_download_path(&state, id)? else {
        return Ok(false);
    };
    let Some(valid) = validate_on_disk(&path)? else {
        return Ok(false);
    };
    spawn_open(os::reveal_path_command(&valid))?;

    Ok(true)
}

/// The recorded path of a *completed* download, from the session list or
/// persisted history. `None` for unknown ids; opening a non-completed item is
/// an error (an in-flight file is partial; failed/cancelled left no file).
fn completed_download_path(
    state: &State<'_, AppState>,
    id: DownloadId,
) -> WisResult<Option<PathBuf>> {
    let in_memory = {
        let items = state.downloads.lock().expect("downloads poisoned");
        items
            .iter()
            .find(|item| item.id == id)
            .map(|item| (item.status, item.path.clone()))
    };
    if let Some((status, path)) = in_memory {
        if status != DownloadStatus::Completed {
            return Err(WisError::InvalidInput {
                message: format!("download is {status:?}, not completed"),
            });
        }
        return Ok(Some(path));
    }
    let record = state
        .storage
        .get_download(id)
        .map_err(|err| WisError::PersistenceFailed {
            reason: err.to_string(),
        })?;

    Ok(record.map(|r| PathBuf::from(r.path)))
}

/// Boundary check before shelling out: the file must still exist as a
/// regular file directly inside the directory it was downloaded to (the
/// recorded path's parent). A recorded path is our own data, but symlink
/// swaps and DB tampering are cheap to rule out; custom ask-where locations
/// are covered because the check anchors on the recorded parent rather than
/// the default downloads directory. `None` means "not there anymore" (the
/// moved-or-deleted case); structural violations are errors.
fn validate_on_disk(path: &Path) -> WisResult<Option<PathBuf>> {
    let Some(parent) = path.parent() else {
        return Err(WisError::InvalidInput {
            message: "download path has no parent directory".to_string(),
        });
    };
    let Ok(parent_canonical) = parent.canonicalize() else {
        return Ok(None);
    };
    let Ok(canonical) = path.canonicalize() else {
        return Ok(None);
    };
    if !canonical.is_file() {
        return Ok(None);
    }
    if canonical.parent() != Some(parent_canonical.as_path()) {
        return Err(WisError::InvalidInput {
            message: "download path resolves outside its recorded directory".to_string(),
        });
    }

    Ok(Some(canonical))
}

fn spawn_open(mut cmd: std::process::Command) -> WisResult<()> {
    cmd.spawn()
        .map(|_| ())
        .map_err(|err| WisError::InvalidInput {
            message: format!("failed to open download: {err}"),
        })
}

/// Decide the destination for a requested download and register it.
/// Returning `false` denies the download.
fn on_requested(app: &AppHandle, url: &Url, destination: &mut PathBuf, private: bool) -> bool {
    let dir = match downloads_dir(app) {
        Ok(dir) => dir,
        Err(err) => {
            log::warn!("download denied, no downloads dir: {err}");
            return false;
        }
    };

    // wry prefills `destination` with its own <downloads>/<suggested> guess;
    // only the leaf is taken, as an untrusted suggestion.
    let suggested = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let sanitized = wis_core::sanitize_filename(suggested);

    let state = app.state::<AppState>();
    let prefs = state
        .download_prefs
        .lock()
        .expect("download_prefs poisoned")
        .clone();

    let save_as = crate::pages::save_url::take_mark(url).is_some();
    let path = if prefs.ask_where || save_as {
        match pick_destination(app, &state, &prefs, &sanitized, &dir) {
            Some(path) => path,
            None => return false,
        }
    } else {
        let items = state.downloads.lock().expect("downloads poisoned");
        let filename = wis_core::unique_filename(&sanitized, |candidate| {
            let target = dir.join(candidate);
            target.exists()
                || items
                    .iter()
                    .any(|item| !item.status.is_terminal() && item.path == target)
        });
        let path = dir.join(&filename);
        // Defense in depth: the sanitizer guarantees a bare leaf, but never
        // hand the engine a path outside the downloads directory.
        if filename.contains(['/', '\\']) || path.parent() != Some(dir.as_path()) {
            log::error!("download denied, destination escaped {}", dir.display());
            return false;
        }

        path
    };

    // Read within the started callback's call stack, where the patched wry
    // stashed the response's expected size (-1 = unknown).
    #[cfg(target_os = "macos")]
    let total_bytes = {
        let expected = wry::last_download_expected_bytes();
        (expected > 0).then_some(expected as u64)
    };

    #[cfg(not(target_os = "macos"))]
    let total_bytes = None;

    *destination = path.clone();
    let id = track(app, url, path, total_bytes, private, false);
    spawn_progress_poller(app.clone(), id);
    true
}

/// List a download writing to `path` as in progress.
pub(crate) fn track(
    app: &AppHandle,
    url: &Url,
    path: PathBuf,
    total_bytes: Option<u64>,
    private: bool,
    fetched: bool,
) -> DownloadId {
    let item = DownloadItem {
        id: DownloadId::new(),
        url: url.to_string(),
        filename: path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| wis_core::FALLBACK_FILENAME.to_string()),
        path,
        received_bytes: 0,
        total_bytes,
        status: DownloadStatus::InProgress,
        private,
        started_at_ms: now_ms(),
        fetched,
    };
    let id = item.id;
    log::info!("download started: {url} -> {}", item.path.display());
    {
        let state = app.state::<AppState>();
        let mut items = state.downloads.lock().expect("downloads poisoned");
        items.push(item);
        trim_terminal(&mut items);
    }

    emit_changed(app);
    id
}

/// Record how much of an in-flight download has arrived.
#[cfg(windows)]
pub(crate) fn progress(app: &AppHandle, id: DownloadId, received: u64) {
    {
        let state = app.state::<AppState>();
        let mut items = state.downloads.lock().expect("downloads poisoned");
        let Some(item) = items
            .iter_mut()
            .find(|item| item.id == id && item.status == DownloadStatus::InProgress)
        else {
            return;
        };
        item.received_bytes = received;
    }

    emit_changed(app);
}

/// Ask where a Save as download goes, starting in the remembered directory.
/// `None` when the user cancels. Blocks on the panel, so never call it on
/// the async runtime.
#[cfg(windows)]
pub(crate) fn ask_save_as(app: &AppHandle, suggested: &str) -> Option<PathBuf> {
    let dir = downloads_dir(app)
        .map_err(|err| log::warn!("save as denied, no downloads dir: {err}"))
        .ok()?;
    let state = app.state::<AppState>();
    let prefs = state
        .download_prefs
        .lock()
        .expect("download_prefs poisoned")
        .clone();

    pick_destination(
        app,
        &state,
        &prefs,
        &wis_core::sanitize_filename(suggested),
        &dir,
    )
}

/// Ask-where flow: run the native save panel (starting in the remembered
/// directory) and remember the picked directory for next time. `None` means
/// the user cancelled (or the panel is unavailable): deny the download.
fn pick_destination(
    app: &AppHandle,
    state: &State<'_, AppState>,
    prefs: &DownloadPrefs,
    suggested: &str,
    default_dir: &Path,
) -> Option<PathBuf> {
    let start_dir = prefs
        .last_dir
        .as_deref()
        .filter(|dir| dir.is_dir())
        .unwrap_or(default_dir);
    let Some(chosen) = os::ask_destination(suggested, start_dir) else {
        log::info!("download cancelled in the save panel");
        return None;
    };
    // The panel already asked "replace?" for an existing file, but WKDownload
    // refuses an existing destination, so honour the user's explicit choice
    // by clearing the way first. Server names never reach this branch.
    if chosen.exists() {
        if let Err(err) = std::fs::remove_file(&chosen) {
            log::warn!(
                "download denied, cannot replace {}: {err}",
                chosen.display()
            );
            return None;
        }
    }
    let parent = chosen.parent()?.to_path_buf();
    {
        let mut prefs = state
            .download_prefs
            .lock()
            .expect("download_prefs poisoned");
        prefs.last_dir = Some(parent.clone());
    }
    persist_last_dir(app, parent);

    Some(chosen)
}

/// Persist the remembered download directory into `UserSettings` off the
/// main thread, so it survives a restart.
fn persist_last_dir(app: &AppHandle, dir: PathBuf) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let patch = UserSettingsPatch {
            download_last_dir: Some(dir.display().to_string()),
            ..Default::default()
        };
        let result =
            crate::commands::mutate_unit(&app, &state, |s| Ok(s.with_settings(patch))).await;

        if let Err(err) = result {
            log::warn!("remembering download directory failed: {err}");
        }
    });
}

impl DownloadItem {
    /// A persisted history row, viewed as a (completed) list item.
    fn from_record(record: DownloadRecord) -> Self {
        Self {
            id: record.id,
            url: record.url,
            filename: record.filename,
            path: PathBuf::from(record.path),
            received_bytes: record.bytes.max(0) as u64,
            total_bytes: None,
            status: DownloadStatus::Completed,
            private: false,
            started_at_ms: record.completed_at.timestamp_millis().max(0) as u64,
            fetched: false,
        }
    }
}

/// Mark the matching in-flight download finished.
fn on_finished(app: &AppHandle, url: &Url, success: bool) {
    // macOS reports no path on finish, so match the oldest in-flight engine
    // download of this URL.
    let found = finish_where(
        app,
        |item| {
            !item.fetched && item.status == DownloadStatus::InProgress && item.url == url.as_str()
        },
        success,
    );
    if !found {
        log::warn!("finish event for untracked download: {url}");
        crate::pages::save_url::finished_untracked(app, url, success);
    }
}

/// End an in-flight download.
#[cfg(windows)]
pub(crate) fn finish(app: &AppHandle, id: DownloadId, success: bool) {
    finish_where(app, |item| item.id == id, success);
}

/// End the first download `matches` picks, quarantining and recording it
/// when it succeeded. `false` when none matches.
fn finish_where(app: &AppHandle, matches: impl Fn(&DownloadItem) -> bool, success: bool) -> bool {
    let state = app.state::<AppState>();
    let completed = {
        let mut items = state.downloads.lock().expect("downloads poisoned");
        let Some(item) = items.iter_mut().find(|item| matches(item)) else {
            return false;
        };

        match item.status.finished(success) {
            Ok(next) => item.status = next,
            Err(err) => {
                log::warn!("download {}: {err}", item.url);
                return true;
            }
        }

        if let Ok(meta) = std::fs::metadata(&item.path) {
            item.received_bytes = meta.len();
        }
        log::info!(
            "download {}: {}",
            if success { "completed" } else { "failed" },
            item.path.display()
        );

        success.then(|| item.clone())
    };

    if let Some(item) = completed {
        if let Err(err) = os::set_quarantine(&item.path) {
            log::warn!("quarantine xattr on {} failed: {err}", item.path.display());
        }
        if !item.private {
            persist_completed(app, item);
        }
    }

    emit_changed(app);
    true
}

/// Write a completed download's history row off the main thread. Private
/// downloads never reach here.
fn persist_completed(app: &AppHandle, item: DownloadItem) {
    let record = DownloadRecord {
        id: item.id,
        url: item.url,
        filename: item.filename,
        path: item.path.display().to_string(),
        bytes: item.received_bytes.min(i64::MAX as u64) as i64,
        completed_at: chrono::Utc::now(),
    };
    let storage = app.state::<AppState>().storage.clone();

    tauri::async_runtime::spawn(async move {
        if let Err(err) = storage.record_download(&record) {
            log::warn!("download history write failed for {}: {err}", record.url);
        }
    });
}

/// The user's downloads directory, created if missing. Downloads are denied
/// when it cannot be resolved, never redirected elsewhere.
fn downloads_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .download_dir()
        .map_err(|err| format!("resolve: {err}"))?;
    std::fs::create_dir_all(&dir).map_err(|err| format!("create {}: {err}", dir.display()))?;

    Ok(dir)
}

/// Poll the destination file's size while the download is in flight, so the
/// chrome can show progress (WKDownload exposes no progress callback through
/// wry). Stops as soon as the item is terminal or gone.
fn spawn_progress_poller(app: AppHandle, id: DownloadId) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(PROGRESS_POLL_MS)).await;
            let state = app.state::<AppState>();
            let grown = {
                let mut items = state.downloads.lock().expect("downloads poisoned");
                match items.iter_mut().find(|item| item.id == id) {
                    Some(item) if item.status == DownloadStatus::InProgress => {
                        let size = std::fs::metadata(&item.path).map(|m| m.len()).unwrap_or(0);
                        let grown = size != item.received_bytes;
                        item.received_bytes = size;
                        Some(grown)
                    }
                    _ => None,
                }
            };

            match grown {
                Some(true) => emit_changed(&app),
                Some(false) => {}
                None => break,
            }
        }
    });
}

/// Drop the oldest terminal items once the list exceeds [`MAX_TRACKED`].
/// In-flight items are never trimmed.
fn trim_terminal(items: &mut Vec<DownloadItem>) {
    while items.len() > MAX_TRACKED {
        let Some(pos) = items.iter().position(|item| item.status.is_terminal()) else {
            return;
        };

        items.remove(pos);
    }
}

fn emit_changed(app: &AppHandle) {
    if let Err(err) = app.emit(DOWNLOADS_CHANGED_EVENT, ()) {
        log::warn!("emit {DOWNLOADS_CHANGED_EVENT} failed: {err}");
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
