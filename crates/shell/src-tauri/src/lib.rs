mod biometrics;
mod capture;
mod commands;
mod content_filter;
mod cookies;
mod error_compat;
mod favicons;
mod find;
mod menu;
mod overlay;
mod pane_focus;
mod state;
mod suggest;
mod vault;

use std::sync::Arc;

use tauri::Manager;
use wis_core::BrowserState;
use wis_network::filter_lists;
use wis_network::TrackerBlocker;
use wis_storage::{SqliteStorage, Storage};

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,wis_network=debug"));
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_writer(std::io::stderr)
        .finish();
    let _ = tracing::subscriber::set_global_default(subscriber);

    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(if cfg!(debug_assertions) {
                    log::LevelFilter::Debug
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

            // Pick the initial filter list: cached EasyPrivacy if fresh,
            // otherwise the bundled fallback.
            let filter_cache_dir = data_dir.join("filter_lists");
            let (filter_text, _source) = filter_lists::load_cached_or_default(&filter_cache_dir);
            let tracker_blocker = match TrackerBlocker::from_filter_texts(&[&filter_text]) {
                Ok(b) => Arc::new(b),
                Err(err) => {
                    log::error!(
                        "failed to build tracker blocker ({err}); falling back to empty list"
                    );
                    Arc::new(TrackerBlocker::empty())
                }
            };
            let resolver = state::build_resolver(browser_state.settings.doh_resolver.as_deref());
            let auto_update = browser_state.settings.filter_list_auto_update;
            let initial_keybindings = browser_state.settings.keybindings.clone();

            app.manage(AppState::new(
                browser_state,
                storage,
                tracker_blocker,
                resolver,
                filter_cache_dir.clone(),
                &filter_text,
            ));

            // Background EasyPrivacy refresh. Runs once per launch when the
            // setting is on and the cache is stale; failures only log so
            // offline launches still proceed with the bundled fallback.
            if auto_update && filter_lists::fetch::cache_is_stale(&filter_cache_dir) {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    match filter_lists::fetch::refresh_easyprivacy(&filter_cache_dir).await {
                        Ok(text) => {
                            let state = handle.state::<AppState>();
                            if let Err(err) = state.rebuild_filter_engine(text).await {
                                log::warn!("filter_lists: rebuild after refresh failed: {err}");
                            }
                        }
                        Err(err) => {
                            log::warn!("filter_lists: refresh failed: {err}");
                        }
                    }
                });
            }

            #[cfg(target_os = "macos")]
            apply_transparent_chrome_webview(app);

            // Native menu + accelerator wiring.
            let effective_keymap = wis_core::effective_keymap(&initial_keybindings);
            if let Err(err) = menu::install_menu(app, &effective_keymap) {
                log::error!("menu install failed: {err}");
            }

            // Native click-to-focus for split panes.
            pane_focus::install(app.handle());

            // App handle for the native "Save password?" capture handler.
            capture::set_app_handle(app.handle().clone());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_browser_state,
            commands::get_workspace,
            commands::get_tab,
            commands::get_settings,
            commands::get_history,
            commands::delete_history,
            commands::clear_history,
            cookies::clear_site_data,
            cookies::list_site_data,
            cookies::delete_site_data,
            vault::vault_status,
            vault::vault_unlock,
            vault::vault_lock,
            vault::vault_list,
            vault::vault_save,
            vault::vault_reveal,
            vault::vault_delete,
            vault::vault_has_credentials,
            vault::autofill_candidates,
            vault::autofill_fill,
            suggest::get_url_suggestions,
            suggest::record_search,
            suggest::delete_search,
            capture::capture_save,
            capture::capture_dismiss,
            capture::capture_pending,
            commands::create_workspace,
            commands::create_private_workspace,
            commands::rename_workspace,
            commands::delete_workspace,
            commands::recolor_workspace,
            commands::set_active_workspace,
            commands::set_workspace_privacy_mode,
            commands::set_workspace_upstream,
            commands::test_upstream,
            commands::tor_available,
            commands::create_tab,
            commands::split_pane,
            commands::split_pane_with,
            commands::replace_pane_leaf,
            commands::set_pane_ratio,
            commands::unsplit_panes,
            commands::close_tab,
            commands::restore_closed_tab,
            commands::tab_hard_reload,
            commands::tab_zoom,
            commands::reorder_tabs,
            commands::pin_tab,
            commands::set_active_tab,
            commands::navigate_tab,
            commands::set_webview_corner_radius,
            commands::create_tab_webview,
            commands::set_tab_webview_bounds,
            commands::webview_url,
            commands::sync_tab_url,
            commands::update_tab_title,
            commands::update_tab_loading,
            commands::tab_reload,
            commands::tab_go_back,
            commands::tab_go_forward,
            commands::update_settings,
            commands::check_url_policy,
            commands::resolve_hostname,
            commands::set_window_icon,
            commands::set_dock_icon,
            commands::refresh_filter_lists,
            find::tab_find_string,
            find::tab_find_clear,
            overlay::focus_webview,
            overlay::ensure_overlay,
            overlay::set_overlay_bounds,
            overlay::hide_overlay,
            favicons::get_favicon_data_url,
            favicons::favicon_origin_hash,
            favicons::prefetch_favicon,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Load persisted state, falling back to defaults on corruption or absence.
/// Logs the chosen path so a user can tell from the dev console whether their
/// data survived a restart.
fn hydrate_state(storage: &SqliteStorage) -> BrowserState {
    let state = match storage.load_state() {
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
            BrowserState::new_first_run()
        }
        Err(err) => {
            log::warn!("failed to load persisted state ({err}); starting with defaults");
            BrowserState::new_first_run()
        }
    };

    // Heal any workspace that lost its tabs.
    let mut healed = state;
    let empty_workspace_ids: Vec<_> = healed
        .workspaces
        .iter()
        .filter(|w| w.tab_order.is_empty())
        .map(|w| w.id)
        .collect();

    for ws_id in empty_workspace_ids {
        match healed.with_new_tab(ws_id, "about:blank") {
            Ok((next, _)) => healed = next,
            Err(err) => log::warn!("could not seed tab for workspace {ws_id}: {err}"),
        }
    }

    if healed.settings.always_private {
        let (next, private_ws) =
            healed.with_new_private_workspace("Private".to_string(), wis_core::AccentColor::Ink);
        healed = next;
        match healed.with_new_tab(private_ws.id, "about:blank") {
            Ok((next, _)) => healed = next,
            Err(err) => log::warn!("could not seed tab for private workspace: {err}"),
        }
        if let Ok(next) = healed.with_active_workspace(private_ws.id) {
            healed = next;
        }
    }

    healed
}

/// Function to remove visual bug with liquid glass effect where the webview's
/// background would briefly flash solid during navigation.
#[cfg(target_os = "macos")]
fn apply_transparent_chrome_webview(app: &tauri::App) {
    use tauri::Manager;

    let Some(window) = app.get_webview_window("main") else {
        log::warn!("chrome transparency: no 'main' window at setup time");
        return;
    };
    window.set_shadow(false).ok();
}
