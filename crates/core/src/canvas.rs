//! Canvases: named spaces of cards, one set per workspace. The document is
//! the canvas page's own JSON; Rust only bounds it.

use serde::Serialize;

use crate::error::{WisError, WisResult};
use crate::{CanvasId, WorkspaceId};

pub const DEFAULT_CANVAS_NAME: &str = "New canvas";
pub const MAX_NAME_CHARS: usize = 120;
pub const MAX_DOC_BYTES: usize = 8 * 1024 * 1024;
/// What a new canvas stores; the page fills in its defaults.
pub const EMPTY_DOC: &str = "{}";

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
