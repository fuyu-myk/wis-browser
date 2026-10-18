//! Choosing, staging, installing and removing extensions. A chosen folder or
//! `.zip` is copied into a staging folder and reviewed; installing moves it
//! into the extensions folder under a fresh id.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::oneshot;
use wis_core::extensions::{self, InstalledExtension, ReviewInput};
use wis_core::{WisError, WisResult};

use super::{host, stage, StagedExtension};
use crate::state::AppState;

const EXTENSIONS_DIR: &str = "extensions";
const STAGING_DIR: &str = "staging";
const READ_WAIT: Duration = Duration::from_secs(30);
const CHOOSE_WAIT: Duration = Duration::from_secs(30 * 60);

struct Staged {
    dir: PathBuf,
    root: PathBuf,
    name: String,
    version: String,
}

static STAGED: Mutex<Option<HashMap<String, Staged>>> = Mutex::new(None);

/// Whether this run has cleared what an earlier one left staged.
static SWEPT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn refused(message: impl Into<String>) -> WisError {
    WisError::InvalidInput {
        message: message.into(),
    }
}

fn staged<R>(f: impl FnOnce(&mut HashMap<String, Staged>) -> R) -> R {
    let mut guard = STAGED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(guard.get_or_insert_with(HashMap::new))
}

/// The extensions folder, without touching the disk.
pub(super) fn extensions_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|dir| dir.join(EXTENSIONS_DIR))
}

pub(super) fn extensions_dir(app: &AppHandle) -> WisResult<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|err| refused(format!("app data dir: {err}")))?
        .join(EXTENSIONS_DIR);
    std::fs::create_dir_all(&dir)
        .map_err(|err| refused(format!("create {}: {err}", dir.display())))?;
    Ok(dir)
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> WisResult<T> + Send + 'static,
) -> WisResult<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|err| refused(format!("extension task failed: {err}")))?
}

async fn on_main<T: Send + 'static>(
    app: &AppHandle,
    wait: Duration,
    work: impl FnOnce(oneshot::Sender<T>) + Send + 'static,
) -> WisResult<T> {
    let (tx, rx) = oneshot::channel();
    app.run_on_main_thread(move || work(tx))
        .map_err(|err| refused(format!("run_on_main_thread: {err}")))?;
    tokio::time::timeout(wait, rx)
        .await
        .map_err(|_| refused("the extension took too long to read"))?
        .map_err(|_| refused("the extension could not be read"))
}

pub(super) async fn choose(app: AppHandle) -> WisResult<Option<StagedExtension>> {
    let Some(source) = on_main(&app, CHOOSE_WAIT, |tx| {
        let _ = tx.send(ask_source());
    })
    .await?
    else {
        return Ok(None);
    };

    let staging = extensions_dir(&app)?.join(STAGING_DIR);
    let token = uuid::Uuid::new_v4().simple().to_string();
    let dir = staging.join(&token);
    let copied = {
        let dir = dir.clone();
        blocking(move || {
            if !SWEPT.swap(true, std::sync::atomic::Ordering::SeqCst) {
                let _ = std::fs::remove_dir_all(&staging);
            }
            std::fs::create_dir_all(&staging)
                .map_err(|err| refused(format!("create the staging folder: {err}")))?;
            if source.is_dir() {
                stage::copy_folder(&source, &dir)?;
            } else {
                stage::unzip(&source, &dir)?;
            }
            let root = stage::manifest_root(&dir)
                .ok_or_else(|| refused("there is no manifest.json at the top of the extension"))?;
            let manifest = stage::read_manifest(&root)?;
            Ok((
                root.clone(),
                stage::declared_permissions(&manifest),
                stage::default_rules(&root, &manifest),
            ))
        })
        .await
    };
    let (root, declared, default_rules) = match copied {
        Ok(found) => found,
        Err(err) => {
            discard_dir(dir).await;
            return Err(err);
        }
    };

    let read_root = root.clone();
    let facts = on_main(&app, READ_WAIT, move |tx| {
        let Some(mtm) = objc2_06::MainThreadMarker::new() else {
            return;
        };
        host::load_extension(&read_root, mtm, move |result| {
            let _ = tx.send(result.map(|extension| host::facts_of(&extension)));
        });
    })
    .await
    .and_then(|read| read.map_err(refused));
    let facts = match facts {
        Ok(facts) => facts,
        Err(err) => {
            discard_dir(dir).await;
            return Err(err);
        }
    };

    let review = extensions::review(&ReviewInput {
        name: &facts.name,
        version: &facts.version,
        description: &facts.description,
        declared: &declared,
        recognised: &facts.recognised,
        patterns: &facts.patterns,
        default_rules,
    });
    staged(|map| {
        map.insert(
            token.clone(),
            Staged {
                dir,
                root,
                name: review.name.clone(),
                version: review.version.clone(),
            },
        )
    });
    Ok(Some(StagedExtension { token, review }))
}

