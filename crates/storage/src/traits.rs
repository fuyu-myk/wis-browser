//! The [`Storage`] trait — the contract the rest of the app codes against.

use crate::error::StorageResult;
use wis_core::BrowserState;

/// A pluggable persistent store for the browser's state.
///
/// Implementations must be safe to share across threads (`Send + Sync`).
/// `load_state` returns `Ok(None)` when there is no persisted state yet
/// (first launch, or after a wipe), so the caller can fall back to
/// [`BrowserState::new_with_defaults`] without conflating "empty" with
/// "broken".
pub trait Storage: Send + Sync {
    /// Read the persisted state. Returns `Ok(None)` when nothing has been
    /// saved yet. The implementation is responsible for verifying invariants
    /// before returning the state.
    fn load_state(&self) -> StorageResult<Option<BrowserState>>;

    /// Persist the full state atomically. Replaces any prior persisted
    /// contents.
    fn save_state(&self, state: &BrowserState) -> StorageResult<()>;
}
