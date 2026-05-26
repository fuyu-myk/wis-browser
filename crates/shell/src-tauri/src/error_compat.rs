//! Internal shell-side error type.

use thiserror::Error;
use wis_core::WisError;

#[derive(Debug, Error)]
pub enum WisShellError {
    #[error(transparent)]
    Wis(#[from] WisError),

    #[error("{0}")]
    Other(String),
}

impl From<WisShellError> for WisError {
    fn from(err: WisShellError) -> Self {
        match err {
            WisShellError::Wis(inner) => inner,
            WisShellError::Other(message) => WisError::InvalidInput { message },
        }
    }
}
