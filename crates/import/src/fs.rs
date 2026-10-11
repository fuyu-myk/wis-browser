use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::Path;

use crate::ImportError;

/// `None` when the file does not exist. Only regular files are read, so a
/// FIFO or device in a profile cannot block.
pub(crate) fn read_capped(path: &Path, cap: usize) -> Result<Option<Vec<u8>>, ImportError> {
    let file_name = || {
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let unreadable = |err: std::io::Error| ImportError::Unreadable {
        file: file_name(),
        reason: err.to_string(),
    };

    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => {}
        Ok(_) => {
            return Err(ImportError::Unreadable {
                file: file_name(),
                reason: "not a regular file".to_string(),
            })
        }
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(unreadable(err)),
    }

    let file = File::open(path).map_err(unreadable)?;
    let mut bytes = Vec::new();
    file.take(cap as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(unreadable)?;
    if bytes.len() > cap {
        return Err(ImportError::TooLarge { file: file_name() });
    }

    Ok(Some(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{write, TempDir};

    #[test]
    fn missing_files_read_as_none() {
        let dir = TempDir::new();
        assert!(read_capped(&dir.path().join("nope"), 10).unwrap().is_none());
    }

    #[test]
    fn files_over_the_cap_are_rejected() {
        let dir = TempDir::new();
        let path = dir.path().join("Bookmarks");
        write(&path, "12345678901");

        assert!(matches!(
            read_capped(&path, 10),
            Err(ImportError::TooLarge { file }) if file == "Bookmarks"
        ));
        assert_eq!(read_capped(&path, 11).unwrap().unwrap().len(), 11);
    }

    #[test]
    fn non_regular_files_are_refused() {
        let dir = TempDir::new();
        let path = dir.path().join("Bookmarks");
        std::fs::create_dir_all(&path).unwrap();

        assert!(matches!(
            read_capped(&path, 10),
            Err(ImportError::Unreadable { file, .. }) if file == "Bookmarks"
        ));
    }
}
