//! Custom chrome themes: a named pair of light/dark token sets.
//!
//! A theme overrides the curated set of CSS tokens that define the chrome's
//! look (accent + surfaces + text + border) per mode; everything else keeps
//! the built-in values so a theme can never produce an unreadable half-set.
//! Stored in `UserSettings` (JSON blob, no migration) and shared as
//! `.wistheme` files (versioned JSON). All hex fields validate at the IPC
//! boundary via [`CustomTheme::validate`].

use serde::{Deserialize, Serialize};

use crate::background::is_valid_hex_color;
use crate::error::{WisError, WisResult};
use crate::ids::ThemeId;

pub const MAX_THEME_NAME_BYTES: usize = 64;

/// Token values for one mode. All `#rrggbb`/`#rrggbbaa` hex.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModeTokens {
    /// Accent base; the chrome derives the full primary scale from it.
    pub accent: String,
    pub bg: String,
    pub surface: String,
    pub surface_raised: String,
    pub chrome_bg: String,
    pub content_bg: String,
    pub text_primary: String,
    pub text_secondary: String,
    pub border: String,
}

impl ModeTokens {
    fn validate(&self) -> WisResult<()> {
        let fields = [
            &self.accent,
            &self.bg,
            &self.surface,
            &self.surface_raised,
            &self.chrome_bg,
            &self.content_bg,
            &self.text_primary,
            &self.text_secondary,
            &self.border,
        ];
        if fields.iter().any(|c| !is_valid_hex_color(c)) {
            return Err(WisError::InvalidInput {
                message: "theme colours must be #rrggbb or #rrggbbaa hex".to_string(),
            });
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeTokens {
    pub light: ModeTokens,
    pub dark: ModeTokens,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomTheme {
    pub id: ThemeId,
    pub name: String,
    pub tokens: ThemeTokens,
}

impl CustomTheme {
    pub fn validate(&self) -> WisResult<()> {
        let name = self.name.trim();
        if name.is_empty() || name.len() > MAX_THEME_NAME_BYTES {
            return Err(WisError::InvalidInput {
                message: "theme name must be 1..=64 bytes".to_string(),
            });
        }

        self.tokens.light.validate()?;
        self.tokens.dark.validate()
    }
}

/// On-disk `.wistheme` envelope, versioned for forward compatibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeFile {
    pub version: u8,
    pub theme: CustomTheme,
}

pub const THEME_FILE_VERSION: u8 = 1;

#[cfg(test)]
mod tests {
    use super::*;

    fn mode() -> ModeTokens {
        ModeTokens {
            accent: "#6b77f8".into(),
            bg: "#f7f6f3".into(),
            surface: "#ffffff".into(),
            surface_raised: "#fafaf9".into(),
            chrome_bg: "#eceae3".into(),
            content_bg: "#ffffff".into(),
            text_primary: "#1a1916".into(),
            text_secondary: "#6b6a66".into(),
            border: "#e2dfd5".into(),
        }
    }

    fn theme() -> CustomTheme {
        CustomTheme {
            id: ThemeId::new(),
            name: "Test".into(),
            tokens: ThemeTokens {
                light: mode(),
                dark: mode(),
            },
        }
    }

    #[test]
    fn valid_theme_passes_and_round_trips() {
        let t = theme();
        t.validate().unwrap();
        let json = serde_json::to_string(&ThemeFile {
            version: THEME_FILE_VERSION,
            theme: t.clone(),
        })
        .unwrap();
        let back: ThemeFile = serde_json::from_str(&json).unwrap();
        assert_eq!(back.theme, t);
    }

    #[test]
    fn rejects_bad_name_and_colours() {
        let mut t = theme();
        t.name = "  ".into();
        assert!(t.validate().is_err());

        let mut t = theme();
        t.name = "x".repeat(65);
        assert!(t.validate().is_err());

        let mut t = theme();
        t.tokens.dark.border = "red".into();
        assert!(t.validate().is_err());
    }
}
