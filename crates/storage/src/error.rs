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

    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type StorageResult<T> = Result<T, StorageError>;
