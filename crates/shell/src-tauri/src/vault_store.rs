//! Where the vault key is kept and how the user shows it is them: the OS key
//! store behind a biometric check, or a file sealed with a vault password.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::State;
use wis_core::{WisError, WisResult};
use wis_secrets::{password_vault, MasterKey, VaultError};
use zeroize::Zeroizing;

use crate::state::AppState;
use crate::vault::{unsupported, vault_err};

const FREE_TRIES: u32 = 3;

static SEALED_KEY: OnceLock<PathBuf> = OnceLock::new();
static ATTEMPTS: Mutex<Attempts> = Mutex::new(Attempts {
    failures: 0,
    retry_at: None,
});

struct Attempts {
    failures: u32,
    retry_at: Option<Instant>,
}

/// A vault password stands in for the biometric check on platforms without
/// one, and in debug builds run with `WIS_VAULT_PASSWORD_MODE` set.
pub(crate) fn uses_password() -> bool {
    cfg!(not(any(target_os = "macos", windows)))
        || (cfg!(debug_assertions) && std::env::var_os("WIS_VAULT_PASSWORD_MODE").is_some())
}

/// Keep the sealed key in the app's data folder, beside `wis.db`.
pub(crate) fn keep_sealed_key_in(data_dir: &Path) {
    if SEALED_KEY
        .set(data_dir.join(password_vault::FILE_NAME))
        .is_err()
    {
        log::warn!("vault: the sealed key's folder was already set");
    }
}

fn sealed_key_path() -> WisResult<PathBuf> {
    SEALED_KEY
        .get()
        .cloned()
        .ok_or_else(|| WisError::PersistenceFailed {
            reason: "the vault's data folder is not known yet".to_string(),
        })
}

pub(crate) fn has_key() -> bool {
    if uses_password() {
        sealed_key_path().is_ok_and(|path| path.try_exists().unwrap_or(true))
    } else {
        wis_secrets::has_master_key().unwrap_or(true)
    }
}

pub(crate) fn delete_key() -> WisResult<()> {
    if uses_password() {
        password_vault::remove_file(&sealed_key_path()?).map_err(vault_err)
    } else {
        wis_secrets::keychain_delete().map_err(vault_err)
    }
}

/// Put `key` back where it is kept, sealed with `password` where a vault
/// password is used.
pub(crate) fn restore_key(key: &MasterKey, password: Option<&str>) -> WisResult<()> {
    if uses_password() {
        seal_and_store(
            key,
            password.ok_or_else(|| invalid("Choose a new vault password"))?,
        )
    } else {
        wis_secrets::restore_master_key(key).map_err(vault_err)
    }
}

/// Seal `key` with `password` and store it, which also clears any wait left by
/// wrong guesses at the old password.
pub(crate) fn seal_and_store(key: &MasterKey, password: &str) -> WisResult<()> {
    check_new_password(password)?;
    let sealed = password_vault::seal(key, password).map_err(vault_err)?;
    password_vault::write_file(&sealed_key_path()?, &sealed).map_err(vault_err)?;
    *ATTEMPTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Attempts {
        failures: 0,
        retry_at: None,
    };
    Ok(())
}

/// Whether `new_password` fits this device.
pub(crate) fn check_password_for_mode(new_password: Option<&str>) -> WisResult<()> {
    match (uses_password(), new_password) {
        (true, Some(password)) => check_new_password(password),
        (true, None) => Err(invalid("Choose a new vault password")),
        (false, Some(_)) => Err(invalid("This device does not use a vault password")),
        (false, None) => Ok(()),
    }
}

fn open_with_password(password: &str) -> WisResult<MasterKey> {
    let mut attempts = ATTEMPTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(wait) = attempts
        .retry_at
        .and_then(|at| at.checked_duration_since(Instant::now()))
    {
        return Err(invalid(&format!(
            "Too many wrong passwords. Try again in {} seconds",
            wait.as_secs().max(1)
        )));
    }
    let sealed = password_vault::read_file(&sealed_key_path()?)
        .map_err(vault_err)?
        .ok_or_else(|| invalid("No vault is set up on this device"))?;
    match password_vault::open(&sealed, password) {
        Ok(key) => {
            *attempts = Attempts {
                failures: 0,
                retry_at: None,
            };
            Ok(key)
        }
        Err(VaultError::WrongPassword) => {
            attempts.failures = attempts.failures.saturating_add(1);
            let wait = wait_after(attempts.failures);
            attempts.retry_at = (!wait.is_zero()).then(|| Instant::now() + wait);
            Err(invalid("Incorrect vault password"))
        }
        Err(err) => Err(vault_err(err)),
    }
}

