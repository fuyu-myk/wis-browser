//! A recovery copy of the vault key, sealed to the user's own public key, and
//! erasing the vault to start over.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use wis_core::{WisError, WisResult};
use wis_secrets::recovery::{self, Recipient, RecoveryError, SecretString};
use wis_secrets::EncryptedBlob;
use zeroize::Zeroizing;

use crate::state::AppState;
use crate::vault::{load_key, locked, unsupported, vault_supported};

const RECOVERY_FILE: &str = "vault-recovery.json";
const MAX_RECOVERY_FILE_BYTES: u64 = 32 * 1024;
const MAX_SSH_FILES: usize = 200;

static COPY_WRITES: Mutex<()> = Mutex::new(());
static ERASE_DIALOG_OPEN: AtomicBool = AtomicBool::new(false);

#[derive(Serialize, Deserialize)]
struct RecoveryCopy {
    recipient: String,
    sealed: String,
    check: String,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RecoverOutcome {
    Cancelled,
    Recovered,
    Partial { opened: usize, total: usize },
}

/// The public key the vault key is sealed to, if recovery is set up. A copy
/// that no longer matches the unlocked key is stale and is removed.
#[tauri::command]
pub async fn vault_recovery_key(
    app: AppHandle,
    state: State<'_, AppState>,
) -> WisResult<Option<String>> {
    let path = copy_path(&app)?;
    let read_from = path.clone();
    let Some(copy) = blocking(move || read_copy(&read_from)).await? else {
        return Ok(None);
    };
    let current = state
        .vault_key
        .lock()
        .await
        .as_ref()
        .map(recovery::check_value);
    match current {
        Some(check) if check != copy.check => {
            blocking(move || remove_if_stale(&path, &check)).await?;
            Ok(None)
        }
        _ => Ok(Some(copy.recipient)),
    }
}

/// Seal the vault key to `public_key` (an age or SSH public key) after a
/// biometric check, replacing any earlier recovery copy. An SSH key whose
/// private half sits in `~/.ssh` without a passphrase is refused.
#[tauri::command]
pub async fn vault_set_recovery_key(
    app: AppHandle,
    state: State<'_, AppState>,
    public_key: String,
    vault_password: Option<String>,
) -> WisResult<String> {
    let vault_password = vault_password.map(Zeroizing::new);
    let recipient = recovery::recipient(&public_key).map_err(recovery_err)?;
    if recipient.is_ssh() {
        let ssh_dir = app
            .path()
            .home_dir()
            .map(|home| home.join(".ssh"))
            .map_err(|err| persist_err(format!("home dir: {err}")))?;
        let key = recipient.to_string();
        blocking(move || refuse_unprotected_local_key(&ssh_dir, &key)).await?;
    }
    crate::vault::authorize_vault(
        &state,
        "Set up recovery for your saved passwords",
        vault_password.as_deref().map(String::as_str),
    )
    .await?;
    seal_to(&app, &state, &recipient).await?;
    Ok(recipient.to_string())
}

/// Delete the recovery copy once the user confirms it is them. Resolves
/// `false` when they do not.
#[tauri::command]
pub async fn vault_remove_recovery_key(
    app: AppHandle,
    state: State<'_, AppState>,
    vault_password: Option<String>,
) -> WisResult<bool> {
    let vault_password = vault_password.map(Zeroizing::new);
    let confirmed = crate::vault_store::confirm_user(
        &state,
        "Remove recovery for your saved passwords",
        vault_password.as_deref().map(String::as_str),
    )
    .await?;
    if !confirmed {
        return Ok(false);
    }
    let path = copy_path(&app)?;
    blocking(move || remove_copy(&path)).await?;
    Ok(true)
}

/// Open the recovery copy with a private key the user picks and put it back
/// where the key is kept when it opens every saved password, or with `force`
/// when it opens some. A biometric check comes first; with a vault password,
/// the private key alone is the proof and `new_password` seals the key again.
#[tauri::command]
pub async fn vault_recover(
    app: AppHandle,
    state: State<'_, AppState>,
    passphrase: Option<String>,
    force: bool,
    new_password: Option<String>,
) -> WisResult<RecoverOutcome> {
    if !vault_supported() {
        return Err(unsupported());
    }

    let new_password = new_password.map(Zeroizing::new);
    let uses_password = crate::vault_store::uses_password();
    crate::vault_store::check_password_for_mode(new_password.as_deref().map(String::as_str))?;
    let path = copy_path(&app)?;
    let Some(copy) = blocking(move || read_copy(&path)).await? else {
        return Err(invalid("No recovery key is set up on this device"));
    };
    let storage = state.storage.clone();
    if !blocking(move || crate::vault_store::has_sealed_data(&*storage).map_err(persist_err))
        .await?
    {
        return Err(invalid("There is nothing saved to recover"));
    }
    if !uses_password
        && !crate::vault_store::confirm_user(&state, "Recover your saved passwords", None).await?
    {
        return Ok(RecoverOutcome::Cancelled);
    }
    let Some(private_path) = crate::panels::run_open_panel(&app, &[])? else {
        return Ok(RecoverOutcome::Cancelled);
    };

    let storage = state.storage.clone();
    let (key, opened, total) = blocking(move || {
        let private_key = read_secret(&private_path, recovery::MAX_KEY_FILE_BYTES)?;
        let passphrase = passphrase
            .filter(|text| !text.is_empty())
            .map(SecretString::from);
        let key = recovery::open(&copy.sealed, &private_key, passphrase).map_err(recovery_err)?;
        let passwords = storage.list_credentials(None).map_err(persist_err)?;
        let addresses = storage.list_addresses().map_err(persist_err)?;
        let total = passwords.len() + addresses.len();
        let opened_passwords = passwords
            .into_iter()
            .filter(|row| {
                let blob = EncryptedBlob {
                    ciphertext: row.ciphertext.clone(),
                    nonce: row.nonce.clone(),
                };
                wis_secrets::decrypt(&key, &blob)
                    .map(Zeroizing::new)
                    .is_ok()
            })
            .count();
        let opened_addresses = addresses
            .iter()
            .filter(|row| crate::addresses::opens(&key, row))
            .count();
        Ok((key, opened_passwords + opened_addresses, total))
    })
    .await?;
    if total == 0 {
        return Err(invalid("There is nothing saved to recover"));
    }
    if opened == 0 {
        return Err(invalid("This key doesn't open anything you saved"));
    }
    if opened < total && !force {
        return Ok(RecoverOutcome::Partial { opened, total });
    }

    let mut slot = state.vault_key.lock().await;
    let storage = state.storage.clone();
    let restored = blocking(move || {
        if !crate::vault_store::has_sealed_data(&*storage).map_err(persist_err)? {
            return Err(invalid("There is nothing saved to recover"));
        }
        crate::vault_store::restore_key(&key, new_password.as_deref().map(String::as_str))?;
        let addresses = storage.list_addresses().map_err(persist_err)?;
        addresses
            .iter()
            .filter(|row| !crate::addresses::opens(&key, row))
            .try_for_each(|row| storage.delete_address(row.id))
            .map_err(persist_err)?;
        Ok(key)
    })
    .await?;
    *slot = Some(restored);

    Ok(RecoverOutcome::Recovered)
}

/// Erase the vault key, every saved password and the recovery copy, then
/// start a new vault, sealed with `new_password` where a vault password is
/// used. A biometric check comes first; a vault password is not asked for.
/// The key goes first, so a key store that refuses leaves everything as it
/// was. Resolves `false` when the check is cancelled.
#[tauri::command]
pub async fn vault_reset(
    app: AppHandle,
    state: State<'_, AppState>,
    new_password: Option<String>,
) -> WisResult<bool> {
    if !vault_supported() {
        return Err(unsupported());
    }

    let new_password = new_password.map(Zeroizing::new);
    let uses_password = crate::vault_store::uses_password();
    crate::vault_store::check_password_for_mode(new_password.as_deref().map(String::as_str))?;
    let confirmed = if uses_password {
        confirm_erase_natively(&app).await?
    } else {
        crate::vault_store::confirm_user(&state, "Erase your saved passwords and addresses", None)
            .await?
    };
    if !confirmed {
        return Ok(false);
    }

    {
        let mut slot = state.vault_key.lock().await;
        *slot = None;
        let storage = state.storage.clone();
        let path = copy_path(&app)?;
        let fresh = blocking(move || {
            crate::vault_store::delete_key().map_err(|err| {
                invalid(&format!(
                    "Wis couldn't remove the vault's key, so nothing was erased ({err})"
                ))
            })?;
            storage.clear_vault().map_err(persist_err)?;
            remove_copy(&path)?;

            let Some(password) = new_password else {
                return Ok(None);
            };
            let key = wis_secrets::generate_master_key();
            crate::vault_store::seal_and_store(&key, &password)?;
            Ok(Some(key))
        })
        .await?;

        if let Some(key) = fresh {
            *slot = Some(key);
        }
    }
    if !uses_password {
        load_key(&state).await?;
    }

    Ok(true)
}

async fn confirm_erase_natively(app: &AppHandle) -> WisResult<bool> {
    if ERASE_DIALOG_OPEN.swap(true, Ordering::AcqRel) {
        return Ok(false);
    }
    let app = app.clone();
    let answered = tokio::task::spawn_blocking(move || {
        crate::panels::confirm_natively(
            &app,
            "Erase every saved password and address?",
            "Wis deletes your saved passwords and addresses, the vault's key and its recovery copy, then sets up a new vault with the password you chose. This can't be undone.",
            "Erase",
        )
    })
    .await;
    ERASE_DIALOG_OPEN.store(false, Ordering::Release);
    answered.map_err(|err| persist_err(format!("vault task panicked: {err}")))?
}

fn refuse_unprotected_local_key(ssh_dir: &Path, key: &str) -> WisResult<()> {
    let Ok(entries) = std::fs::read_dir(ssh_dir) else {
        return Ok(());
    };
    let wanted = recovery::recipient(key).map_err(recovery_err)?;
    let files = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| regular_file(path))
        .take(MAX_SSH_FILES);
    for file in files {
        let (private_file, paired) = if file.extension().is_some_and(|ext| ext == "pub") {
            let pairs = crate::file_io::read_capped(&file, recovery::MAX_PUBLIC_KEY_BYTES)
                .ok()
                .is_some_and(|text| {
                    recovery::recipient(&text).is_ok_and(|found| found.same_key(&wanted))
                });
            let private_file = file.with_extension("");
            if !pairs || !regular_file(&private_file) {
                continue;
            }
            (private_file, true)
        } else {
            (file, false)
        };
        let Ok(private_key) = read_secret(&private_file, recovery::MAX_KEY_FILE_BYTES) else {
            continue;
        };
        let ours = paired || recovery::ssh_key_matches(&private_key, &wanted);
        if ours && recovery::ssh_key_has_passphrase(&private_key) == Some(false) {
            let name = private_file
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            return Err(invalid(&format!(
                "The private half of this key is on this device without a passphrase (~/.ssh/{name}), so anyone who copies your files could open your vault. Add a passphrase with ssh-keygen -p, or use a key kept elsewhere"
            )));
        }
    }