async fn discard_dir(dir: PathBuf) {
    let _ = blocking(move || {
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    })
    .await;
}

fn take_staged(token: &str) -> WisResult<Staged> {
    if !extensions::is_extension_id(token) {
        return Err(refused("not a staged extension"));
    }
    staged(|map| map.remove(token))
        .ok_or_else(|| refused("that extension is no longer waiting to be installed"))
}

pub(super) async fn discard(token: String) -> WisResult<()> {
    let staged = take_staged(&token)?;
    discard_dir(staged.dir).await;
    Ok(())
}

pub(super) async fn install(
    app: AppHandle,
    state: State<'_, AppState>,
    token: String,
) -> WisResult<()> {
    let staged = take_staged(&token)?;
    let id = uuid::Uuid::new_v4().simple().to_string();
    let target = extensions_dir(&app)?.join(&id);
    {
        let target = target.clone();
        let staged_dir = staged.dir.clone();
        let root = staged.root.clone();
        blocking(move || {
            let moved = std::fs::rename(&root, &target).map_err(|err| {
                refused(format!(
                    "the extension could not be moved into place: {err}"
                ))
            });
            let _ = std::fs::remove_dir_all(&staged_dir);
            moved
        })
        .await?;
    }

    let ext = InstalledExtension {
        id,
        name: staged.name,
        version: staged.version,
        ..Default::default()
    };
    let added = change(app, state, move |list| {
        extensions::with_extension(list, ext)
    })
    .await;
    if added.is_err() {
        discard_dir(target).await;
    }

    added
}

pub(super) async fn remove(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> WisResult<()> {
    if !extensions::is_extension_id(&id) {
        return Err(refused("not an extension id"));
    }

    let dir = extensions_dir(&app)?.join(&id);
    change(app.clone(), state.clone(), move |list| {
        Ok(extensions::without_extension(list, &id))
    })
    .await?;
    clear_removed_data(&app, &state).await;

    blocking(move || match std::fs::remove_dir_all(&dir) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(refused(format!(
            "the extension's files could not be removed: {err}"
        ))),
    })
    .await
}

/// Applies `edit` to the installed list, then brings every live workspace in
/// line with it and has the windows rebuild the pages whose set changed.
pub(super) async fn change(
    app: AppHandle,
    state: State<'_, AppState>,
    edit: impl FnOnce(&[InstalledExtension]) -> WisResult<Vec<InstalledExtension>> + Send,
) -> WisResult<()> {
    let before =
        crate::commands::mutate_with(&app, &state, crate::commands::Persist::Immediate, |s| {
            let next = edit(&s.settings.extensions)?;
            Ok((s.with_extensions(next), s.settings.extensions.clone()))
        })
        .await?;
    reload(&app, &state, &before).await
}

async fn reload(
    app: &AppHandle,
    state: &State<'_, AppState>,
    before: &[InstalledExtension],
) -> WisResult<()> {
    let (wanted, targets) = {
        let browser = state.browser.lock().await;
        (
            super::wanted_by_workspace(&browser),
            super::rebuild_targets(before, &browser),
        )
    };
    let dir = extensions_dir(app)?;
    app.run_on_main_thread(move || host::sync_all(dir, wanted))
        .map_err(|err| refused(format!("run_on_main_thread: {err}")))?;
    if let Err(err) = app.emit(super::EXTENSIONS_CHANGED_EVENT, targets) {
        log::warn!("open tabs were not told the extensions changed: {err}");
    }
    Ok(())
}

/// Clears what extensions no longer installed stored, in every regular
/// workspace, including what earlier removals left.
async fn clear_removed_data(app: &AppHandle, state: &State<'_, AppState>) {
    let (workspaces, keep) = {
        let browser = state.browser.lock().await;
        let workspaces: Vec<_> = browser
            .workspaces
            .iter()
            .filter(|w| !w.ephemeral)
            .map(|w| w.id)
            .collect();
        let keep: HashSet<String> = browser
            .settings
            .extensions
            .iter()
            .map(|e| e.id.clone())
            .collect();
        (workspaces, keep)
    };
    if let Err(err) = app.run_on_main_thread(move || host::clear_data(workspaces, keep)) {
        log::warn!("removed extensions' data was not cleared: {err}");
    }
}

