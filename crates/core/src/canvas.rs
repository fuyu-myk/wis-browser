//! Canvases: named spaces of cards, one set per workspace. The document is
//! the canvas page's own JSON; Rust only bounds it.

use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::error::{WisError, WisResult};
use crate::{CanvasId, WorkspaceId};

/// The scheme a canvas tab's page is served on.
pub const SCHEME: &str = "wis-canvas";

pub const DEFAULT_CANVAS_NAME: &str = "New canvas";
pub const MAX_NAME_CHARS: usize = 120;
pub const MAX_DOC_BYTES: usize = 8 * 1024 * 1024;
/// What a new canvas stores; the page fills in its defaults.
pub const EMPTY_DOC: &str = "{}";

/// How canvases look, apart from the browser's theme.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanvasTheme {
    #[default]
    Dark,
    Light,
}

impl CanvasTheme {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }
}

/// A canvas as the switcher lists it. Times are Unix milliseconds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CanvasSummary {
    pub id: CanvasId,
    pub name: String,
    pub updated_at: i64,
}

/// A stored canvas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanvasRecord {
    pub id: CanvasId,
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub doc: String,
    pub created_at: i64,
    pub updated_at: i64,
}

impl CanvasRecord {
    pub fn summary(&self) -> CanvasSummary {
        CanvasSummary {
            id: self.id,
            name: self.name.clone(),
            updated_at: self.updated_at,
        }
    }
}

/// A newer document for a stored canvas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanvasDocUpdate {
    pub workspace_id: WorkspaceId,
    pub id: CanvasId,
    pub doc: String,
    pub updated_at: i64,
}

/// A canvas name with whitespace collapsed, cut to [`MAX_NAME_CHARS`].
pub fn canvas_name(raw: &str) -> WisResult<String> {
    let name = raw
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_NAME_CHARS)
        .collect::<String>();
    if name.is_empty() {
        return Err(WisError::InvalidInput {
            message: "a canvas needs a name".into(),
        });
    }
    Ok(name)
}

/// The canvas a canvas tab's address names, if any.
pub fn canvas_in_url(url: &Url) -> Option<CanvasId> {
    if url.scheme() != SCHEME {
        return None;
    }
    url.query_pairs()
        .find(|(key, _)| key == "canvas")
        .and_then(|(_, value)| Uuid::parse_str(&value).ok())
        .map(CanvasId::from)
}

/// A canvas tab's address for `id`.
pub fn canvas_url(id: CanvasId) -> Url {
    Url::parse(&format!("{SCHEME}://localhost/?canvas={}", id.as_uuid()))
        .expect("a canvas address is a valid URL")
}

/// Accept a document only when it is a JSON object within [`MAX_DOC_BYTES`].
pub fn check_doc(doc: &str) -> WisResult<()> {
    if doc.len() > MAX_DOC_BYTES {
        return Err(WisError::InvalidInput {
            message: "the canvas is too large to save".into(),
        });
    }
    match serde_json::from_str::<serde_json::Value>(doc) {
        Ok(serde_json::Value::Object(_)) => Ok(()),
        _ => Err(WisError::InvalidInput {
            message: "the canvas could not be read".into(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_canvas_theme_defaults_to_dark_and_patches() {
        use crate::settings::{UserSettings, UserSettingsPatch};

        let settings = UserSettings::default();
        assert_eq!(settings.canvas_theme, CanvasTheme::Dark);

        let mut stored = serde_json::to_value(&settings).unwrap();
        stored.as_object_mut().unwrap().remove("canvas_theme");
        let older: UserSettings = serde_json::from_value(stored).unwrap();
        assert_eq!(
            older.canvas_theme,
            CanvasTheme::Dark,
            "settings saved before canvases"
        );

        let patched = settings.with_patch(UserSettingsPatch {
            canvas_theme: Some(CanvasTheme::Light),
            ..Default::default()
        });
        assert_eq!(patched.canvas_theme, CanvasTheme::Light);
        assert_eq!(
            serde_json::to_string(&CanvasTheme::Light).unwrap(),
            "\"light\""
        );
    }

    #[test]
    fn names_are_tidied_and_capped() {
        assert_eq!(canvas_name("  Trip \n plans ").unwrap(), "Trip plans");
        assert_eq!(
            canvas_name(&"é".repeat(200)).unwrap().chars().count(),
            MAX_NAME_CHARS
        );
        assert!(canvas_name(" \t ").is_err());
    }

    #[test]
    fn documents_must_be_bounded_json_objects() {
        assert!(check_doc(EMPTY_DOC).is_ok());
        assert!(check_doc(r#"{"version":1,"cards":[]}"#).is_ok());
        for bad in ["[]", "\"text\"", "{", ""] {
            assert!(check_doc(bad).is_err(), "{bad:?} must be refused");
        }
        let big = format!(r#"{{"pad":"{}"}}"#, "x".repeat(MAX_DOC_BYTES));
        assert!(check_doc(&big).is_err());
    }
}
