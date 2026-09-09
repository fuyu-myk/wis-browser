//! Persistent storage for the Wis browser.
//!
//! Defines the [`Storage`] trait and a SQLite implementation. The trait is
//! pulled into the shell's `AppState` so storage can be swapped out in tests
//! (and, later, replaced with an encrypted variant).

pub mod bookmarks;
pub mod closed_tabs;
pub mod credentials;
pub mod downloads;
pub mod error;
pub mod history;
pub mod searches;
pub mod sqlite;
pub mod traits;

pub use bookmarks::{Bookmark, BookmarkFolder, BookmarkImport, BookmarkImportStats};
pub use closed_tabs::ClosedTabRecord;
pub use credentials::CredentialRecord;
pub use downloads::DownloadRecord;
pub use error::{StorageError, StorageResult};
pub use history::{HistoryEntry, HistoryQuery};
pub use searches::SearchEntry;
pub use sqlite::{is_intact, SqliteStorage};
pub use traits::Storage;
