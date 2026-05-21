//! Domain errors for the core data model. Serialisable so they can be
//! returned across the Tauri IPC boundary unchanged.

use crate::ids::{TabId, WorkspaceId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Error, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum WisError {
    #[error("workspace not found: {id}")]
    WorkspaceNotFound { id: WorkspaceId },

    #[error("tab not found: {id}")]
    TabNotFound { id: TabId },

    #[error("invalid URL: {url}")]
    InvalidUrl { url: String },

    #[error("cannot delete the last remaining workspace")]
    CannotDeleteLastWorkspace,

    #[error("tab {tab_id} does not belong to workspace {workspace_id}")]
    TabWorkspaceMismatch {
        tab_id: TabId,
        workspace_id: WorkspaceId,
    },

    #[error("invalid input: {message}")]
    InvalidInput { message: String },

    #[error("persistence failed: {reason}")]
    PersistenceFailed { reason: String },
}

pub type WisResult<T> = Result<T, WisError>;