    Ok(())
}

fn regular_file(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_file())
}

async fn seal_to(
    app: &AppHandle,
    state: &State<'_, AppState>,
    recipient: &Recipient,
) -> WisResult<()> {
    let (sealed, check) = {
        let key = state.vault_key.lock().await;
        let key = key.as_ref().ok_or_else(locked)?;
        let sealed = recovery::seal(recipient, key).map_err(recovery_err)?;
        (sealed, recovery::check_value(key))
    };
    let copy = RecoveryCopy {
        recipient: recipient.to_string(),
        sealed,
        check,
    };
    let path = copy_path(app)?;
    blocking(move || write_copy(&path, &copy)).await
}

fn copy_path(app: &AppHandle) -> WisResult<PathBuf> {
    app.path()
        .app_data_dir()
        .map(|dir| dir.join(RECOVERY_FILE))
        .map_err(|err| persist_err(format!("app data dir: {err}")))
}

fn read_copy(path: &Path) -> WisResult<Option<RecoveryCopy>> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(invalid("The recovery copy on this device couldn't be read")),
    };
    let mut text = String::new();
    file.take(MAX_RECOVERY_FILE_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|_| invalid("The recovery copy on this device couldn't be read"))?;
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|_| invalid("The recovery copy on this device is damaged"))
}

