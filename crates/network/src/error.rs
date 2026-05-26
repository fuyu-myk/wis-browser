//! Errors produced by the network layer.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum NetworkError {
    #[error("resolver: {0}")]
    Resolver(String),

    #[error("filter list: {0}")]
    FilterList(String),

    #[error("invalid url: {0}")]
    InvalidUrl(#[from] url::ParseError),

    #[error("config: {0}")]
    Config(String),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("proxy: {0}")]
    Proxy(String),
}

pub type NetworkResult<T> = Result<T, NetworkError>;
