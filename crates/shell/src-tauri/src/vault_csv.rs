//! Importing and exporting saved passwords as CSV, each behind the vault's
//! check that it is the user.

use serde::Serialize;
use tauri::{AppHandle, State};
use wis_core::library::backup::CredentialBackup;
use wis_core::{WisError, WisResult};
use zeroize::Zeroizing;

use crate::state::AppState;
use crate::vault::{export_credentials, import_credentials, unsupported, vault_supported};

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
    vault_password: Option<String>,
) -> WisResult<Option<CsvImportStats>> {
    let vault_password = vault_password.map(Zeroizing::new);
    if !vault_supported() {
        return Err(unsupported());
    }
    let Some(path) = crate::panels::run_open_panel(&app, &["csv", "txt"])? else {
        return Ok(None);
    };

    // Bounded read + parse off the async runtime; the file is untrusted.
    let parsed = tokio::task::spawn_blocking(move || -> WisResult<wis_core::ParsedCredentials> {
        let source = crate::file_io::read_capped(
            &path,
            wis_core::vault::password_csv::MAX_PASSWORD_CSV_BYTES,
        )?;
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
    let imported = import_credentials(
        &state,
        creds,
        "Import passwords into your vault",
        vault_password.as_deref().map(String::as_str),
    )
    .await?;

    Ok(Some(CsvImportStats { imported, skipped }))
}

/// Write every saved login to a plaintext CSV at a user-picked path, after a
/// biometric confirm (always prompts, even when unlocked; the UI additionally
/// shows an explicit plaintext warning first). Resolves `false` when the user
/// cancels the save panel.
#[tauri::command]
pub async fn vault_export_csv(
    app: AppHandle,
    state: State<'_, AppState>,
    vault_password: Option<String>,
) -> WisResult<bool> {
    let vault_password = vault_password.map(Zeroizing::new);
    if !vault_supported() {
        return Err(unsupported());
    }
    let Some(path) = crate::panels::run_save_panel(&app, &suggested_csv_name())? else {
        return Ok(false);
    };

    let creds = export_credentials(
        &state,
        "Export your saved passwords to a file",
        vault_password.as_deref().map(String::as_str),
    )
    .await?;
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
