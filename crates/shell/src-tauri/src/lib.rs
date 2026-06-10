mod biometrics;
mod bookmarks;
mod capture;
mod commands;
mod content_filter;
mod cookies;
mod downloads;
mod error_compat;
mod external;
mod favicons;
mod find;
mod interaction_state;
mod menu;
mod navguard;
mod overlay;
mod pane_focus;
mod peek;
mod save_coalescer;
mod state;
mod suggest;
mod vault;
mod webview_prefs;
mod widget_panel;

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
            let storage = open_storage_resilient(&data_dir, &db_path);

            let browser_state = hydrate_state(&storage);
            let storage: Arc<dyn Storage> = Arc::new(storage);

            // Pick the initial filter list: cached EasyPrivacy if fresh,
            // otherwise the bundled fallback.
            let filter_cache_dir = data_dir.join("filter_lists");
            let (filter_text, _source) = filter_lists::load_cached_or_default(&filter_cache_dir);
            // Cache-aware build: a warm launch deserializes the compiled
            // engine instead of re-parsing the whole filter list.
            let tracker_blocker = match TrackerBlocker::from_filter_texts_with_cache(
                &[&filter_text],
                &filter_cache_dir,
            ) {
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

            // Drop compiled rule lists from previous filter-list versions so
            // the WebKit store doesn't accumulate stale entries forever.
            {
                let state = app.state::<AppState>();
                let keep: Vec<String> = [
                    wis_core::PrivacyMode::Strict,
                    wis_core::PrivacyMode::Balanced,
                    wis_core::PrivacyMode::Permissive,
                ]
                .into_iter()
                .filter_map(|mode| state.content_rule_spec(mode).map(|spec| spec.identifier))
                .collect();

                content_filter::prune_stale_rule_lists(keep);
            }

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

            // App handle for the native Opt+click "Peek" handler.
            peek::set_app_handle(app.handle().clone());

            // App handle for the native click navigation-guard handler.
            navguard::set_app_handle(app.handle().clone());

            // Periodically snapshot session cookies for activated workspaces so
            // logins survive a restart even when no page reload occurs (e.g. a
            // cookie set via fetch). Restore happens per-workspace on first tab.
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    // Drop any persisted sessions past their TTL at launch.
                    cookies::purge_persisted_sessions(&handle, false).await;
                    let mut tick = tokio::time::interval(std::time::Duration::from_secs(60));
                    tick.tick().await; // consume the immediate first tick
                    loop {
                        tick.tick().await;
                        let workspaces: Vec<wis_core::WorkspaceId> = handle
                            .state::<AppState>()
                            .restored_cookies
                            .lock()
                            .await
                            .iter()
                            .copied()
                            .collect();
                        for ws in workspaces {
                            cookies::persist_session_cookies(&handle, ws).await;
                        }
                    }
                });
            }

            // Idle proxy teardown: a workspace's loopback proxy (and its
            // sockets/tasks) is dropped once the workspace has had no live
            // webviews for a while; `ensure_proxy_for` lazily respawns it on
            // the next webview creation. The active workspace is exempt so
            // peek/widget panels never race a teardown.
            {
                const PROXY_IDLE_SECS: u64 = 10 * 60;

                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let mut idle_since: std::collections::HashMap<
                        wis_core::WorkspaceId,
                        std::time::Instant,
                    > = std::collections::HashMap::new();
                    let mut tick = tokio::time::interval(std::time::Duration::from_secs(60));
                    tick.tick().await; // consume the immediate first tick

                    loop {
                        tick.tick().await;
                        let state = handle.state::<AppState>();

                        // Workspaces that still have a live webview pointed at
                        // their proxy. Widget panels and Peek always belong to
                        // the active workspace.
                        let (tab_workspaces, active_ws) = {
                            let browser = state.browser.lock().await;
                            let map: std::collections::HashMap<String, wis_core::WorkspaceId> =
                                browser
                                    .tabs
                                    .iter()
                                    .map(|t| (t.id.as_uuid().to_string(), t.workspace_id))
                                    .collect();
                            (map, browser.active_workspace_id)
                        };
                        let mut live: std::collections::HashSet<wis_core::WorkspaceId> =
                            std::collections::HashSet::new();
                        live.insert(active_ws);
                        for label in handle.webviews().keys() {
                            if let Some(uuid) = label.strip_prefix("tab-") {
                                if let Some(ws) = tab_workspaces.get(uuid) {
                                    live.insert(*ws);
                                }
                            }
                        }

                        let mut proxies = state.proxies.lock().await;
                        let now = std::time::Instant::now();
                        idle_since.retain(|ws, _| proxies.contains_key(ws));
                        for ws in proxies.keys().copied().collect::<Vec<_>>() {
                            if live.contains(&ws) {
                                idle_since.remove(&ws);
                                continue;
                            }
                            let since = *idle_since.entry(ws).or_insert(now);
                            if now.duration_since(since).as_secs() >= PROXY_IDLE_SECS {
                                // ProxyServer::drop signals shutdown; the
                                // accept loop and its sockets wind down.
                                proxies.remove(&ws);
                                idle_since.remove(&ws);
                                log::info!(
                                    "workspace {ws} proxy torn down (no live webviews for {PROXY_IDLE_SECS}s)"
                                );
                            }
                        }
                    }
                });
            }

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
            commands::create_loose_tab,
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
            commands::create_tab_folder,
            commands::rename_tab_folder,
            commands::recolor_tab_folder,
            commands::set_tab_folder_collapsed,
            commands::delete_tab_folder,
            commands::reorder_tab_folders,
            commands::assign_tab_to_folder,
            commands::select_tab_folder,
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
            external::open_in_external_browser,
            downloads::list_downloads,
            downloads::remove_download,
            downloads::remove_downloads,
            downloads::open_download,
            downloads::reveal_download,
            interaction_state::tab_interaction_state,
            find::tab_find_string,
            find::tab_find_clear,
            peek::open_peek,
            peek::set_peek_bounds,
            peek::close_peek,
            peek::peek_set_reader,
            widget_panel::open_widget_panel,
            widget_panel::show_widget_panel,
            widget_panel::hide_widget_panel,
            widget_panel::set_widget_panel_bounds,
            widget_panel::close_widget_panel,
            widget_panel::close_all_widget_panels,
            widget_panel::list_widget_panels,
            widget_panel::reload_widget_panel,
            widget_panel::navigate_widget_panel,
            navguard::nav_guard_proceed,
            bookmarks::list_bookmark_folders,
            bookmarks::list_bookmarks,
            bookmarks::create_bookmark_folder,
            bookmarks::rename_bookmark_folder,
            bookmarks::reorder_bookmark_folders,
            bookmarks::delete_bookmark_folder,
            bookmarks::add_bookmark,
            bookmarks::move_bookmark,
            bookmarks::set_bookmark_pinned,
            bookmarks::reorder_pinned_bookmarks,
            bookmarks::reorder_bookmarks,
            bookmarks::delete_bookmark,
            overlay::focus_webview,
            overlay::ensure_overlay,
            overlay::set_overlay_bounds,
            overlay::hide_overlay,
            favicons::get_favicon_data_url,
            favicons::favicon_origin_hash,
            favicons::prefetch_favicon,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // Flush any debounced state save before the process goes away, so
            // the coalescer's quiet-period window can never lose the last
            // mutations of a session.
            if matches!(
                event,
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
            ) {
                let state = app_handle.state::<state::AppState>();
                state.save_coalescer.flush(&state.storage);
            }
        });
}

