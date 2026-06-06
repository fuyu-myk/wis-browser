//! Password-vault commands.
//!
//! Unlock requires a Touch ID (or password) check; on success the master key is
//! loaded from the Keychain into [`AppState::vault_key`] for the session.
//! Listing, revealing, saving, and deleting require the vault to be unlocked.
//! Plaintext passwords cross the IPC boundary only on an explicit reveal and
//! are never logged.

use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use wis_core::{TabId, WisError, WisResult};
use wis_secrets::EncryptedBlob;

use crate::state::AppState;

/// Lock state + whether a vault has ever been set up (a master key exists).
#[derive(Debug, Serialize)]
pub struct VaultStatus {
    pub unlocked: bool,
    pub has_master_key: bool,
    /// False on platforms without Keychain/biometric support.
    pub supported: bool,
}

/// A credential without its password (safe to list).
#[derive(Debug, Serialize)]
pub struct CredentialSummary {
    pub id: i64,
    pub host: String,
    pub username: String,
    pub updated_at: String,
}

#[tauri::command]
pub async fn vault_status(state: State<'_, AppState>) -> WisResult<VaultStatus> {
    let unlocked = state.vault_key.lock().await.is_some();
    let supported = cfg!(target_os = "macos");
    let has_master_key = if supported {
        // No biometric prompt: this only checks for the item's existence.
        wis_secrets::keychain_load()
            .map(|k| k.is_some())
            .unwrap_or(false)
    } else {
        false
    };

    Ok(VaultStatus {
        unlocked,
        has_master_key,
        supported,
    })
}

/// Authenticate (Touch ID / password) and load the master key for the session.
/// On first use this creates and stores a new master key. Returns whether the
/// vault is now unlocked.
#[tauri::command]
pub async fn vault_unlock(_app: AppHandle, state: State<'_, AppState>) -> WisResult<bool> {
    if !cfg!(target_os = "macos") {
        return Err(unsupported());
    }
    let authed = crate::biometrics::authenticate("Unlock your saved passwords".to_string()).await;
    if !authed {
        return Ok(false);
    }

    let key = match wis_secrets::keychain_load() {
        Ok(Some(key)) => key,
        Ok(None) => create_master_key()?,
        Err(err) => {
            log::warn!("vault: stored key unreadable ({err}); recreating master key");
            create_master_key()?
        }
    };
    *state.vault_key.lock().await = Some(key);

    Ok(true)
}

/// Generate a fresh master key and persist it to the Keychain.
fn create_master_key() -> WisResult<wis_secrets::MasterKey> {
    let key = wis_secrets::generate_master_key();
    wis_secrets::keychain_store(&key).map_err(vault_err)?;
    Ok(key)
}

/// Drop the in-memory master key; the vault returns to the locked state.
#[tauri::command]
pub async fn vault_lock(state: State<'_, AppState>) -> WisResult<()> {
    *state.vault_key.lock().await = None;
    Ok(())
}

#[tauri::command]
pub async fn vault_list(
    state: State<'_, AppState>,
    host: Option<String>,
) -> WisResult<Vec<CredentialSummary>> {
    require_unlocked(&state).await?;
    let storage = state.storage.clone();
    let records = run_blocking(move || storage.list_credentials(host.as_deref())).await?;

    Ok(records
        .into_iter()
        .map(|r| CredentialSummary {
            id: r.id,
            host: r.host,
            username: r.username,
            updated_at: r.updated_at.to_rfc3339(),
        })
        .collect())
}

#[tauri::command]
pub async fn vault_save(
    state: State<'_, AppState>,
    host: String,
    username: String,
    password: String,
) -> WisResult<()> {
    let blob = {
        let guard = state.vault_key.lock().await;
        let key = guard.as_ref().ok_or_else(locked)?;
        wis_secrets::encrypt(key, password.as_bytes()).map_err(vault_err)?
    };
    let storage = state.storage.clone();

    run_blocking(move || {
        storage
            .save_credential(&host, &username, &blob.ciphertext, &blob.nonce)
            .map(|_| ())
    })
    .await
}

/// Decrypt and return one stored password. Requires the vault to be unlocked.
#[tauri::command]
pub async fn vault_reveal(state: State<'_, AppState>, id: i64) -> WisResult<String> {
    let storage = state.storage.clone();
    let record = run_blocking(move || storage.get_credential(id))
        .await?
        .ok_or_else(|| WisError::InvalidInput {
            message: "credential not found".to_string(),
        })?;

    let guard = state.vault_key.lock().await;
    let key = guard.as_ref().ok_or_else(locked)?;
    let blob = EncryptedBlob {
        ciphertext: record.ciphertext,
        nonce: record.nonce,
    };
    let plaintext = wis_secrets::decrypt(key, &blob).map_err(vault_err)?;

    String::from_utf8(plaintext).map_err(|_| WisError::InvalidInput {
        message: "stored password is not valid UTF-8".to_string(),
    })
}

#[tauri::command]
pub async fn vault_delete(state: State<'_, AppState>, id: i64) -> WisResult<()> {
    require_unlocked(&state).await?;
    let storage = state.storage.clone();
    run_blocking(move || storage.delete_credential(id)).await
}

/// Whether any credential is saved for `host`.
#[tauri::command]
pub async fn vault_has_credentials(state: State<'_, AppState>, host: String) -> WisResult<bool> {
    let storage = state.storage.clone();
    let records = run_blocking(move || storage.list_credentials(Some(&host))).await?;
    Ok(!records.is_empty())
}

