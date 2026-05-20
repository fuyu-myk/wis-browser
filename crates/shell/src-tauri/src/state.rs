//! Application state container managed by Tauri. Wraps [`BrowserState`] in
//! an `RwLock` so commands can read and update it concurrently.
//!
//! Storage and proxy handles will be added alongside the browser state in future.

use std::sync::RwLock;
use wis_core::BrowserState;

pub struct AppState {
    pub browser: RwLock<BrowserState>,
}

impl AppState {
    pub fn new_with_defaults() -> Self {
        Self {
            browser: RwLock::new(BrowserState::new_with_defaults()),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new_with_defaults()
    }
}
