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
use wis_core::{TabId, WisError, WisResult, WorkspaceId};
use wis_secrets::EncryptedBlob;
use zeroize::Zeroizing;

use crate::state::AppState;

/// Lock state + whether a vault has ever been set up (a master key exists).
#[derive(Debug, Serialize)]
pub struct VaultStatus {
    pub unlocked: bool,
    pub has_master_key: bool,
    pub supported: bool,
    pub password: bool,
}

pub(crate) fn vault_supported() -> bool {
    cfg!(any(target_os = "macos", windows)) || crate::vault_store::uses_password()
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
    let has_master_key = supported && crate::vault_store::has_key();

    Ok(VaultStatus {
        unlocked,
        has_master_key,
        supported,
        password: crate::vault_store::uses_password(),
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
    if crate::vault_store::uses_password() {
        return Err(WisError::InvalidInput {
            message: "Unlock with your vault password".to_string(),
        });
    }
    let authed =
        crate::vault_store::confirm_user(&state, "Unlock your saved passwords", None).await?;
    if !authed {
        return Ok(false);
    }

    load_key(&state).await?;

    Ok(true)
}

pub(crate) async fn load_key(state: &State<'_, AppState>) -> WisResult<()> {
    let mut key = state.vault_key.lock().await;
    if key.is_none() && crate::vault_store::uses_password() {
        return Err(locked());
    }
    if key.is_none() {
        let storage = state.storage.clone();
        let loaded = tokio::task::spawn_blocking(move || {
            let saved = crate::vault_store::has_sealed_data(&*storage).unwrap_or_else(|err| {
                log::warn!("vault: could not tell whether passwords are saved: {err}");
                true
            });
            wis_secrets::master_key_or_new(!saved)
        })
        .await
        .map_err(|err| WisError::PersistenceFailed {
            reason: format!("vault task panicked: {err}"),
        })?
        .map_err(key_unavailable)?;
        *key = Some(loaded);
    }
    Ok(())
}

fn key_unavailable(err: wis_secrets::VaultError) -> WisError {
    log::warn!("vault: master key unavailable: {err}");
    let message = match err {
        wis_secrets::VaultError::Missing => "The key your saved passwords and addresses were sealed with is missing from this device, so nothing was changed".to_string(),
        err => format!("Your vault's key couldn't be loaded, so nothing was changed ({err})"),
    };
    WisError::InvalidInput { message }
}

/// Touch-ID-authorize a vault backup operation and load the master key for the
/// session. **Always** prompts (even when the vault is already unlocked), since
/// exporting or restoring saved passwords is an explicit, sensitive action the
/// user should confirm each time. `reason` is shown in the system prompt.
pub(crate) async fn authorize_vault(
    state: &State<'_, AppState>,
    reason: &str,
    password: Option<&str>,
) -> WisResult<()> {
    if !vault_supported() {
        return Err(unsupported());
    }

    let authed = crate::vault_store::confirm_user(state, reason, password).await?;
    if !authed {
        return Err(WisError::InvalidInput {
            message: "Authentication was cancelled".to_string(),
        });
    }

    load_key(state).await
}

/// Decrypt every saved login for an export, unlocking the vault first
/// (biometric prompt shows `reason`; always prompts, even when unlocked).
///
/// Callers own what happens to the plaintext: the encrypted backup seals it
/// under a passphrase, the CSV export writes it after an explicit confirm.
pub(crate) async fn export_credentials(
    state: &State<'_, AppState>,
    reason: &str,
    password: Option<&str>,
) -> WisResult<Vec<CredentialBackup>> {
    authorize_vault(state, reason, password).await?;
    credentials_in_clear(state).await
}

/// Every saved login in the clear, for an export already authorized.
pub(crate) async fn credentials_in_clear(
    state: &State<'_, AppState>,
) -> WisResult<Vec<CredentialBackup>> {
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
    password: Option<&str>,
) -> WisResult<usize> {
    if creds.is_empty() {
        return Ok(0);
    }
    authorize_vault(state, reason, password).await?;
    seal_credentials(state, creds).await
}

/// Seal and save logins under the vault key, for an import already authorized.
pub(crate) async fn seal_credentials(
    state: &State<'_, AppState>,
    creds: Vec<CredentialBackup>,
) -> WisResult<usize> {
    if creds.is_empty() {
        return Ok(0);
    }

    let sealing = state.vault_key.lock().await;
    let key = sealing.as_ref().ok_or_else(locked)?;
    let sealed: Vec<(String, String, EncryptedBlob)> = creds
        .into_iter()
        .map(|c| {
            let blob = wis_secrets::encrypt(key, c.password.as_bytes()).map_err(vault_err)?;
            Ok((c.host, c.username, blob))
        })
        .collect::<WisResult<Vec<_>>>()?;

    let count = sealed.len();
    let storage = state.storage.clone();
    run_blocking(move || {
        for (host, username, blob) in &sealed {
            storage.save_credential(host, username, &blob.ciphertext, &blob.nonce)?;
        }
        Ok(())
    })
    .await?;
    drop(sealing);

    Ok(count)
}

/// Generate a password with the OS CSPRNG. Deliberately outside the unlock
/// gate as nothing is read or written, and the value goes only to the chrome
/// that asked for it. Lowercase letters are always drawn from.
///
/// The generator hands back a `Zeroizing` string, but that guarantee ends on
/// the next line: the returned `String`, its JSON encoding and the JavaScript
/// string it becomes are ordinary plaintext, exactly like `vault_reveal`.
#[tauri::command]
pub fn generate_password(length: u32, uppercase: bool, digits: bool, symbols: bool) -> String {
    let recipe = wis_secrets::PasswordRecipe {
        length: length as usize,
        uppercase,
        digits,
        symbols,
    };
    wis_secrets::generate_password(&recipe).to_string()
}

/// Generate a password the field's own `passwordrules` and `maxlength` allow,
/// so the site does not reject it on submit. Rules that cannot be read leave
/// the default recipe in place.
#[tauri::command]
pub fn generate_password_for_field(rules: Option<String>, max_length: Option<u32>) -> String {
    let fitted =
        wis_secrets::recipe_for_rules(rules.as_deref(), max_length.map(|max| max as usize));
    wis_secrets::generate_password_from(&fitted.recipe, &fitted.symbols).to_string()
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
    let sealing = state.vault_key.lock().await;
    let key = sealing.as_ref().ok_or_else(locked)?;
    let blob = wis_secrets::encrypt(key, password.as_bytes()).map_err(vault_err)?;
    let storage = state.storage.clone();

    let saved = run_blocking(move || {
        storage
            .save_credential(&host, &username, &blob.ciphertext, &blob.nonce)
            .map(|_| ())
    })
    .await;
    drop(sealing);
    saved
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

/// One saved password found in a public breach data set.
#[derive(Debug, Serialize)]
pub struct BreachFinding {
    pub id: i64,
    /// Times the password appears across known breaches.
    pub count: u32,
}

/// What a breach check found.
#[derive(Debug, Serialize)]
pub struct BreachReport {
    /// Passwords checked.
    pub checked: u32,
    /// Collection of breached passwords.
    pub breached: Vec<BreachFinding>,
}

/// Maximum vault size before it is checked in parts.
const MAX_BREACH_CHECKS: usize = 200;
/// Per-request budget for one prefix lookup.
const BREACH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
/// Time budget for the whole run.
const BREACH_TOTAL_BUDGET: std::time::Duration = std::time::Duration::from_secs(90);

/// Check saved passwords against the Pwned Passwords data set, disclosing
/// none of them.
///
/// Each password is hashed locally and only the first five characters of that
/// hash are sent; the service answers with every hash sharing the prefix and
/// the match is decided here (see [`wis_secrets::breach`]). The requests go
/// through the workspace's loopback proxy, so they inherit its encrypted DNS
/// and its upstream, and a Tor workspace asks over Tor.
///
/// Nothing here runs on a timer or at launch. It asks for biometrics even when
/// the vault is already unlocked to confirm the user's identity and consent.
#[tauri::command]
pub async fn vault_check_breaches(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    vault_password: Option<String>,
) -> WisResult<BreachReport> {
    let vault_password = vault_password.map(Zeroizing::new);
    authorize_vault(
        &state,
        "Check your saved passwords against known breaches",
        vault_password.as_deref().map(String::as_str),
    )
    .await?;

    let storage = state.storage.clone();
    let records = run_blocking(move || storage.list_credentials(None)).await?;

    // Hash under the lock, then drop. Nothing but a five-character prefix leaves.
    let lookups = {
        let guard = state.vault_key.lock().await;
        let key = guard.as_ref().ok_or_else(locked)?;
        let mut lookups: Vec<(i64, String, Zeroizing<String>)> = Vec::new();
        for record in records.into_iter().take(MAX_BREACH_CHECKS) {
            let blob = EncryptedBlob {
                ciphertext: record.ciphertext,
                nonce: record.nonce,
            };
            let Ok(plaintext) = wis_secrets::decrypt(key, &blob) else {
                continue;
            };
            let Ok(password) = String::from_utf8(plaintext) else {
                continue;
            };
            let (prefix, suffix) = wis_secrets::range_split(&password);

            lookups.push((record.id, prefix, Zeroizing::new(suffix)));
        }

        lookups
    };

    let proxy_port =
        state
            .ensure_proxy_for(workspace_id)
            .await
            .map_err(|err| WisError::InvalidInput {
                message: format!("proxy unavailable: {err:?}"),
            })?;
    let client = reqwest::Client::builder()
        .timeout(BREACH_TIMEOUT)
        .proxy(
            reqwest::Proxy::all(format!("http://127.0.0.1:{proxy_port}")).map_err(|err| {
                WisError::InvalidInput {
                    message: format!("proxy: {err}"),
                }
            })?,
        )
        .build()
        .map_err(|err| WisError::InvalidInput {
            message: format!("http client: {err}"),
        })?;

    let checked = u32::try_from(lookups.len()).unwrap_or(u32::MAX);

    let lookup_all = async {
        let mut ranges: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut breached = Vec::new();
        for (id, prefix, suffix) in lookups {
            let body = match ranges.get(&prefix) {
                Some(body) => body,
                None => {
                    let fetched = fetch_range(&client, &prefix).await?;
                    ranges.entry(prefix).or_insert(fetched)
                }
            };
            let count = wis_secrets::count_in_range(body, &suffix);
            if count > 0 {
                breached.push(BreachFinding { id, count });
            }
        }

        WisResult::Ok(breached)
    };

    let breached = tokio::time::timeout(BREACH_TOTAL_BUDGET, lookup_all)
        .await
        .map_err(|_| WisError::InvalidInput {
            message: "the breach check ran out of time before finishing. Try again.".to_string(),
        })??;

    Ok(BreachReport { checked, breached })
}

/// Ask for one hash prefix. `Add-Padding` makes every answer a similar size.
async fn fetch_range(client: &reqwest::Client, prefix: &str) -> WisResult<String> {
    let response = client
        .get(format!("https://api.pwnedpasswords.com/range/{prefix}"))
        .header("Add-Padding", "true")
        .header(reqwest::header::USER_AGENT, "Wis-Browser")
        .send()
        .await
        .map_err(|err| WisError::InvalidInput {
            message: format!("breach check could not reach the service: {err}"),
        })?;

    if !response.status().is_success() {
        return Err(WisError::InvalidInput {
            message: format!("breach check answered {}", response.status()),
        });
    }

    response.text().await.map_err(|err| WisError::InvalidInput {
        message: format!("breach check answer could not be read: {err}"),
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

    let host_js = serde_json::to_string(strip_www(&record.host)).map_err(json_err)?;
    let user_js = serde_json::to_string(&username).map_err(json_err)?;
    let pass_js = serde_json::to_string(&password).map_err(json_err)?;
    let script = fill_script(&host_js, &user_js, &pass_js);

    webview.eval(&script).map_err(|err| WisError::InvalidInput {
        message: format!("autofill eval failed: {err}"),
    })
}

fn strip_www(host: &str) -> &str {
    host.strip_prefix("www.").unwrap_or(host)
}

/// JS that fills a password field and the nearest text/email field before it,
/// dispatching input/change so frameworks notice. It starts from the focused
/// field when that is for a sign-in, so a username-first page gets its
/// username and a page with two forms fills the one in use; otherwise the
/// page's first password field. `host`/`user`/`pass` are already JSON string
/// literals; the page fills nothing unless it is still on `host`.
fn fill_script(host: &str, user: &str, pass: &str) -> String {
    format!(
        r#"(function(){{
  try {{
    var here = location.hostname;
    if (here !== {host} && here !== 'www.' + {host}) return;
    var PW = 'input[type=password]:not([disabled]):not([readonly])';
    function kind(el) {{
      if (!el || el.tagName !== 'INPUT' || el.disabled || el.readOnly) return '';
      return (el.type || 'text').toLowerCase();
    }}
    function textual(el) {{
      var k = kind(el);
      return k === 'text' || k === 'email' || k === 'tel';
    }}
    var active = document.activeElement;
    var pw = kind(active) === 'password' ? active : null;
    var user = null;
    if (textual(active)) {{
      var near = (active.form || document).querySelector(PW);
      var named = /username/i.test(active.getAttribute('autocomplete') || '');
      if (near ? active.compareDocumentPosition(near) & 4 : named) {{
        user = active;
        pw = near;
      }}
    }}
    if (!pw && !user) pw = document.querySelector(PW);
    if (!pw && !user) return;
    if (pw && !user) {{
      var inputs = Array.prototype.slice.call(document.querySelectorAll('input'));
      for (var i = inputs.indexOf(pw) - 1; i >= 0; i--) {{
        if (textual(inputs[i])) {{ user = inputs[i]; break; }}
      }}
    }}
    function setVal(el, val) {{
      var d = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(el), 'value');
      if (d && d.set) {{ d.set.call(el, val); }} else {{ el.value = val; }}
      el.dispatchEvent(new Event('input', {{ bubbles: true }}));
      el.dispatchEvent(new Event('change', {{ bubbles: true }}));
    }}
    if (user) setVal(user, {user});
    if (pw) setVal(pw, {pass});
  }} catch (e) {{}}
}})();"#,
        host = host,
        user = user,
        pass = pass,
    )
}

fn json_err(err: serde_json::Error) -> WisError {
    WisError::InvalidInput {
        message: format!("autofill: {err}"),
    }
}

// -- helpers ----------------------------------------------------------------

async fn require_unlocked(state: &State<'_, AppState>) -> WisResult<()> {
    if state.vault_key.lock().await.is_some() {
        Ok(())
    } else {
        Err(locked())
    }
}

pub(crate) fn locked() -> WisError {
    WisError::InvalidInput {
        message: "vault is locked".to_string(),
    }
}

pub(crate) fn unsupported() -> WisError {
    WisError::InvalidInput {
        message: "the password vault is not available on this platform".to_string(),
    }
}

pub(crate) fn vault_err(err: wis_secrets::VaultError) -> WisError {
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
