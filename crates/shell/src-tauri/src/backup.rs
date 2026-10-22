//! Encrypted profile backup: export selected categories to a passphrase-
//! encrypted `.wisbackup` file, and import one back. The payload shape,
//! boundary validation, and re-mint discipline live in [`wis_core::backup`];
//! this module adds the passphrase KDF + AEAD envelope (wis-secrets) and the
//! native file panels. Import returns the validated, re-minted payload and the
//! frontend applies it through the existing settings commands, so a merge
//! reuses their validation and side effects (menu rebuild, theme apply, ...).

use std::collections::HashMap;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use wis_core::backup::{
    merge_profile, BackupPayload, BookmarkBackup, CanvasBackup, HistoryBackup, ProfileBackup,
    WorkspaceBookmarks, WorkspaceCanvases, WorkspaceHistory, BACKUP_VERSION,
};
use wis_core::canvas::{canvas_name, CanvasRecord};
use wis_core::{Tab, UserSettings, WisError, WisResult, Workspace, WorkspaceId};

use crate::commands::mutate_immediate;
use crate::panels::{run_open_panel, run_save_panel};
use crate::state::AppState;

/// Cap on a backup file, to reject malformed or hostile input before
/// allocating. Generous enough for settings, themes, and profile data.
const MAX_BACKUP_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// Per-workspace history is capped on export to bound the bundle size.
const HISTORY_BACKUP_LIMIT: u32 = 10_000;

/// Which categories an export includes. Defaults to nothing; the frontend sets
/// the toggles the user turned on.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct BackupOptions {
    #[serde(default)]
    pub settings: bool,
    /// Workspaces + their tabs + bookmarks.
    #[serde(default)]
    pub profile: bool,
    /// Browsing history (only meaningful alongside `profile`).
    #[serde(default)]
    pub history: bool,
    /// Saved passwords (Touch ID required to decrypt them for the bundle).
    #[serde(default)]
    pub vault: bool,
    /// Canvases (only meaningful alongside `profile`).
    #[serde(default)]
    pub canvases: bool,
}

/// What an import applied, for the frontend to surface + finish (it applies the
/// returned settings through the existing settings commands).
#[derive(Debug, Serialize)]
pub struct ImportSummary {
    pub settings: Option<UserSettings>,
    pub workspaces_added: usize,
    pub bookmarks_added: usize,
    pub history_added: usize,
    pub canvases_added: usize,
    pub passwords_added: usize,
}

/// On-disk envelope: a versioned, passphrase-encrypted [`BackupPayload`]. The
/// salt and nonce are public (standard AEAD practice); only `ciphertext` is
/// secret. Bytes are base64 so the file stays compact, valid JSON.
#[derive(Debug, Serialize, Deserialize)]
struct BackupFile {
    version: u8,
    salt: String,
    nonce: String,
    ciphertext: String,
}

/// Export the selected categories to a passphrase-encrypted `.wisbackup` file.
/// Resolves `false` on cancel.
#[tauri::command]
pub async fn export_backup(
    app: AppHandle,
    state: State<'_, AppState>,
    passphrase: String,
    options: BackupOptions,
) -> WisResult<bool> {
    if passphrase.trim().is_empty() {
        return Err(WisError::InvalidInput {
            message: "choose a passphrase to encrypt the backup".to_string(),
        });
    }

    let payload = BackupPayload {
        settings: if options.settings {
            Some(state.browser.lock().await.settings.clone())
        } else {
            None
        },
        profile: if options.profile {
            Some(build_profile(&state, options.history, options.canvases).await?)
        } else {
            None
        },
        credentials: if options.vault {
            Some(
                crate::vault::export_credentials(
                    &state,
                    "Include your saved passwords in this backup",
                )
                .await?,
            )
        } else {
            None
        },
    };
    if payload.is_empty() {
        return Err(WisError::InvalidInput {
            message: "select at least one category to back up".to_string(),
        });
    }

    let plaintext = serde_json::to_vec(&payload).map_err(|err| WisError::InvalidInput {
        message: format!("serialize backup: {err}"),
    })?;

    let salt = wis_secrets::random_salt();
    let key = wis_secrets::derive_passphrase_key(&passphrase, &salt).map_err(|_| {
        WisError::InvalidInput {
            message: "key derivation failed".to_string(),
        }
    })?;
    let blob = wis_secrets::encrypt(&key, &plaintext).map_err(|_| WisError::InvalidInput {
        message: "encryption failed".to_string(),
    })?;

    let file = BackupFile {
        version: BACKUP_VERSION,
        salt: BASE64.encode(salt),
        nonce: BASE64.encode(&blob.nonce),
        ciphertext: BASE64.encode(&blob.ciphertext),
    };
    let json = serde_json::to_string_pretty(&file).map_err(|err| WisError::InvalidInput {
        message: format!("serialize backup file: {err}"),
    })?;

    let Some(path) = run_save_panel(&app, &suggested_name())? else {
        return Ok(false);
    };
    std::fs::write(&path, json).map_err(|err| WisError::InvalidInput {
        message: format!("write {}: {err}", path.display()),
    })?;

    Ok(true)
}

