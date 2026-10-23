//! Custom-theme commands: activation, `.wistheme` import/export, and portable
//! share codes (base64 of the same versioned [`wis_core::appearance::theme::ThemeFile`]).
//!
//! Theme files are versioned JSON ([`wis_core::appearance::theme::ThemeFile`]) moved via
//! native save/open panels (user gesture only). Import validates the whole
//! structure at the boundary and re-mints the id, so a shared file can never
//! collide with (or overwrite) an existing theme. Share codes follow the same
//! validate-and-re-mint path over the clipboard instead of the filesystem.

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use tauri::{AppHandle, State};
use wis_core::{CustomTheme, ThemeFile, ThemeId, WisError, WisResult, THEME_FILE_VERSION};

use crate::state::AppState;
use crate::system::panels::{run_open_panel, run_save_panel};

/// Imported files are tiny JSON; anything larger is malformed or hostile.
const MAX_THEME_FILE_BYTES: u64 = 256 * 1024;

/// Prefix on a share code, so a pasted string is recognisably a Wis theme.
const THEME_CODE_PREFIX: &str = "wis-theme:";

/// Theme JSON is ~1 KB; its base64 a little more. Anything past this is
/// malformed or hostile, so decoding rejects it before allocating.
const MAX_THEME_CODE_BYTES: usize = 16 * 1024;

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

/// Encode a theme as a portable share code (`wis-theme:<base64>`), so it can
/// be copied to the clipboard and pasted elsewhere. Same versioned envelope
/// as `.wistheme` files.
#[tauri::command]
pub async fn encode_theme_code(theme: CustomTheme) -> WisResult<String> {
    encode_code(theme)
}

/// Decode a share code into a theme with a fresh id (so it can never collide
/// with an existing one). Tolerates surrounding whitespace and an absent
/// prefix; validates the whole structure at the boundary.
#[tauri::command]
pub async fn decode_theme_code(code: String) -> WisResult<CustomTheme> {
    decode_code(&code)
}

// Pure core for the share-code commands (no I/O), kept testable.

fn encode_code(theme: CustomTheme) -> WisResult<String> {
    theme.validate()?;
    let file = ThemeFile {
        version: THEME_FILE_VERSION,
        theme,
    };
    let json = serde_json::to_string(&file).map_err(|err| WisError::InvalidInput {
        message: format!("serialize theme: {err}"),
    })?;

    Ok(format!("{THEME_CODE_PREFIX}{}", BASE64.encode(json)))
}

fn decode_code(code: &str) -> WisResult<CustomTheme> {
    let bad = || WisError::InvalidInput {
        message: "not a valid theme code".to_string(),
    };
    let trimmed = code.trim();
    let payload = trimmed.strip_prefix(THEME_CODE_PREFIX).unwrap_or(trimmed);
    if payload.is_empty() || payload.len() > MAX_THEME_CODE_BYTES {
        return Err(bad());
    }

    let json = BASE64.decode(payload).map_err(|_| bad())?;
    let file: ThemeFile = serde_json::from_slice(&json).map_err(|_| bad())?;
    if file.version > THEME_FILE_VERSION {
        return Err(WisError::InvalidInput {
            message: "theme code was made by a newer Wis".to_string(),
        });
    }

    let theme = CustomTheme {
        id: ThemeId::new(),
        ..file.theme
    };
    theme.validate()?;

    Ok(theme)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wis_core::{ModeTokens, ThemeTokens};

    fn sample() -> CustomTheme {
        let mode = ModeTokens {
            accent: "#6b77f8".into(),
            bg: "#f7f6f3".into(),
            surface: "#ffffff".into(),
            surface_raised: "#fafaf9".into(),
            chrome_bg: "#eceae3".into(),
            content_bg: "#ffffff".into(),
            text_primary: "#1a1916".into(),
            text_secondary: "#6b6a66".into(),
            border: "#e2dfd5".into(),
        };
        CustomTheme {
            id: ThemeId::new(),
            name: "Sample".into(),
            tokens: ThemeTokens {
                light: mode.clone(),
                dark: mode,
            },
        }
    }

    #[test]
    fn round_trips_and_re_mints_id() {
        let theme = sample();
        let code = encode_code(theme.clone()).unwrap();
        assert!(code.starts_with(THEME_CODE_PREFIX));

        let back = decode_code(&code).unwrap();
        assert_eq!(back.name, theme.name);
        assert_eq!(back.tokens, theme.tokens);
        // A shared theme can never collide with the original.
        assert_ne!(back.id, theme.id);
    }

    #[test]
    fn decode_tolerates_whitespace_and_missing_prefix() {
        let code = encode_code(sample()).unwrap();
        let bare = code.strip_prefix(THEME_CODE_PREFIX).unwrap();
        decode_code(&format!("  {code}  \n")).unwrap();
        decode_code(bare).unwrap();
    }

    #[test]
    fn rejects_garbage_and_newer_versions() {
        assert!(decode_code("").is_err());
        assert!(decode_code("wis-theme:not base64!!").is_err());
        assert!(decode_code("totally bogus").is_err());

        let newer = ThemeFile {
            version: THEME_FILE_VERSION + 1,
            theme: sample(),
        };
        let json = serde_json::to_string(&newer).unwrap();
        let code = format!("{THEME_CODE_PREFIX}{}", BASE64.encode(json));
        assert!(decode_code(&code).is_err());
    }
}
