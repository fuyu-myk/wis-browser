//! Custom-theme commands: activation plus `.wistheme` import/export.
//!
//! Theme files are versioned JSON ([`wis_core::theme::ThemeFile`]) moved via
//! native save/open panels (user gesture only). Import validates the whole
//! structure at the boundary and re-mints the id, so a shared file can never
//! collide with (or overwrite) an existing theme.

use tauri::{AppHandle, State};
use wis_core::{CustomTheme, ThemeFile, ThemeId, WisError, WisResult, THEME_FILE_VERSION};

use crate::panels::{run_open_panel, run_save_panel};
use crate::state::AppState;

/// Imported files are tiny JSON; anything larger is malformed or hostile.
const MAX_THEME_FILE_BYTES: u64 = 256 * 1024;

/// Activate a custom theme (`None` = built-in presets).
#[tauri::command]
pub async fn set_active_theme(
    app: AppHandle,
    state: State<'_, AppState>,
    id: Option<ThemeId>,
) -> WisResult<()> {
    crate::commands::mutate_unit(&app, &state, |s| s.with_active_theme(id)).await
}

/// Save a theme as a `.wistheme` file. Resolves `false` on cancel.
#[tauri::command]
pub async fn export_theme(app: AppHandle, theme: CustomTheme) -> WisResult<bool> {
    theme.validate()?;
    let suggested = format!(
        "{}.wistheme",
        wis_core::sanitize_filename(theme.name.trim())
    );
    let Some(path) = run_save_panel(&app, &suggested)? else {
        return Ok(false);
    };
    let file = ThemeFile {
        version: THEME_FILE_VERSION,
        theme,
    };
    let json = serde_json::to_string_pretty(&file).map_err(|err| WisError::InvalidInput {
        message: format!("serialize theme: {err}"),
    })?;
    std::fs::write(&path, json).map_err(|err| WisError::InvalidInput {
        message: format!("write {}: {err}", path.display()),
    })?;

    Ok(true)
}

/// Open a `.wistheme` file, validate it, and return the theme with a fresh
/// id. `None` on cancel; the frontend appends it to `custom_themes`.
#[tauri::command]
pub async fn import_theme(app: AppHandle) -> WisResult<Option<CustomTheme>> {
    let Some(path) = run_open_panel(&app, &["wistheme", "json"])? else {
        return Ok(None);
    };

    let size = std::fs::metadata(&path)
        .map(|m| m.len())
        .unwrap_or(u64::MAX);
    if size > MAX_THEME_FILE_BYTES {
        return Err(WisError::InvalidInput {
            message: "theme file is too large".to_string(),
        });
    }

    let json = std::fs::read_to_string(&path).map_err(|err| WisError::InvalidInput {
        message: format!("read {}: {err}", path.display()),
    })?;
    let file: ThemeFile = serde_json::from_str(&json).map_err(|err| WisError::InvalidInput {
        message: format!("not a valid .wistheme file: {err}"),
    })?;
    if file.version > THEME_FILE_VERSION {
        return Err(WisError::InvalidInput {
            message: "theme file was made by a newer Wis".to_string(),
        });
    }

    let theme = CustomTheme {
        id: ThemeId::new(),
        ..file.theme
    };
    theme.validate()?;

    Ok(Some(theme))
}
