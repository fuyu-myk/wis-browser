//! Persistent storage for the Wis browser.
//!
//! Defines the [`Storage`] trait and a SQLite implementation. The trait is
//! pulled into the shell's `AppState` so storage can be swapped out in tests
//! (and, later, replaced with an encrypted variant).

pub mod error;
pub mod sqlite;
pub mod traits;

pub use error::{StorageError, StorageResult};
pub use sqlite::SqliteStorage;
pub use traits::Storage;