/// Open the state DB resiliently so a corrupt store never blocks launch or
/// silently wipes the session:
///  1. Open the primary DB; on success, refresh a known-good backup
///     (`wis.db.bak`) capturing the session at this launch.
///  2. If the primary won't open (corruption), set it aside, restore from the
///     backup, and reopen.
///  3. If there's no usable backup, start fresh (the corrupt file is kept as
///     `wis.db.corrupt-<ts>`).
fn open_storage_resilient(data_dir: &std::path::Path, db_path: &std::path::Path) -> SqliteStorage {
    let backup = data_dir.join("wis.db.bak");

    match SqliteStorage::open(db_path) {
        Ok(storage) => {
            if let Err(err) = storage.backup_to(&backup) {
                log::warn!("session backup failed (non-fatal): {err}");
            }
            storage
        }
        Err(open_err) => {
            log::error!("state DB failed to open ({open_err}); attempting recovery");
            let _ = std::fs::remove_file(data_dir.join("wis.db-wal"));
            let _ = std::fs::remove_file(data_dir.join("wis.db-shm"));
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = std::fs::rename(db_path, data_dir.join(format!("wis.db.corrupt-{stamp}")));

            if backup.exists() && std::fs::copy(&backup, db_path).is_ok() {
                if let Ok(storage) = SqliteStorage::open(db_path) {
                    log::warn!("recovered previous session from backup");
                    return storage;
                }
                log::error!("backup recovery failed; starting fresh");
                let _ = std::fs::remove_file(db_path);
            }

            SqliteStorage::open(db_path).expect("fresh state DB must open")
        }
    }
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
