//! Password-vault commands.
//!
//! Unlock requires a biometric check (Touch ID on macOS, Windows Hello on
//! Windows, with the OS password/PIN fallback where offered); on success the
//! master key is loaded from the OS key store (Keychain / DPAPI-sealed blob)
//! into [`AppState::vault_key`] for the session.
//! Listing, revealing, saving, and deleting require the vault to be unlocked.
//! Plaintext passwords cross the IPC boundary only on an explicit reveal and
//! are never logged.

use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use wis_core::backup::CredentialBackup;
use wis_core::{TabId, WisError, WisResult};
use wis_secrets::EncryptedBlob;

use crate::state::AppState;

/// Lock state + whether a vault has ever been set up (a master key exists).
#[derive(Debug, Serialize)]
pub struct VaultStatus {
    pub unlocked: bool,
    pub has_master_key: bool,
    /// False on platforms without an OS key store + biometric gate.
    pub supported: bool,
}

/// Platforms with both an OS key store and a biometric gate: macOS
/// (Keychain + Touch ID) and Windows (DPAPI + Windows Hello).
fn vault_supported() -> bool {
    cfg!(any(target_os = "macos", windows))
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
    let supported = vault_supported();
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
    if !vault_supported() {
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

/// Touch-ID-authorize a vault backup operation and load the master key for the
/// session. **Always** prompts (even when the vault is already unlocked), since
/// exporting or restoring saved passwords is an explicit, sensitive action the
/// user should confirm each time. `reason` is shown in the system prompt.
pub(crate) async fn authorize_vault(state: &State<'_, AppState>, reason: &str) -> WisResult<()> {
    if !vault_supported() {
        return Err(unsupported());
    }

    let authed = crate::biometrics::authenticate(reason.to_string()).await;
    if !authed {
        return Err(WisError::InvalidInput {
            message: "Authentication was cancelled".to_string(),
        });
    }

    if state.vault_key.lock().await.is_none() {
        let key = match wis_secrets::keychain_load() {
            Ok(Some(key)) => key,
            Ok(None) => create_master_key()?,
            Err(err) => {
                log::warn!("vault: stored key unreadable ({err}); recreating master key");
                create_master_key()?
            }
        };
        *state.vault_key.lock().await = Some(key);
    }

    Ok(())
}

/// Decrypt every saved login for an export, unlocking the vault first
/// (biometric prompt shows `reason`; always prompts, even when unlocked).
///
/// Callers own what happens to the plaintext: the encrypted backup seals it
/// under a passphrase, the CSV export writes it after an explicit confirm.
pub(crate) async fn export_credentials(
    state: &State<'_, AppState>,
    reason: &str,
) -> WisResult<Vec<CredentialBackup>> {
    authorize_vault(state, reason).await?;
    let storage = state.storage.clone();
    let records = run_blocking(move || storage.list_credentials(None)).await?;

    let guard = state.vault_key.lock().await;
    let key = guard.as_ref().ok_or_else(locked)?;
    let mut out = Vec::with_capacity(records.len());
    for record in records {
        let plaintext = wis_secrets::decrypt(
            key,
            &EncryptedBlob {
                ciphertext: record.ciphertext,
                nonce: record.nonce,
            },
        )
        .map_err(vault_err)?;
        let password = String::from_utf8(plaintext).map_err(|_| WisError::InvalidInput {
            message: "a stored password is not valid UTF-8".to_string(),
        })?;

        out.push(CredentialBackup {
            host: record.host,
            username: record.username,
            password,
        });
    }

    Ok(out)
}

/// Re-encrypt imported logins under this device's vault key and save them,
/// unlocking first (biometric prompt shows `reason`). Upserts by
/// `(host, username)`, so a re-import updates rather than duplicates.
/// Returns the count written.
pub(crate) async fn import_credentials(
    state: &State<'_, AppState>,
    creds: Vec<CredentialBackup>,
    reason: &str,
) -> WisResult<usize> {
    if creds.is_empty() {
        return Ok(0);
    }
    authorize_vault(state, reason).await?;

    let sealed: Vec<(String, String, EncryptedBlob)> = {
        let guard = state.vault_key.lock().await;
        let key = guard.as_ref().ok_or_else(locked)?;
        creds
            .into_iter()
            .map(|c| {
                let blob = wis_secrets::encrypt(key, c.password.as_bytes()).map_err(vault_err)?;
                Ok((c.host, c.username, blob))
            })
            .collect::<WisResult<Vec<_>>>()?
    };

    let count = sealed.len();
    let storage = state.storage.clone();
    run_blocking(move || {
        for (host, username, blob) in &sealed {
            storage.save_credential(host, username, &blob.ciphertext, &blob.nonce)?;
        }
        Ok(())
    })
    .await?;

    Ok(count)
}

/// Outcome of a password CSV import.
#[derive(Debug, Serialize)]
pub struct CsvImportStats {
    pub imported: usize,
    pub skipped: usize,
}

/// Pick a password CSV (a Chrome/Firefox/Safari export) and merge it into the
/// vault after a biometric confirm. Upserts by `(host, username)`. Resolves
/// `None` when the user cancels the picker.
#[tauri::command]
pub async fn vault_import_csv(
    app: AppHandle,
    state: State<'_, AppState>,
) -> WisResult<Option<CsvImportStats>> {
    if !vault_supported() {
        return Err(unsupported());
    }
    let Some(path) = crate::panels::run_open_panel(&app, &["csv", "txt"])? else {
        return Ok(None);
    };

    // Bounded read + parse off the async runtime; the file is untrusted.
    let parsed = tokio::task::spawn_blocking(move || -> WisResult<wis_core::ParsedCredentials> {
        let source =
            crate::file_io::read_capped(&path, wis_core::password_csv::MAX_PASSWORD_CSV_BYTES)?;
        wis_core::parse_password_csv(&source)
    })
    .await
    .map_err(|err| WisError::InvalidInput {
        message: format!("import task failed: {err}"),
    })??;

    let skipped = parsed.skipped;
    let creds: Vec<CredentialBackup> = parsed
        .credentials
        .into_iter()
        .map(|c| CredentialBackup {
            host: c.host,
            username: c.username,
            password: c.password,
        })
        .collect();
    let imported = import_credentials(&state, creds, "Import passwords into your vault").await?;

    Ok(Some(CsvImportStats { imported, skipped }))
}

/// Write every saved login to a plaintext CSV at a user-picked path, after a
/// biometric confirm (always prompts, even when unlocked; the UI additionally
/// shows an explicit plaintext warning first). Resolves `false` when the user
/// cancels the save panel.
#[tauri::command]
pub async fn vault_export_csv(app: AppHandle, state: State<'_, AppState>) -> WisResult<bool> {
    if !vault_supported() {
        return Err(unsupported());
    }
    let Some(path) = crate::panels::run_save_panel(&app, &suggested_csv_name())? else {
        return Ok(false);
    };

    let creds = export_credentials(&state, "Export your saved passwords to a file").await?;
    let rows: Vec<wis_core::ParsedCredential> = creds
        .into_iter()
        .map(|c| wis_core::ParsedCredential {
            host: c.host,
            username: c.username,
            password: c.password,
        })
        .collect();
    let csv = wis_core::to_password_csv(&rows);
    write_private(&path, &csv).map_err(|err| WisError::InvalidInput {
        message: format!("write {}: {err}", path.display()),
    })?;

    Ok(true)
}

/// Write the plaintext export readable by the owning user only (Unix mode
/// 0600 at creation); Windows inherits the destination folder's ACLs.
fn write_private(path: &std::path::Path, contents: &str) -> std::io::Result<()> {
    use std::io::Write;

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    options.open(path)?.write_all(contents.as_bytes())
}

fn suggested_csv_name() -> String {
    let stamp = chrono::Local::now().format("%Y-%m-%d");
    format!(
        "{}.csv",
        wis_core::sanitize_filename(&format!("Wis Passwords {stamp}"))
    )
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

/// Whether any credential is saved for `host`. Matches on the `www.`-stripped
/// host (capture stores the stripped form).
#[tauri::command]
pub async fn vault_has_credentials(state: State<'_, AppState>, host: String) -> WisResult<bool> {
    let host = strip_www(&host).to_string();
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
    let host = strip_www(&host).to_string();
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
        message: "the password vault is not available on this platform".to_string(),
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
