//! Application state container managed by Tauri.
//!
//! Holds the in-memory [`BrowserState`] alongside the persistent
//! [`Storage`] backend. Commands acquire the async mutex, compute the new
//! state, persist it, then replace the in-memory copy — see
//! `mutate` in `commands`.

use std::sync::Arc;

use tokio::sync::Mutex;
use wis_core::BrowserState;
use wis_storage::Storage;

pub struct AppState {
    pub browser: Mutex<BrowserState>,
    pub storage: Arc<dyn Storage>,
}

impl AppState {
    pub fn new(browser: BrowserState, storage: Arc<dyn Storage>) -> Self {
        Self {
            browser: Mutex::new(browser),
            storage,
        }
    }
}
