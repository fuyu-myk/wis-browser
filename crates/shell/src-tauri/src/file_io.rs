//! Bounded reads for user-picked import files.
//!
//! The read itself is capped rather than pre-checked via `fs::metadata`,
//! which bounds neither FIFOs handed to the file picker nor a file grown
//! after the check. Reading one byte past the cap lets the caller's parser
//! reject the file by its own length check instead of silently truncating.

use std::path::Path;

use wis_core::{WisError, WisResult};

/// Read at most `cap + 1` bytes of UTF-8 from `path`.
pub fn read_capped(path: &Path, cap: usize) -> WisResult<String> {
    use std::io::Read;

    let mut source = String::new();
    std::fs::File::open(path)
        .map_err(|err| WisError::InvalidInput {
            message: format!("read {}: {err}", path.display()),
        })?
        .take(cap as u64 + 1)
        .read_to_string(&mut source)
        .map_err(|err| WisError::InvalidInput {
            message: format!("read {}: {err}", path.display()),
        })?;

    Ok(source)
}