/// Saved logins for `host` (id + username only) for the URL-bar account picker.
/// Unlike [`vault_list`], this does *not* require the vault to be unlocked: it
/// exposes only the (non-secret) host/username so the picker can be shown
/// before Touch ID; filling still unlocks + decrypts via [`autofill_fill`].
#[tauri::command]
pub async fn autofill_candidates(
    state: State<'_, AppState>,
    host: String,
) -> WisResult<Vec<CredentialSummary>> {
    let storage = state.storage.clone();
    let records = run_blocking(move || storage.list_credentials(Some(&host))).await?;

    Ok(records
        .into_iter()
        .map(|r| CredentialSummary {
            id: r.id,
            host: r.host,
            username: r.username,
            updated_at: r.updated_at.to_rfc3339(),
        })
        .collect())
}

/// Fill a tab's login form with a saved credential. The password is decrypted
/// in the backend and injected straight into the page via `eval`.
/// Refuses to fill unless the page's host matches the saved host.
#[tauri::command]
pub async fn autofill_fill(
    app: AppHandle,
    state: State<'_, AppState>,
    tab_id: TabId,
    credential_id: i64,
) -> WisResult<()> {
    let storage = state.storage.clone();
    let record = run_blocking(move || storage.get_credential(credential_id))
        .await?
        .ok_or_else(|| WisError::InvalidInput {
            message: "credential not found".to_string(),
        })?;

    let label = format!("tab-{}", tab_id.as_uuid());
    let webview = app
        .webviews()
        .get(&label)
        .cloned()
        .ok_or_else(|| WisError::InvalidInput {
            message: format!("no webview {label}"),
        })?;

    // Host gate: only fill when the live page host equals the saved host.
    let live_host = webview
        .url()
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()));
    let matches = live_host
        .as_deref()
        .map(strip_www)
        .map(|h| h == strip_www(&record.host))
        .unwrap_or(false);
    if !matches {
        return Err(WisError::InvalidInput {
            message: "page host does not match the saved login".to_string(),
        });
    }

    let username = record.username.clone();
    let password = {
        let guard = state.vault_key.lock().await;
        let key = guard.as_ref().ok_or_else(locked)?;
        let plaintext = wis_secrets::decrypt(
            key,
            &EncryptedBlob {
                ciphertext: record.ciphertext,
                nonce: record.nonce,
            },
        )
        .map_err(vault_err)?;

        String::from_utf8(plaintext).map_err(|_| WisError::InvalidInput {
            message: "stored password is not valid UTF-8".to_string(),
        })?
    };

    let user_js = serde_json::to_string(&username).unwrap_or_else(|_| "\"\"".to_string());
    let pass_js = serde_json::to_string(&password).unwrap_or_else(|_| "\"\"".to_string());
    let script = fill_script(&user_js, &pass_js);

    webview.eval(&script).map_err(|err| WisError::InvalidInput {
        message: format!("autofill eval failed: {err}"),
    })
}

fn strip_www(host: &str) -> &str {
    host.strip_prefix("www.").unwrap_or(host)
}

/// JS that fills the page's password field and the nearest preceding text/email
/// field, dispatching input/change so frameworks notice. `user`/`pass` are
/// already JSON string literals.
fn fill_script(user: &str, pass: &str) -> String {
    format!(
        r#"(function(){{
  try {{
    var pw = document.querySelector('input[type=password]:not([disabled]):not([readonly])');
    if (!pw) return;
    var inputs = Array.prototype.slice.call(document.querySelectorAll('input'));
    var idx = inputs.indexOf(pw);
    var user = null;
    for (var i = idx - 1; i >= 0; i--) {{
      var t = (inputs[i].type || 'text').toLowerCase();
      if (t === 'text' || t === 'email' || t === 'tel') {{ user = inputs[i]; break; }}
    }}
    function setVal(el, val) {{
      var d = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(el), 'value');
      if (d && d.set) {{ d.set.call(el, val); }} else {{ el.value = val; }}
      el.dispatchEvent(new Event('input', {{ bubbles: true }}));
      el.dispatchEvent(new Event('change', {{ bubbles: true }}));
    }}
    if (user) setVal(user, {user});
    setVal(pw, {pass});
  }} catch (e) {{}}
}})();"#,
        user = user,
        pass = pass,
    )
}

// -- helpers ----------------------------------------------------------------

async fn require_unlocked(state: &State<'_, AppState>) -> WisResult<()> {
    if state.vault_key.lock().await.is_some() {
        Ok(())
    } else {
        Err(locked())
    }
}

fn locked() -> WisError {
    WisError::InvalidInput {
        message: "vault is locked".to_string(),
    }
}

fn unsupported() -> WisError {
    WisError::InvalidInput {
        message: "the password vault is only available on macOS".to_string(),
    }
}

fn vault_err(err: wis_secrets::VaultError) -> WisError {
    WisError::InvalidInput {
        message: format!("vault: {err}"),
    }
}

async fn run_blocking<T, F>(op: F) -> WisResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> wis_storage::StorageResult<T> + Send + 'static,
{
    tokio::task::spawn_blocking(op)
        .await
        .map_err(|err| WisError::PersistenceFailed {
            reason: format!("vault task panicked: {err}"),
        })?
        .map_err(|err| WisError::PersistenceFailed {
            reason: err.to_string(),
        })
}
