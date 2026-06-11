//! Compact icon references for folders and workspaces.
//!
//! Encoded as a string so both `TabFolder.icon` and `Workspace.icon` stay
//! plain `Option<String>` columns/JSON: `glyph:<name>` (wireframe),
//! `glyph-fill:<name>` (filled), or `emoji:<char>`. [`parse_icon_ref`] is the
//! boundary validator; glyph names are charset-checked here and resolved
//! against the bundled registry in the chrome (an unknown name renders the
//! fallback dot rather than failing).

use crate::error::{WisError, WisResult};

/// Parsed form of an icon reference string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IconRef {
    /// Wireframe glyph from the bundled registry.
    Glyph(String),
    /// Filled glyph from the bundled registry.
    GlyphFill(String),
    /// A literal emoji (one grapheme, byte-capped).
    Emoji(String),
}

const MAX_GLYPH_NAME_BYTES: usize = 32;
const MAX_EMOJI_BYTES: usize = 16;

/// Validate + parse an encoded icon reference.
pub fn parse_icon_ref(s: &str) -> WisResult<IconRef> {
    if let Some(name) = s.strip_prefix("glyph-fill:") {
        return glyph_name(name).map(IconRef::GlyphFill);
    }
    if let Some(name) = s.strip_prefix("glyph:") {
        return glyph_name(name).map(IconRef::Glyph);
    }
    if let Some(ch) = s.strip_prefix("emoji:") {
        let valid = !ch.is_empty()
            && ch.len() <= MAX_EMOJI_BYTES
            && !ch.chars().any(|c| c.is_control() || c.is_ascii());
        if !valid {
            return Err(invalid("emoji icon must be a single emoji"));
        }

        return Ok(IconRef::Emoji(ch.to_string()));
    }

    Err(invalid(
        "icon must be glyph:<name>, glyph-fill:<name>, or emoji:<char>",
    ))
}

fn glyph_name(name: &str) -> WisResult<String> {
    let valid = !name.is_empty()
        && name.len() <= MAX_GLYPH_NAME_BYTES
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !valid {
        return Err(invalid("glyph names are lowercase kebab-case"));
    }

    Ok(name.to_string())
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
    fn parses_all_three_kinds() {
        assert_eq!(
            parse_icon_ref("glyph:folder").unwrap(),
            IconRef::Glyph("folder".into())
        );
        assert_eq!(
            parse_icon_ref("glyph-fill:star").unwrap(),
            IconRef::GlyphFill("star".into())
        );
        assert_eq!(
            parse_icon_ref("emoji:\u{1f680}").unwrap(),
            IconRef::Emoji("\u{1f680}".into())
        );
    }

    #[test]
    fn rejects_malformed_references() {
        for bad in [
            "",
            "folder",
            "glyph:",
            "glyph:Folder",
            "glyph:has space",
            "glyph-fill:",
            "emoji:",
            "emoji:abc",
            "emoji:\u{7}",
        ] {
            assert!(parse_icon_ref(bad).is_err(), "{bad:?} must be rejected");
        }
    }

    #[test]
    fn caps_lengths() {
        assert!(parse_icon_ref(&format!("glyph:{}", "a".repeat(33))).is_err());
        assert!(parse_icon_ref(&format!("emoji:{}", "\u{1f600}".repeat(5))).is_err());
    }
}
