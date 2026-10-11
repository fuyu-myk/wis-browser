//! Finds other browsers' profiles on this machine and reads their bookmarks.
//! Profile files are untrusted input: reads are size-capped and parsing is
//! lenient.

mod chromium;
mod fs;

use std::path::PathBuf;

use wis_core::bookmark_tree::BookmarkNode;

/// Base directories the per-OS profile locations hang off.
#[derive(Debug, Clone)]
pub struct Roots {
    pub home: PathBuf,
    pub config: PathBuf,
    pub local_data: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Browser {
    Chrome,
    Chromium,
    Edge,
    Brave,
    Vivaldi,
    Opera,
    Arc,
}

impl Browser {
    pub fn name(self) -> &'static str {
        match self {
            Self::Chrome => "Chrome",
            Self::Chromium => "Chromium",
            Self::Edge => "Edge",
            Self::Brave => "Brave",
            Self::Vivaldi => "Vivaldi",
            Self::Opera => "Opera",
            Self::Arc => "Arc",
        }
    }
}

/// A browser profile found on this machine. Only `id` crosses IPC; the
/// directory never does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub id: String,
    pub browser: Browser,
    /// Packaging that installs a second copy alongside the native one.
    pub variant: Option<&'static str>,
    pub profile: String,
    dir: PathBuf,
}

impl Source {
    pub fn browser_label(&self) -> String {
        match self.variant {
            Some(variant) => format!("{} {variant}", self.browser.name()),
            None => self.browser.name().to_string(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("{file} is too large to import")]
    TooLarge { file: String },
    #[error("{file} could not be read: {reason}")]
    Unreadable { file: String, reason: String },
}

pub fn discover(roots: &Roots) -> Vec<Source> {
    chromium::discover(roots)
}

/// Resolves only against a fresh discovery, so a caller cannot name a path.
pub fn find(roots: &Roots, id: &str) -> Option<Source> {
    discover(roots).into_iter().find(|source| source.id == id)
}

pub fn read_bookmarks(source: &Source) -> Result<Vec<BookmarkNode>, ImportError> {
    chromium::read_bookmarks(&source.dir)
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::{Path, PathBuf};

    use super::Roots;

    pub struct TempDir(PathBuf);

    impl TempDir {
        pub fn new() -> Self {
            let path = std::env::temp_dir().join(format!("wis-import-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        pub fn path(&self) -> &Path {
            &self.0
        }

        pub fn roots(&self) -> Roots {
            Roots {
                home: self.0.join("home"),
                config: self.0.join("config"),
                local_data: self.0.join("local"),
            }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    pub fn write(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
}
