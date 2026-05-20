mod commands;
mod state;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::new_with_defaults())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
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
