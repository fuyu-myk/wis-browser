//! Download manager: the imperative shell around [`wis_core::download`].
//!
//! Routes wry download events from remote webviews (tabs, Peek, widget
//! panels) into tracked state. Engine-created popups (`window.open` allowed
//! via `on_new_window`) carry no delegates in wry, so downloads inside them
//! are not intercepted.
//!
//! Security model:
//! - The destination is always `<downloads dir>/<leaf>`, where the leaf has
//!   passed [`wis_core::sanitize_filename`] + [`wis_core::unique_filename`]. The
//!   server-suggested name can never influence the directory, hide the file
//!   (leading dot), or overwrite an existing one.
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
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use url::Url;
use wis_core::{DownloadId, DownloadStatus};

use crate::state::AppState;

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
    pub status: DownloadStatus,
    /// Requested from an ephemeral (private) context: still saved to disk at
    /// the user's request, but never recorded in persistent history.
    pub private: bool,
    pub started_at_ms: u64,
}

/// Wire download handling onto a remote webview builder. `private` marks
/// downloads from ephemeral contexts (private workspaces, Peek).
pub fn attach_download_handler<R: Runtime>(
    builder: WebviewBuilder<R>,
    private: bool,
) -> WebviewBuilder<R> {
    builder.on_download(move |webview, event| match event {
        DownloadEvent::Requested { url, destination } => {
            on_requested(webview.app_handle(), &url, destination, private)
        }
        DownloadEvent::Finished { url, success, .. } => {
            on_finished(webview.app_handle(), &url, success);
            true
        }
        _ => true,
    })
}

/// Snapshot of tracked downloads, newest first.
#[tauri::command]
pub fn list_downloads(state: State<'_, AppState>) -> Vec<DownloadItem> {
    let items = state.downloads.lock().expect("downloads poisoned");
    items.iter().rev().cloned().collect()
}

/// Decide the destination for a requested download and register it.
/// Returning `false` denies the download.
fn on_requested<R: Runtime>(
    app: &AppHandle<R>,
    url: &Url,
    destination: &mut PathBuf,
    private: bool,
) -> bool {
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
    let id = {
        let mut items = state.downloads.lock().expect("downloads poisoned");
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

        let item = DownloadItem {
            id: DownloadId::new(),
            url: url.to_string(),
            filename,
            path: path.clone(),
            received_bytes: 0,
            status: DownloadStatus::InProgress,
            private,
            started_at_ms: now_ms(),
        };
        let id = item.id;
        log::info!("download started: {url} -> {}", path.display());

        items.push(item);
        trim_terminal(&mut items);
        *destination = path;

        id
    };

    emit_changed(app);
    spawn_progress_poller(app.clone(), id);
    true
}

/// Mark the matching in-flight download finished and quarantine the file.
fn on_finished<R: Runtime>(app: &AppHandle<R>, url: &Url, success: bool) {
    let state = app.state::<AppState>();
    let completed_path = {
        let mut items = state.downloads.lock().expect("downloads poisoned");
        // macOS reports no path on finish, so match the oldest in-flight
        // download of this URL.
        let Some(item) = items
            .iter_mut()
            .find(|item| item.status == DownloadStatus::InProgress && item.url == url.as_str())
        else {
            log::warn!("finish event for untracked download: {url}");
            return;
        };

        match item.status.finished(success) {
            Ok(next) => item.status = next,
            Err(err) => {
                log::warn!("download {url}: {err}");
                return;
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

        success.then(|| item.path.clone())
    };

    if let Some(path) = completed_path {
        if let Err(err) = set_quarantine(&path) {
            log::warn!("quarantine xattr on {} failed: {err}", path.display());
        }
    }

    emit_changed(app);
}

/// The user's downloads directory, created if missing. Downloads are denied
/// when it cannot be resolved, never redirected elsewhere.
fn downloads_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
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
fn spawn_progress_poller<R: Runtime>(app: AppHandle<R>, id: DownloadId) {
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

fn emit_changed<R: Runtime>(app: &AppHandle<R>) {
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

/// Tag a completed download with `com.apple.quarantine` so Gatekeeper treats
/// it like any browser download. Field order: flags;hex-timestamp;agent;.
#[cfg(target_os = "macos")]
fn set_quarantine(path: &Path) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let value = format!("0081;{secs:08x};Wis;");
    let cpath = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::other("path contains NUL"))?;
    let cname = CString::new("com.apple.quarantine").expect("static name has no NUL");
    let rc = unsafe {
        libc::setxattr(
            cpath.as_ptr(),
            cname.as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
            0,
        )
    };

    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }

    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn set_quarantine(_path: &Path) -> std::io::Result<()> {
    Ok(())
}