fn write_copy(path: &Path, copy: &RecoveryCopy) -> WisResult<()> {
    use std::io::Write;

    let json = serde_json::to_string(copy).map_err(persist_err)?;
    let _writing = COPY_WRITES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let dir = path
        .parent()
        .ok_or_else(|| persist_err("recovery copy has no folder"))?;
    std::fs::create_dir_all(dir).map_err(persist_err)?;
    let staged = dir.join(format!(
        ".{RECOVERY_FILE}.{}",
        uuid::Uuid::new_v4().simple()
    ));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let written = options
        .open(&staged)
        .and_then(|mut file| {
            file.write_all(json.as_bytes())
                .and_then(|()| file.sync_all())
        })
        .and_then(|()| std::fs::rename(&staged, path));
    if written.is_err() {
        if let Err(err) = std::fs::remove_file(&staged) {
            log::warn!("vault: could not remove a staged recovery copy: {err}");
        }
    }
    written.map_err(persist_err)?;

    #[cfg(unix)]
    if let Ok(folder) = std::fs::File::open(dir) {
        folder.sync_all().map_err(persist_err)?;
    }

    Ok(())
}

fn remove_if_stale(path: &Path, current: &str) -> WisResult<()> {
    let _writing = COPY_WRITES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match read_copy(path)? {
        Some(copy) if copy.check != current => match std::fs::remove_file(path) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(persist_err(err)),
            _ => Ok(()),
        },
        _ => Ok(()),
    }
}

