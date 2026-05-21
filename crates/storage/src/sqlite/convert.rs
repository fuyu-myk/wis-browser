//! Conversion helpers between Rust types and the SQL representation.
//!
//! Enums in `wis-core` are unit-variant enums that serde renders as
//! snake_case strings. That serialisation is reused as the canonical SQL form
//! so the column values are human-readable and any future serde rename gets
//! picked up automatically.

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

use crate::error::{StorageError, StorageResult};

/// Render a unit-variant enum as the bare snake_case string serde produces.
pub fn enum_to_text<T: Serialize>(value: &T) -> StorageResult<String> {
    match serde_json::to_value(value)? {
        Value::String(s) => Ok(s),
        other => Err(StorageError::DataCorruption(format!(
            "expected unit-variant enum to serialise as a string, got {other:?}"
        ))),
    }
}

/// Parse a snake_case enum string back into its Rust variant.
pub fn enum_from_text<T: DeserializeOwned>(s: &str) -> StorageResult<T> {
    serde_json::from_value(Value::String(s.to_string())).map_err(StorageError::from)
}
