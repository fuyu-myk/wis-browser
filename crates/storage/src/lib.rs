//! Persistent storage for the Wis browser.
//!
//! Defines the [`Storage`] trait and a SQLite implementation. The trait is
//! pulled into the shell's `AppState` so storage can be swapped out in tests
//! (and, later, replaced with an encrypted variant).

pub mod credentials;
pub mod error;
pub mod history;
pub mod searches;
pub mod sqlite;
pub mod traits;

pub use credentials::CredentialRecord;
pub use error::{StorageError, StorageResult};
pub use history::{HistoryEntry, HistoryQuery};
pub use searches::SearchEntry;
pub use sqlite::SqliteStorage;
pub use traits::Storage;