/// Extension folders that are not installed.
fn orphans(names: &[String], installed: &HashSet<String>) -> Vec<String> {
    names
        .iter()
        .filter(|name| extensions::is_extension_id(name) && !installed.contains(*name))
        .cloned()
        .collect()
}

pub(super) async fn sweep_orphans(app: AppHandle, installed: HashSet<String>) {
    let Some(dir) = extensions_path(&app) else {
        return;
    };
    let _ = blocking(move || {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Ok(());
        };
        let names: Vec<String> = entries
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        for name in orphans(&names, &installed) {
            if let Err(err) = std::fs::remove_dir_all(dir.join(&name)) {
                log::warn!("an orphaned extension folder was not removed: {err}");
            }
        }
        Ok(())
    })
    .await;
}

/// The native open panel, for a folder or a `.zip`. Main thread only.
fn ask_source() -> Option<PathBuf> {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;

    let ns_string = |text: &str| -> *mut AnyObject {
        let c = CString::new(text).unwrap_or_default();
        // SAFETY: a class factory taking a NUL-terminated UTF-8 string.
        unsafe { msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()] }
    };

    // SAFETY: AppKit on the main thread; every object is checked for nil.
    unsafe {
        let panel: *mut AnyObject = msg_send![class!(NSOpenPanel), openPanel];
        if panel.is_null() {
            return None;
        }

        let _: () = msg_send![panel, setCanChooseFiles: true];
        let _: () = msg_send![panel, setCanChooseDirectories: true];
        let _: () = msg_send![panel, setAllowsMultipleSelection: false];
        let _: () =
            msg_send![panel, setMessage: ns_string("Choose an extension's folder or .zip file")];
        let _: () = msg_send![panel, setPrompt: ns_string("Review")];
        let zip = ns_string("zip");
        let types: *mut AnyObject = msg_send![class!(NSArray), arrayWithObject: zip];
        let _: () = msg_send![panel, setAllowedFileTypes: types];

        let response: isize = msg_send![panel, runModal];
        if response != 1 {
            return None;
        }
        let url: *mut AnyObject = msg_send![panel, URL];
        if url.is_null() {
            return None;
        }
        let path: *mut AnyObject = msg_send![url, path];
        if path.is_null() {
            return None;
        }
        let utf8: *const c_char = msg_send![path, UTF8String];
        if utf8.is_null() {
            return None;
        }

        Some(PathBuf::from(
            CStr::from_ptr(utf8).to_string_lossy().into_owned(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use objc2::runtime::{AnyObject, Bool};
    use objc2::{class, sel};

    type Obj = *mut AnyObject;

    #[test]
    fn only_uninstalled_extension_folders_are_orphans() {
        let installed: std::collections::HashSet<String> =
            ["0123456789abcdef0123456789abcdef".to_string()].into();
        let names: Vec<String> = [
            "0123456789abcdef0123456789abcdef",
            "fedcba9876543210fedcba9876543210",
            "staging",
            ".DS_Store",
            "notes",
        ]
        .iter()
        .map(|n| n.to_string())
        .collect();
        assert_eq!(
            super::orphans(&names, &installed),
            vec!["fedcba9876543210fedcba9876543210".to_string()]
        );
    }

    /// A wrong message type aborts at runtime only.
    #[test]
    fn every_panel_send_matches_its_method() {
        let panel = class!(NSOpenPanel);
        panel
            .metaclass()
            .verify_sel::<(), Obj>(sel!(openPanel))
            .unwrap();
        for setter in [
            sel!(setCanChooseFiles:),
            sel!(setCanChooseDirectories:),
            sel!(setAllowsMultipleSelection:),
        ] {
            panel.verify_sel::<(Bool,), ()>(setter).unwrap();
        }
        for setter in [
            sel!(setMessage:),
            sel!(setPrompt:),
            sel!(setAllowedFileTypes:),
        ] {
            panel.verify_sel::<(Obj,), ()>(setter).unwrap();
        }
        panel.verify_sel::<(), isize>(sel!(runModal)).unwrap();
        panel.verify_sel::<(), Obj>(sel!(URL)).unwrap();
        class!(NSArray)
            .metaclass()
            .verify_sel::<(Obj,), Obj>(sel!(arrayWithObject:))
            .unwrap();
        class!(NSURL).verify_sel::<(), Obj>(sel!(path)).unwrap();
    }
}