/// Open a `.wisbackup` file, decrypt it with `passphrase`, apply the profile
/// graph (workspaces/tabs into state, bookmarks/history into storage) by
/// **merging** (re-minted ids, same-named workspaces skipped), and return a
/// summary. This includes the settings for the frontend to apply through the
/// settings commands. `None` on cancel. A wrong passphrase or a tampered file
/// surfaces as a decrypt error (AEAD).
#[tauri::command]
pub async fn import_backup(
    app: AppHandle,
    state: State<'_, AppState>,
    passphrase: String,
) -> WisResult<Option<ImportSummary>> {
    let Some(path) = run_open_panel(&app, &["wisbackup", "json"])? else {
        return Ok(None);
    };

    let size = std::fs::metadata(&path)
        .map(|m| m.len())
        .unwrap_or(u64::MAX);
    if size > MAX_BACKUP_FILE_BYTES {
        return Err(WisError::InvalidInput {
            message: "backup file is too large".to_string(),
        });
    }

    let json = std::fs::read_to_string(&path).map_err(|err| WisError::InvalidInput {
        message: format!("read {}: {err}", path.display()),
    })?;
    let file: BackupFile = serde_json::from_str(&json).map_err(|err| WisError::InvalidInput {
        message: format!("not a valid .wisbackup file: {err}"),
    })?;
    if file.version > BACKUP_VERSION {
        return Err(WisError::InvalidInput {
            message: "backup was made by a newer version of Wis".to_string(),
        });
    }

    let salt = BASE64.decode(&file.salt).map_err(|_| malformed())?;
    let nonce = BASE64.decode(&file.nonce).map_err(|_| malformed())?;
    let ciphertext = BASE64.decode(&file.ciphertext).map_err(|_| malformed())?;

    let key = wis_secrets::derive_passphrase_key(&passphrase, &salt).map_err(|_| {
        WisError::InvalidInput {
            message: "key derivation failed".to_string(),
        }
    })?;
    let plaintext = wis_secrets::decrypt(&key, &wis_secrets::EncryptedBlob { ciphertext, nonce })
        .map_err(|_| WisError::InvalidInput {
        message: "wrong passphrase, or the backup is corrupted".to_string(),
    })?;

    let payload: BackupPayload =
        serde_json::from_slice(&plaintext).map_err(|err| WisError::InvalidInput {
            message: format!("backup contents are unreadable: {err}"),
        })?;
    payload.validate()?;
    let payload = payload.reminted();

    let mut summary = ImportSummary {
        settings: payload.settings.clone(),
        workspaces_added: 0,
        bookmarks_added: 0,
        history_added: 0,
        canvases_added: 0,
        passwords_added: 0,
    };

    if let Some(creds) = payload.credentials {
        summary.passwords_added =
            crate::vault::import_credentials(&state, creds, "Restore your saved passwords").await?;
    }

    let imported = payload
        .profile
        .as_ref()
        .map(|profile| profile.workspaces.clone())
        .unwrap_or_default();
    if let Some(profile) = payload.profile {
        let (bm, hist, canvases) = apply_profile(&app, &state, profile, &mut summary).await?;
        summary.bookmarks_added = bm;
        summary.history_added = hist;
        summary.canvases_added = canvases;
    }
    if let Some(settings) = summary.settings.as_mut() {
        let live = state.browser.lock().await;
        settings.link_routes = wis_core::backup::link_routes_onto(
            std::mem::take(&mut settings.link_routes),
            &imported,
            &live.workspaces,
        );
    }

    Ok(Some(summary))
}

