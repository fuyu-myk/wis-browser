//! Errors that storage operations can produce.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("migration: {0}")]
    Migration(#[from] rusqlite_migration::Error),

    #[error("serialisation: {0}")]
    Serialisation(#[from] serde_json::Error),

    #[error("url parse: {0}")]
    UrlParse(#[from] url::ParseError),

    #[error("uuid parse: {0}")]
    UuidParse(#[from] uuid::Error),

    #[error("data corruption: {0}")]
    DataCorruption(String),

    /// The profile on disk was written by a newer build than this one
    #[error("profile schema v{found} is newer than this build supports (v{supported})")]
    ProfileTooNew { found: usize, supported: usize },

    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

impl StorageError {
    /// True when the failure is a busy or locked database rather than damage.
    ///
    /// Another process may hold the file for a moment during launch; callers
    /// retry these instead of treating them as a broken profile.
    pub fn is_transient(&self) -> bool {
        fn busy(err: &rusqlite::Error) -> bool {
            matches!(
                err,
                rusqlite::Error::SqliteFailure(e, _)
                    if e.code == rusqlite::ErrorCode::DatabaseBusy
                        || e.code == rusqlite::ErrorCode::DatabaseLocked
            )
        }

        match self {
            Self::Sqlite(err) => busy(err),
            Self::Migration(rusqlite_migration::Error::RusqliteError { err, .. }) => busy(err),
            _ => false,
        }
    }
}

pub type StorageResult<T> = Result<T, StorageError>;