fn remove_copy(path: &Path) -> WisResult<()> {
    let _writing = COPY_WRITES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match std::fs::remove_file(path) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(persist_err(err)),
        _ => Ok(()),
    }
}

fn read_secret(path: &Path, cap: usize) -> WisResult<Zeroizing<Vec<u8>>> {
    let unreadable = || invalid("That file couldn't be read");
    let file = std::fs::File::open(path).map_err(|_| unreadable())?;
    let mut bytes = Zeroizing::new(Vec::with_capacity(cap + 1));
    file.take(cap as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| unreadable())?;
    Ok(bytes)
}

async fn blocking<T, F>(op: F) -> WisResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> WisResult<T> + Send + 'static,
{
    tokio::task::spawn_blocking(op)
        .await
        .map_err(|err| persist_err(format!("vault task panicked: {err}")))?
}

fn recovery_err(err: RecoveryError) -> WisError {
    let message = match err {
        RecoveryError::NotAPublicKey => "That isn't an age or SSH public key",
        RecoveryError::PrivateKeyGiven => {
            "That is a private key. Paste the public key and keep the private one to yourself"
        }
        RecoveryError::NotAPrivateKey => "That file isn't an age or SSH private key",
        RecoveryError::NeedsPassphrase => {
            "This key has a passphrase. Enter it and choose the key again"
        }
        RecoveryError::WrongPassphrase => "That passphrase doesn't unlock this key",
        RecoveryError::ExcessiveWork => {
            "This key's passphrase protection asks for more work than Wis will do"
        }
        RecoveryError::WrongKey => "This key doesn't open the recovery copy",
        RecoveryError::Damaged => "The recovery copy on this device is damaged",
        RecoveryError::Sealing => "The recovery copy couldn't be made",
    };
    invalid(message)
}

fn persist_err(err: impl std::fmt::Display) -> WisError {
    WisError::PersistenceFailed {
        reason: err.to_string(),
    }
}

fn invalid(message: &str) -> WisError {
    WisError::InvalidInput {
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PUBLIC: &str = include_str!("../../../secrets/testdata/recovery_ed25519.pub");
    const PRIVATE: &str = include_str!("../../../secrets/testdata/recovery_ed25519");
    const LOCKED_PUBLIC: &str =
        include_str!("../../../secrets/testdata/recovery_ed25519_locked.pub");
    const LOCKED_PRIVATE: &str = include_str!("../../../secrets/testdata/recovery_ed25519_locked");

    struct SshDir(PathBuf);

    impl SshDir {
        fn with(files: &[(&str, &str)]) -> Self {
            let dir =
                std::env::temp_dir().join(format!("wis-ssh-{}", uuid::Uuid::new_v4().simple()));
            std::fs::create_dir_all(&dir).unwrap();
            for (name, text) in files {
                std::fs::write(dir.join(name), text).unwrap();
            }
            Self(dir)
        }
    }

    impl Drop for SshDir {
        fn drop(&mut self) {
            if let Err(err) = std::fs::remove_dir_all(&self.0) {
                eprintln!("could not remove {}: {err}", self.0.display());
            }
        }
    }

    #[test]
    fn a_key_without_a_passphrase_on_this_device_is_refused() {
        let paired = SshDir::with(&[("id_ed25519", PRIVATE), ("id_ed25519.pub", PUBLIC)]);
        assert!(refuse_unprotected_local_key(&paired.0, PUBLIC).is_err());

        let alone = SshDir::with(&[("backup_key", PRIVATE)]);
        assert!(refuse_unprotected_local_key(&alone.0, PUBLIC).is_err());
    }

    #[test]
    fn a_protected_absent_or_unrelated_key_is_allowed() {
        let locked = SshDir::with(&[
            ("id_locked", LOCKED_PRIVATE),
            ("id_locked.pub", LOCKED_PUBLIC),
        ]);
        assert!(refuse_unprotected_local_key(&locked.0, LOCKED_PUBLIC).is_ok());

        let unrelated = SshDir::with(&[("id_ed25519", PRIVATE), ("id_ed25519.pub", PUBLIC)]);
        assert!(refuse_unprotected_local_key(&unrelated.0, LOCKED_PUBLIC).is_ok());

        let missing = std::env::temp_dir().join("wis-ssh-absent");
        assert!(refuse_unprotected_local_key(&missing, PUBLIC).is_ok());
    }
}