/// Merge a re-minted profile: splice its non-clashing workspaces + tabs into the
/// live state, then write those workspaces' bookmarks + history to storage. Sets
/// `summary.workspaces_added`; returns the bookmark + history counts.
async fn apply_profile(
    app: &AppHandle,
    state: &State<'_, AppState>,
    profile: ProfileBackup,
    summary: &mut ImportSummary,
) -> WisResult<(usize, usize, usize)> {
    let graph = profile.clone();
    let added: Vec<WorkspaceId> = mutate_immediate(app, state, move |s| {
        let (next, added) = merge_profile(s.clone(), &graph);
        Ok((next, added))
    })
    .await?;
    summary.workspaces_added = added.len();
    if added.is_empty() {
        return Ok((0, 0, 0));
    }

    let added: std::collections::HashSet<WorkspaceId> = added.into_iter().collect();
    let bookmarks: Vec<WorkspaceBookmarks> = profile
        .bookmarks
        .into_iter()
        .filter(|wb| added.contains(&wb.workspace))
        .collect();
    let history: Vec<WorkspaceHistory> = profile
        .history
        .into_iter()
        .filter(|wh| added.contains(&wh.workspace))
        .collect();
    let canvases: Vec<WorkspaceCanvases> = profile
        .canvases
        .into_iter()
        .filter(|wc| added.contains(&wc.workspace))
        .collect();

    let storage = state.storage.clone();
    run_blocking(move || {
        let mut bm_count = 0usize;
        for wb in &bookmarks {
            // Re-create folders in order, mapping the export's folder indexes to
            // the new storage rowids.
            let mut folder_ids: Vec<i64> = Vec::with_capacity(wb.folders.len());
            for name in &wb.folders {
                folder_ids.push(storage.create_bookmark_folder(wb.workspace, name)?.id);
            }
            for bm in &wb.bookmarks {
                let folder = bm.folder.and_then(|i| folder_ids.get(i).copied());
                storage.add_bookmark(wb.workspace, folder, &bm.url, &bm.title, None, bm.pinned)?;
                bm_count += 1;
            }
        }
        let mut hist_count = 0usize;
        for wh in &history {
            for e in &wh.entries {
                storage.record_visit(wh.workspace, &e.url, &e.title)?;
                hist_count += 1;
            }
        }
        let mut canvas_count = 0usize;
        for set in &canvases {
            for canvas in &set.canvases {
                storage.insert_canvas(&CanvasRecord {
                    id: canvas.id,
                    workspace_id: set.workspace,
                    name: canvas_name(&canvas.name).map_err(|err| {
                        wis_storage::StorageError::DataCorruption(err.to_string())
                    })?,
                    doc: canvas.doc.clone(),
                    created_at: canvas.created_at,
                    updated_at: canvas.updated_at,
                })?;
                canvas_count += 1;
            }
        }
        Ok((bm_count, hist_count, canvas_count))
    })
    .await
}