fn wait_after(failures: u32) -> Duration {
    match failures.checked_sub(FREE_TRIES) {
        Some(over) => Duration::from_secs(1 << over.min(8)),
        None => Duration::ZERO,
    }
}

pub(crate) fn check_new_password(password: &str) -> WisResult<()> {
    if password.chars().count() < password_vault::MIN_PASSWORD_CHARS {
        return Err(invalid(&format!(
            "A vault password needs at least {} characters",
            password_vault::MIN_PASSWORD_CHARS
        )));
    }
    Ok(())
}

/// User confirmation through a biometric check, or with `password`
/// on platforms without biometrics.
pub(crate) async fn confirm_user(
    state: &State<'_, AppState>,
    reason: &str,
    password: Option<&str>,
) -> WisResult<bool> {
    if !uses_password() {
        return Ok(crate::biometrics::authenticate(reason.to_string()).await);
    }
    let Some(password) = password else {
        return Ok(false);
    };
    let password = Zeroizing::new(password.to_string());
    let mut slot = state.vault_key.lock().await;
    let key = blocking(move || open_with_password(&password)).await?;
    *slot = Some(key);
    Ok(true)
}

/// Set up a vault sealed with `password`, for a device with no vault and no
/// saved passwords.
#[tauri::command]
pub async fn vault_create_password(state: State<'_, AppState>, password: String) -> WisResult<()> {
    let password = Zeroizing::new(password);
    if !uses_password() {
        return Err(unsupported());
    }

    check_new_password(&password)?;
    let mut slot = state.vault_key.lock().await;
    let storage = state.storage.clone();
    let key = blocking(move || {
        if has_key() {
            return Err(invalid("A vault is already set up on this device"));
        }
        if storage.has_credentials().map_err(persist_err)? {
            return Err(vault_err(VaultError::Missing));
        }
        let key = wis_secrets::generate_master_key();
        seal_and_store(&key, &password)?;
        Ok(key)
    })
    .await?;
    *slot = Some(key);

    Ok(())
}

#[tauri::command]
pub async fn vault_unlock_password(state: State<'_, AppState>, password: String) -> WisResult<()> {
    let password = Zeroizing::new(password);
    if !uses_password() {
        return Err(unsupported());
    }
    let mut slot = state.vault_key.lock().await;
    let key = blocking(move || open_with_password(&password)).await?;
    *slot = Some(key);
    Ok(())
}

#[tauri::command]
pub async fn vault_change_password(
    state: State<'_, AppState>,
    current: String,
    new: String,
) -> WisResult<()> {
    let (current, new) = (Zeroizing::new(current), Zeroizing::new(new));
    if !uses_password() {
        return Err(unsupported());
    }
    check_new_password(&new)?;
    let _held = state.vault_key.lock().await;
    blocking(move || {
        let key = open_with_password(&current)?;
        seal_and_store(&key, &new)
    })
    .await
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

    #[test]
    fn wrong_passwords_wait_longer_after_three_free_tries() {
        assert_eq!(wait_after(0), Duration::ZERO);
        assert_eq!(wait_after(2), Duration::ZERO);
        assert_eq!(wait_after(3), Duration::from_secs(1));
        assert_eq!(wait_after(4), Duration::from_secs(2));
        assert_eq!(wait_after(11), Duration::from_secs(256));
        assert_eq!(wait_after(u32::MAX), Duration::from_secs(256));
    }

    #[test]
    fn a_vault_password_needs_twelve_characters() {
        assert!(check_new_password("short").is_err());
        assert!(check_new_password("elevenchars").is_err());
        assert!(check_new_password("twelve chars").is_ok());
        assert!(check_new_password("пароль123456").is_ok());
    }
}
