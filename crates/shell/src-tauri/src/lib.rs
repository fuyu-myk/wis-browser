mod commands;
mod state;

use std::sync::Arc;

use tauri::Manager;
use wis_core::BrowserState;
use wis_storage::{SqliteStorage, Storage};

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(if cfg!(debug_assertions) {
                    log::LevelFilter::Info
                } else {
                    log::LevelFilter::Warn
                })
                .build(),
        )
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("could not resolve app data directory");
            std::fs::create_dir_all(&data_dir)?;
            let db_path = data_dir.join("wis.db");

            log::info!("opening database at {}", db_path.display());
            let storage = SqliteStorage::open(&db_path)?;

            let browser_state = hydrate_state(&storage);
            let storage: Arc<dyn Storage> = Arc::new(storage);

            app.manage(AppState::new(browser_state, storage));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_browser_state,
            commands::get_workspace,
            commands::get_tab,
            commands::get_settings,
            commands::create_workspace,
            commands::rename_workspace,
            commands::delete_workspace,
            commands::set_active_workspace,
            commands::create_tab,
            commands::close_tab,
            commands::reorder_tabs,
            commands::pin_tab,
            commands::set_active_tab,
            commands::update_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Load persisted state, falling back to defaults on corruption or absence.
/// Logs the chosen path so a user can tell from the dev console whether their
/// data survived a restart.
fn hydrate_state(storage: &SqliteStorage) -> BrowserState {
    match storage.load_state() {
        Ok(Some(state)) => {
            log::info!(
                "hydrated state from disk: {} workspaces, {} tabs",
                state.workspaces.len(),
                state.tabs.len(),
            );
            state
        }
        Ok(None) => {
            log::info!("no persisted state found, starting with defaults");
            BrowserState::new_with_defaults()
        }
        Err(err) => {
            log::warn!("failed to load persisted state ({err}); starting with defaults");
            BrowserState::new_with_defaults()
        }
    }
}