/// Snapshot the workspace graph for export: non-ephemeral workspaces and their
/// tabs from the live state, plus each workspace's bookmarks (and history, when
/// `include_history`) from storage.
async fn build_profile(
    state: &State<'_, AppState>,
    include_history: bool,
    include_canvases: bool,
) -> WisResult<ProfileBackup> {
    let (workspaces, tabs, ws_ids): (Vec<Workspace>, Vec<Tab>, Vec<WorkspaceId>) = {
        let browser = state.browser.lock().await;
        let workspaces: Vec<Workspace> = browser
            .workspaces
            .iter()
            .filter(|w| !w.ephemeral)
            .cloned()
            .collect();
        let ws_ids: Vec<WorkspaceId> = workspaces.iter().map(|w| w.id).collect();
        let tabs: Vec<Tab> = browser
            .tabs
            .iter()
            .filter(|t| ws_ids.contains(&t.workspace_id))
            .cloned()
            .collect();

        (workspaces, tabs, ws_ids)
    };

    let ws_ids_for_canvases = ws_ids.clone();
    let storage = state.storage.clone();
    let (bookmarks, history) = run_blocking(move || {
        let mut bookmarks = Vec::new();
        let mut history = Vec::new();
        for ws_id in ws_ids {
            let folders = storage.list_bookmark_folders(ws_id)?;
            let marks = storage.list_bookmarks(ws_id)?;
            if !folders.is_empty() || !marks.is_empty() {
                let index: HashMap<i64, usize> =
                    folders.iter().enumerate().map(|(i, f)| (f.id, i)).collect();
                bookmarks.push(WorkspaceBookmarks {
                    workspace: ws_id,
                    folders: folders.iter().map(|f| f.name.clone()).collect(),
                    bookmarks: marks
                        .iter()
                        .map(|b| BookmarkBackup {
                            folder: b.folder_id.and_then(|fid| index.get(&fid).copied()),
                            url: b.url.clone(),
                            title: b.title.clone(),
                            pinned: b.pinned,
                        })
                        .collect(),
                });
            }

            if include_history {
                let query = wis_storage::HistoryQuery {
                    search: String::new(),
                    workspace_id: Some(ws_id),
                    limit: HISTORY_BACKUP_LIMIT,
                    offset: 0,
                };
                let entries = storage.query_history(&query)?;
                if !entries.is_empty() {
                    history.push(WorkspaceHistory {
                        workspace: ws_id,
                        entries: entries
                            .iter()
                            .map(|e| HistoryBackup {
                                url: e.url.clone(),
                                title: e.title.clone(),
                            })
                            .collect(),
                    });
                }
            }
        }

        Ok((bookmarks, history))
    })
    .await?;

    let canvases = if include_canvases {
        collect_canvases(state, ws_ids_for_canvases).await?
    } else {
        Vec::new()
    };

    Ok(ProfileBackup {
        workspaces,
        tabs,
        bookmarks,
        history,
        canvases,
    })
}

/// Each workspace's canvases, with their newest documents written first.
async fn collect_canvases(
    state: &State<'_, AppState>,
    ws_ids: Vec<WorkspaceId>,
) -> WisResult<Vec<WorkspaceCanvases>> {
    // Pending documents first, so the backup holds the latest of each.
    let flushing = state.storage.clone();
    tokio::task::spawn_blocking(move || crate::canvas::flush_now(flushing.as_ref()))
        .await
        .map_err(|err| WisError::PersistenceFailed {
            reason: format!("writing canvases panicked: {err}"),
        })??;

    let storage = state.storage.clone();
    run_blocking(move || {
        let mut sets = Vec::new();
        for ws_id in ws_ids {
            let mut canvases = Vec::new();
            for summary in storage.list_canvases(ws_id)? {
                if let Some(canvas) = storage.load_canvas(ws_id, summary.id)? {
                    canvases.push(CanvasBackup {
                        id: canvas.id,
                        name: canvas.name,
                        doc: canvas.doc,
                        created_at: canvas.created_at,
                        updated_at: canvas.updated_at,
                    });
                }
            }
            if !canvases.is_empty() {
                sets.push(WorkspaceCanvases {
                    workspace: ws_id,
                    canvases,
                });
            }
        }

        Ok(sets)
    })
    .await
}

/// Run a synchronous storage operation off the async runtime.
async fn run_blocking<T, F>(op: F) -> WisResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> wis_storage::StorageResult<T> + Send + 'static,
{
    tokio::task::spawn_blocking(op)
        .await
        .map_err(|err| WisError::PersistenceFailed {
            reason: format!("backup task panicked: {err}"),
        })?
        .map_err(|err| WisError::PersistenceFailed {
            reason: err.to_string(),
        })
}

fn malformed() -> WisError {
    WisError::InvalidInput {
        message: "the backup file is malformed".to_string(),
    }
}

/// Suggested file name, e.g. `Wis Backup 2026-06-24.wisbackup`.
fn suggested_name() -> String {
    let stamp = chrono::Local::now().format("%Y-%m-%d");
    format!(
        "{}.wisbackup",
        wis_core::sanitize_filename(&format!("Wis Backup {stamp}"))
    )
}
