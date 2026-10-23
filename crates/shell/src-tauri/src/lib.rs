// macOS is the primary platform and keeps the strict dead-code gate.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

mod address_field;
mod addresses;
mod ai;
mod ai_actions;
mod ai_fetch;
mod ai_local;
mod ai_models;
mod app_launch;
mod audio;
mod autoplay;
mod backup;
mod biometrics;
mod bookmarks;
mod browser_import;
mod canvas;
mod canvas_ai;
mod canvas_health;
mod canvas_store;
mod canvas_webview;
mod capture;
mod clean_links;
mod clipboard;
mod commands;
mod content_filter;
mod content_world;
mod cookies;
mod cosmetic;
mod default_browser;
mod devtools;
mod dictation;
mod document_text;
mod downloads;
mod edit_keys;
mod error_compat;
mod extensions;
mod external;
mod fatal_dialog;
mod favicons;
mod file_io;
mod filter_engine;
mod find;
mod gpc;
#[cfg(target_os = "linux")]
mod gtk_dispatch;
mod hardening;
mod hover_guard;
mod idle;
mod interaction_state;
mod keybind;
mod link_open;
#[cfg(any(windows, target_os = "linux"))]
mod load_failure;
mod load_progress;
mod memory;
mod menu;
#[cfg(target_os = "macos")]
mod native_webview;
mod navguard;
mod overlay;
mod page_archive;
mod page_blocks;
mod page_image;
mod page_menu;
#[cfg(any(windows, target_os = "linux", test))]
mod page_message;
mod page_preferences;
mod page_source;
mod page_text;
mod pane_container;
mod pane_focus;
mod panels;
mod peek;
mod permissions;
mod popup_chrome;
mod print;
mod print_export;
mod private_store;
mod reader;
mod save_coalescer;
mod screenshot;
mod selection_color;
mod signup;
mod site_protection;
mod state;
mod suggest;
mod swipe_nav;
mod tab_archive;
mod tab_preview;
mod tab_session;
mod themes;
mod uncloak;
mod usage;
mod vault;
mod vault_csv;
mod vault_recovery;
mod vault_store;
mod video_popout;
mod wake;
mod wallpapers;
#[cfg(target_os = "macos")]
mod webkit_features;
mod webview_close;
mod webview_prefs;
mod widget_panel;
#[cfg(windows)]
mod win_default_browser;
#[cfg(windows)]
mod win_dispatch;
#[cfg(windows)]
mod win_focus;
#[cfg(windows)]
mod win_webview2;
mod windows;
mod zoom;

use std::sync::Arc;

use tauri::Manager;
use wis_core::BrowserState;
use wis_network::filter_lists;
use wis_network::TrackerBlocker;
use wis_storage::{SqliteStorage, Storage, StorageError};

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    document_text::serve_child();
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,wis_network=debug"));
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_writer(std::io::stderr)
        .finish();
    let _ = tracing::subscriber::set_global_default(subscriber);
    webview_prefs::keep_ipc_global_off_remote_pages();
    #[cfg(target_os = "macos")]
    webkit_features::install_configuration_hook();

    let mut builder = tauri::Builder::default();

    #[cfg(any(windows, target_os = "linux"))]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            default_browser::handle_external_urls(app, argv.into_iter().skip(1));
            if let Some(window) = app.get_webview_window("main") {
                if let Err(err) = window.set_focus() {
                    log::warn!("focus after second-instance launch failed: {err}");
                }
            }
        }));
    }

    builder =
        builder.register_asynchronous_uri_scheme_protocol(canvas::SCHEME, canvas::handle_request);
    #[cfg(windows)]
    wry::set_profile_name_hook(Box::new(|label| {
        canvas::is_canvas_tab(label).then(|| canvas::WINDOWS_PROFILE.to_string())
    }));

    builder = builder
        .register_asynchronous_uri_scheme_protocol("wis-asset", wallpapers::handle_asset_request)
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(if cfg!(debug_assertions) {
                    log::LevelFilter::Debug
                } else {
                    log::LevelFilter::Warn
                })
                .build(),
        );

    // Auto-update is desktop-only: the updater verifies signed bundles against
    // the public key in tauri.conf.json, and `process` provides the relaunch
    // after an update lands.
    #[cfg(desktop)]
    {
        builder = builder
            .plugin(tauri_plugin_updater::Builder::new().build())
            .plugin(tauri_plugin_process::init());
    }

    builder
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("could not resolve app data directory");
            std::fs::create_dir_all(&data_dir)?;
            vault_store::keep_sealed_key_in(&data_dir);
            let db_path = data_dir.join("wis.db");

            log::info!("opening database at {}", db_path.display());
            let storage = match open_storage_resilient(&data_dir, &db_path) {
                Ok(storage) => storage,
                Err(fault) => {
                    let message = fault.user_message(&db_path);
                    log::error!("refusing to launch: {message}");
                    fatal_dialog::show("Wis cannot open this profile", &message);
                    std::process::exit(1);
                }
            };

            let (browser_state, origin) = hydrate_state(&storage);

            if origin == StateOrigin::Disk {
                if let Err(err) = storage.backup_to(&data_dir.join("wis.db.bak")) {
                    log::warn!("session backup failed (non-fatal): {err}");
                }
            }

            let storage: Arc<dyn Storage> = Arc::new(storage);

            // Pick the initial filter list: cached EasyPrivacy if fresh,
            // otherwise the bundled fallback.
            let filter_cache_dir = data_dir.join("filter_lists");
            let (filter_text, _source) = filter_lists::load_cached_or_default(&filter_cache_dir);
            // Cache-aware build: a warm launch deserializes the compiled
            // engine instead of re-parsing the whole filter list.
            let sub_urls = browser_state.settings.subscribed_lists();
            let (tracker_blocker, content_rules) = match filter_engine::build(
                &filter_cache_dir,
                &filter_text,
                &filter_engine::Subscriptions::of(&browser_state.settings),
            ) {
                Ok((blocker, rules)) => (Arc::new(blocker), rules),
                Err(err) => {
                    log::error!(
                        "failed to build tracker blocker ({err}); falling back to empty list"
                    );
                    (
                        Arc::new(TrackerBlocker::empty()),
                        content_filter::build_rule_specs_from(&filter_text, None),
                    )
                }
            };
            let resolver = std::sync::Arc::new(wis_network::SwappableResolver::new(
                state::build_resolver(browser_state.settings.doh_resolver.as_deref()),
            ));
            let auto_update = browser_state.settings.filter_list_auto_update;
            let initial_keybindings = browser_state.settings.keybindings.clone();
            let primary_workspace = browser_state.active_workspace_id;
            let restored_tab_ids: Vec<wis_core::TabId> =
                browser_state.tabs.iter().map(|t| t.id).collect();

            app.manage(AppState::new(
                browser_state,
                storage,
                tracker_blocker,
                resolver,
                filter_cache_dir.clone(),
                content_rules,
            ));

            // Restore persisted per-workspace window geometry into the cache
            // before any window opens (secondary windows read it at build time).
            {
                let state = app.state::<AppState>();
                if let Ok(Some(json)) = state.storage.get_meta(state::WINDOW_GEOMETRY_META) {
                    state.load_window_geometry(state::parse_window_geometry(&json));
                }
            }

            // Seed persisted per-tab sessions (WKWebView `interactionState`) so
            // each tab restores its full back/forward history + scroll on its
            // first webview creation.
            {
                let state = app.state::<AppState>();
                if let Err(err) = state.storage.purge_tab_sessions_except(&restored_tab_ids) {
                    log::warn!("purge orphaned tab sessions: {err}");
                }
                if let Some(dir) = tab_preview::previews_dir(app.handle()) {
                    tab_preview::purge_except(&dir, &restored_tab_ids);
                }

                match state.storage.load_tab_sessions() {
                    Ok(sessions) if !sessions.is_empty() => {
                        let restored: std::collections::HashSet<wis_core::TabId> =
                            restored_tab_ids.iter().copied().collect();
                        match state.pending_restore_states.try_lock() {
                            Ok(mut guard) => {
                                for (id, blob) in sessions {
                                    if restored.contains(&id) {
                                        guard.insert(id, blob);
                                    }
                                }
                            }
                            Err(_) => {
                                log::warn!("pending_restore_states busy at startup; not seeded")
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(err) => log::warn!("load persisted tab sessions: {err}"),
                }
            }

            // The main window is created hidden (`visible: false`): revealed after
            // sized/positioned according to its workspace's saved geometry.
            if let Some(main) = app.get_webview_window("main") {
                if let Some(g) = app.state::<AppState>().window_geometry_for(primary_workspace) {
                    let _ = main.set_size(tauri::LogicalSize::new(g.width, g.height));
                    let _ = main.set_position(tauri::LogicalPosition::new(g.x, g.y));
                }
                let app_handle = app.handle().clone();

                main.on_window_event(move |event| {
                    match event {
                        // Same page teardown as secondary windows (windows.rs).
                        tauri::WindowEvent::CloseRequested { .. } => {
                            webview_close::terminate_window_pages(&app_handle, "main");
                        }
                        tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_) => {
                            windows::record_window_geometry(&app_handle, "main");
                        }
                        _ => {}
                    }
                });

                // Windows glass applied by Mica.
                #[cfg(windows)]
                let _ = main.set_effects(windows::chrome_window_effects());

                // Forward the boot activation's frame-level WM_SETFOCUS (and
                // every later one) into a webview, before the window can gain
                // focus for the first time; see `win_focus`.
                #[cfg(windows)]
                if let Some(win) = app.get_window("main") {
                    win_focus::install(&win);
                }

                let _ = main.show();

                // On Windows, show() alone may leave the window shown but not
                // focused, and keyboard focus only reaches a WebView2 once the
                // window has it (the chrome also calls focus_self on mount for
                // the webview-level half; see useKeybindCapture).
                #[cfg(windows)]
                let _ = main.set_focus();
            }

            // Clear any ephemeral-workspace WebView2 data folders a previous run left behind.
            #[cfg(windows)]
            commands::sweep_ephemeral_webview_data(app.handle());

            // Drop compiled rule lists from previous filter-list versions so
            // the WebKit store doesn't accumulate stale entries forever.
            {
                let state = app.state::<AppState>();
                content_filter::prune_stale_rule_lists(state.wis_rule_list_ids());
            }

            // Background EasyPrivacy refresh. Runs once per launch when the
            // setting is on and the cache is stale; failures only log so
            // offline launches still proceed with the bundled fallback.
            let subs_stale = sub_urls
                .iter()
                .any(|u| filter_lists::fetch::subscription_is_stale(&filter_cache_dir, u));
            if auto_update
                && (filter_lists::fetch::cache_is_stale(&filter_cache_dir)
                    || filter_lists::fetch::easylist_cache_is_stale(&filter_cache_dir)
                    || filter_lists::fetch::resources_are_stale(&filter_cache_dir)
                    || subs_stale)
            {
                let handle = app.handle().clone();
                let sub_urls = sub_urls.clone();
                tauri::async_runtime::spawn(async move {
                    if filter_lists::fetch::cache_is_stale(&filter_cache_dir) {
                        if let Err(err) =
                            filter_lists::fetch::refresh_easyprivacy(&filter_cache_dir).await
                        {
                            log::warn!("filter_lists: easyprivacy refresh failed: {err}");
                        }
                    }
                    if filter_lists::fetch::easylist_cache_is_stale(&filter_cache_dir) {
                        if let Err(err) =
                            filter_lists::fetch::refresh_easylist(&filter_cache_dir).await
                        {
                            log::warn!("filter_lists: easylist refresh failed: {err}");
                        }
                    }
                    if filter_lists::fetch::resources_are_stale(&filter_cache_dir) {
                        if let Err(err) =
                            filter_lists::fetch::refresh_resources(&filter_cache_dir).await
                        {
                            log::warn!("filter_lists: resources refresh failed: {err}");
                        }
                    }
                    for url in &sub_urls {
                        if filter_lists::fetch::subscription_is_stale(&filter_cache_dir, url) {
                            if let Err(err) =
                                filter_lists::fetch::refresh_subscription(&filter_cache_dir, url)
                                    .await
                            {
                                log::warn!("filter_lists: subscription refresh failed: {err}");
                            }
                        }
                    }

                    // One rebuild picks up whatever landed.
                    let state = handle.state::<AppState>();
                    if let Err(err) = commands::rebuild_filter_lists(&handle, &state).await {
                        log::warn!("filter_lists: rebuild after refresh failed: {err}");
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

            // Trackpad scroll-phase monitor (macOS; no-op elsewhere).
            swipe_nav::install_phase_monitor(app.handle());

            // PDFs and attachment responses download instead of rendering in
            // WebKit's viewer.
            #[cfg(target_os = "macos")]
            wry::set_main_frame_download_policy(&["application/pdf"], true);

            // App handle for the native "Save password?" capture handler.
            capture::set_app_handle(app.handle().clone());
            #[cfg(target_os = "macos")]
            page_preferences::install_hook(app.handle().clone());
            #[cfg(target_os = "macos")]
            page_blocks::install(app.handle().clone());
            #[cfg(target_os = "macos")]
            clean_links::install(app.handle().clone());
            signup::set_app_handle(app.handle().clone());

            // App handle for the AI page-text channel's native handler.
            page_text::set_app_handle(app.handle().clone());
            page_menu::set_app_handle(app.handle().clone());
            page_source::set_app_handle(app.handle().clone());

            // App handle for the assistant's web-fetch dispatchers.
            #[cfg(any(windows, target_os = "linux"))]
            ai_fetch::set_app_handle(app.handle().clone());

            // App handle for the native Opt+click "Peek" handler.
            peek::set_app_handle(app.handle().clone());

            // App handle for the link-open handler.
            link_open::set_app_handle(app.handle().clone());

            // A first launch's argv can itself carry a URL (Wis started by a
            // link click); queued until the chrome drains it.
            #[cfg(any(windows, target_os = "linux"))]
            default_browser::handle_external_urls(
                app.handle(),
                std::env::args_os().skip(1).filter_map(|a| a.into_string().ok()),
            );

            // App handle for the cosmetic-filtering handler.
            cosmetic::set_app_handle(app.handle().clone());

            // App handle for the native click navigation-guard handler.
            navguard::set_app_handle(app.handle().clone());

            // App handle for the video-popout message handler.
            video_popout::set_app_handle(app.handle().clone());

            // App handle for the popup hover-guard.
            hover_guard::set_app_handle(app.handle().clone());

            wake::install(app.handle());

            // App handle for the camera/microphone consent message handler.
            permissions::set_app_handle(app.handle().clone());

            // App handle for the popup open-in-app navigation delegate.
            app_launch::set_app_handle(app.handle().clone());

            // App handle for the find-in-page match-count delegate.
            find::set_app_handle(app.handle().clone());

            // App handle for the page-side keyboard-shortcut forwarder.
            keybind::set_app_handle(app.handle().clone());
            #[cfg(target_os = "macos")]
            keybind::offer_page_first_combos_to_tabs();

            // App handle for the native page-load failure callbacks.
            #[cfg(any(windows, target_os = "linux"))]
            load_failure::set_app_handle(app.handle().clone());

            // Per-tab audibility poll (tab audio icons + click-to-mute).
            audio::start_poll(app.handle().clone());
            tab_archive::start(app.handle().clone());
            usage::start(app.handle().clone());
            canvas::start(app.handle().clone());
            #[cfg(target_os = "macos")]
            extensions::start(app.handle());

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
            commands::set_workspace_background,
            commands::set_workspace_icon,
            commands::set_tab_folder_icon,
            wallpapers::import_wallpaper,
            themes::set_active_theme,
            themes::export_theme,
            themes::import_theme,
            themes::encode_theme_code,
            themes::decode_theme_code,
            backup::export_backup,
            backup::import_backup,
            commands::set_workspace_custom_accent,
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
            vault_recovery::vault_recovery_key,
            vault_recovery::vault_set_recovery_key,
            vault_recovery::vault_remove_recovery_key,
            vault_recovery::vault_recover,
            vault_recovery::vault_reset,
            vault_store::vault_create_password,
            addresses::address_list,
            addresses::address_any,
            address_field::address_fill,
            address_field::release_address_field,
            addresses::address_save,
            addresses::address_delete,
            vault_store::vault_unlock_password,
            vault_store::vault_change_password,
            vault::vault_list,
            vault::vault_save,
            vault::vault_reveal,
            vault::generate_password,
            vault::vault_check_breaches,
            vault::vault_delete,
            vault::vault_has_credentials,
            vault_csv::vault_import_csv,
            vault_csv::vault_export_csv,
            vault::autofill_candidates,
            vault::autofill_fill,
            vault::generate_password_for_field,
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
            commands::create_background_tab,
            commands::create_related_tab,
            commands::split_pane,
            commands::split_pane_with,
            commands::replace_pane_leaf,
            commands::set_pane_ratio,
            commands::unsplit_panes,
            commands::close_tab,
            commands::restore_closed_tab,
            commands::reopen_closed_tab,
            commands::list_closed_tabs,
            commands::forget_closed_tab,
            commands::clear_closed_tabs,
            commands::move_tab_to_new_window,
            commands::move_tab_to_workspace,
            commands::tab_hard_reload,
            commands::tab_zoom,
            commands::zoom_forget_site,
            site_protection::tab_set_site_protection,
            hardening::set_workspace_lockdown,
            site_protection::site_protection_session,
            page_blocks::tab_page_blocks,
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
            commands::set_canvas_ground,
            commands::create_tab_webview,
            commands::close_tab_webview,
            commands::set_tab_webview_bounds,
            commands::webview_url,
            commands::sync_tab_url,
            commands::update_tab_title,
            commands::update_tab_loading,
            commands::tab_reload,
            commands::tab_stop,
            commands::tab_set_reader,
            commands::tab_go_back,
            commands::tab_go_forward,
            tab_preview::stash_tab_preview,
            tab_preview::capture_tab_preview,
            commands::update_settings,
            commands::apply_shortcuts,
            commands::check_url_policy,
            commands::resolve_hostname,
            commands::set_window_icon,
            commands::set_dock_icon,
            commands::refresh_filter_lists,
            external::open_in_external_browser,
            default_browser::default_browser_status,
            default_browser::set_default_browser,
            default_browser::drain_external_urls,
            default_browser::route_external_link,
            clipboard::copy_text,
            clipboard::clean_link,
            clean_links::clean_followed_link,
            memory::list_webview_memory,
            commands::start_element_picker,
            commands::import_userscript,
            downloads::list_downloads,
            downloads::cancel_download,
            downloads::remove_download,
            downloads::remove_downloads,
            downloads::open_download,
            downloads::reveal_download,
            interaction_state::tab_interaction_state,
            tab_session::persist_tab_session,
            find::tab_find_string,
            find::tab_find_clear,
            print::print_active_tab,
            screenshot::capture_to_clipboard,
            devtools::inspect_tab,
            page_archive::save_page_archive,
            page_image::copy_image,
            page_menu::clear_menu_selection,
            signup::fill_generated_password,
            signup::signup_field,
            signup::release_signup_field,
            page_source::capture_page_source,
            page_source::page_source,
            page_image::save_image_as,
            screenshot::save_last_screenshot,
            peek::open_peek,
            peek::set_peek_bounds,
            peek::close_peek,
            peek::peek_set_reader,
            peek::peek_go_back,
            peek::peek_go_forward,
            peek::peek_reload,
            widget_panel::open_widget_panel,
            widget_panel::show_widget_panel,
            widget_panel::hide_widget_panel,
            widget_panel::set_widget_panel_bounds,
            widget_panel::close_widget_panel,
            widget_panel::close_all_widget_panels,
            widget_panel::list_widget_panels,
            widget_panel::reload_widget_panel,
            widget_panel::navigate_widget_panel,
            video_popout::open_video_popout,
            video_popout::close_video_popout,
            audio::set_tab_muted,
            audio::tab_audio_state,
            widget_panel::set_widget_panel_muted,
            widget_panel::widget_panel_muted,
            windows::new_window,
            windows::open_url_in_new_window,
            windows::free_workspaces,
            windows::restore_session_windows,
            swipe_nav::set_swipe_accent,
            navguard::nav_guard_proceed,
            ai::ai_set_key,
            ai::ai_key_present,
            ai::ai_clear_key,
            ai::ai_chat,
            ai::ai_cancel,
            ai::ai_assess_link,
            ai_models::ai_list_models,
            extensions::extensions_supported,
            extensions::extension_choose,
            extensions::extension_install,
            extensions::extension_discard,
            extensions::extension_remove,
            extensions::extension_set_workspace,
            extensions::extension_set_private,
            extensions::extension_set_pinned,
            extensions::extension_actions,
            extensions::extension_run,
            extensions::extension_open_options,
            extensions::access::extension_answer_permission,
            extensions::access::extension_granted_abilities,
            extensions::access::extension_revoke_granted,
            extensions::access::extension_set_site_access,
            extensions::access::extension_set_site,
            extensions::access::extension_set_site_here,
            extensions::store::extension_store_offer,
            extensions::store::extension_stage_from_store,
            extensions::updates::extension_check_updates,
            extensions::updates::extension_approve_update,
            extensions::linking::extension_link_store,
            extensions::linking::extension_decline_listing,
            extensions::extension_dismiss_popup,
            ai_fetch::ai_web_search,
            ai_fetch::ai_web_fetch,
            ai_actions::ai_preview_tabs,
            ai_actions::ai_preview_urls,
            ai_actions::ai_open_tabs,
            ai_actions::ai_switch_tab,
            ai_actions::ai_close_tabs,
            ai_actions::ai_group_tabs,
            ai_local::ai_list_tabs,
            ai_local::ai_resolve_tab,
            ai_local::ai_search_history,
            ai_local::ai_search_bookmarks,
            page_text::request_page_text,
            permissions::resolve_media_permission,
            app_launch::app_launch_decide,
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
            bookmarks::export_bookmarks_html,
            bookmarks::import_bookmarks_html,
            browser_import::list_import_sources,
            browser_import::import_browser_bookmarks,
            browser_import::import_browser_history,
            usage::usage_summary,
            overlay::focus_webview,
            overlay::focus_self,
            overlay::ensure_overlay,
            overlay::set_overlay_bounds,
            overlay::set_overlay_position,
            overlay::size_overlay,
            overlay::hide_overlay,
            overlay::promote_overlay,
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
                usage::flush_now(&state);
                canvas::flush_logged(state.storage.as_ref());
                canvas::quit(app_handle);
            }

            if let tauri::RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::Focused(focused),
                ..
            } = &event
            {
                usage::window_focused(app_handle, label, *focused);
            }

            // The floating video popout must not outlive the main window
            // (its chrome orchestration dies with it); without this the
            // orphaned popout would also keep the app alive.
            if let tauri::RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::Destroyed,
                ..
            } = &event
            {
                if label == "main" {
                    video_popout::close_popout_window(app_handle);
                    if let Some(id) = app_handle.state::<AppState>().window_workspace("main") {
                        private_store::release(app_handle, id);
                        uncloak::clear(&app_handle.state(), id);
                        #[cfg(target_os = "macos")]
                        extensions::release(app_handle, id);
                    }
                }
            }

            // Links opened from other apps (Wis as the default browser)
            // arrive as Apple Events; queued until the chrome can open tabs.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Opened { urls } = &event {
                default_browser::handle_external_urls(
                    app_handle,
                    urls.iter().map(|u| u.to_string()),
                );
            }
        });
}

/// A profile that refuses to launch over, because launching would either lose
/// data or pretend a broken profile is fine.
enum ProfileFault {
    /// Written by a build newer than this one.
    TooNew { found: usize, supported: usize },
    /// Passes SQLite's integrity check but still would not open.
    Unreadable(String),
}

impl ProfileFault {
    /// Message shown to the user before the app quits. Actionable without a
    /// console.
    fn user_message(&self, db_path: &std::path::Path) -> String {
        match self {
            Self::TooNew { found, supported } => format!(
                "This profile was created by a newer version of Wis \
                 (profile format v{found}; this build understands v{supported}).\n\n\
                 Your data is safe and untouched. Update Wis, or launch the newer \
                 build, to open it.\n\n{}",
                db_path.display()
            ),
            Self::Unreadable(err) => format!(
                "Wis could not open your profile, but the file is undamaged. \
                 \n\n{err}\n\n{}",
                db_path.display()
            ),
        }
    }
}

/// A locked database is transient and undamaged: another process may hold it
/// for a moment during launch. Retry before concluding anything.
const OPEN_ATTEMPTS: usize = 5;
const OPEN_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(200);

fn open_with_retry(db_path: &std::path::Path) -> Result<SqliteStorage, StorageError> {
    for attempt in 1..=OPEN_ATTEMPTS {
        match SqliteStorage::open(db_path) {
            Err(err) if err.is_transient() && attempt < OPEN_ATTEMPTS => {
                log::warn!("state DB busy ({err}); retrying ({attempt}/{OPEN_ATTEMPTS})");
                std::thread::sleep(OPEN_RETRY_DELAY);
            }
            other => return other,
        }
    }

    unreachable!("loop returns on its final attempt")
}

/// Move `wis.db` and its WAL sidecars aside under a shared timestamp.
///
/// The sidecars travel *with* the database rather than being deleted: a WAL can
/// hold committed transactions that have not been checkpointed yet.
fn set_aside(data_dir: &std::path::Path, db_path: &std::path::Path) -> String {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let target = format!("wis.db.corrupt-{stamp}");

    for suffix in ["-wal", "-shm"] {
        let sidecar = data_dir.join(format!("wis.db{suffix}"));
        if sidecar.exists() {
            let _ = std::fs::rename(&sidecar, data_dir.join(format!("{target}{suffix}")));
        }
    }
    let _ = std::fs::rename(db_path, data_dir.join(&target));

    target
}

/// Open the state DB resiliently. A profile is only ever set aside when SQLite
/// itself says the file is damaged; every other failure leaves the file
/// untouched:
///  1. Open the primary DB, retrying while it is merely busy.
///  2. A profile from a newer build, or an intact-but-unopenable one, is a
///     [`ProfileFault`]: report it and let the caller refuse to launch.
///  3. Only genuine corruption sets the file aside (with its WAL sidecars) and
///     restores from `wis.db.bak`, falling back to a fresh profile.
fn open_storage_resilient(
    data_dir: &std::path::Path,
    db_path: &std::path::Path,
) -> Result<SqliteStorage, ProfileFault> {
    let open_err = match open_with_retry(db_path) {
        Ok(storage) => return Ok(storage),
        Err(err) => err,
    };

    if let StorageError::ProfileTooNew { found, supported } = open_err {
        log::error!("profile is from a newer build (v{found} > v{supported}); leaving it alone");
        return Err(ProfileFault::TooNew { found, supported });
    }

    if db_path.exists() && wis_storage::is_intact(db_path) {
        log::error!("state DB failed to open ({open_err}) but is intact; leaving it alone");
        return Err(ProfileFault::Unreadable(open_err.to_string()));
    }

    log::error!("state DB is corrupt ({open_err}); attempting recovery");
    let set_aside_as = set_aside(data_dir, db_path);
    log::warn!("corrupt profile kept as {set_aside_as}");

    let backup = data_dir.join("wis.db.bak");
    if backup.exists() && std::fs::copy(&backup, db_path).is_ok() {
        match open_with_retry(db_path) {
            Ok(storage) => {
                log::warn!("recovered previous session from backup");
                return Ok(storage);
            }
            Err(err) => log::error!("backup recovery failed ({err}); starting fresh"),
        }
        let _ = std::fs::remove_file(db_path);
    }

    open_with_retry(db_path).map_err(|err| ProfileFault::Unreadable(err.to_string()))
}

/// Where the hydrated state came from.
///
/// Only a profile that actually loaded is worth backing up.
#[derive(Clone, Copy, PartialEq, Eq)]
enum StateOrigin {
    Disk,
    Fresh,
}

/// Load persisted state, falling back to defaults on corruption or absence.
/// Logs the chosen path so a user can tell from the dev console whether their
/// data survived a restart.
fn hydrate_state(storage: &SqliteStorage) -> (BrowserState, StateOrigin) {
    let (state, origin) = match storage.load_state() {
        Ok(Some(state)) => {
            log::info!(
                "hydrated state from disk: {} workspaces, {} tabs",
                state.workspaces.len(),
                state.tabs.len(),
            );
            (state, StateOrigin::Disk)
        }
        Ok(None) => {
            log::info!("no persisted state found, starting with defaults");
            (BrowserState::new_first_run(), StateOrigin::Fresh)
        }
        Err(err) => {
            log::warn!("failed to load persisted state ({err}); starting with defaults");
            (BrowserState::new_first_run(), StateOrigin::Fresh)
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

    (healed, origin)
}

/// Disable the window shadow so the liquid-glass chrome does not briefly flash a
/// solid backing during navigation. Per-window, so any new OS window can reuse it.
#[cfg(target_os = "macos")]
pub(crate) fn disable_chrome_window_shadow<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    window.set_shadow(false).ok();
}

/// Apply the chrome transparency tweak to the startup `main` window at setup.
#[cfg(target_os = "macos")]
fn apply_transparent_chrome_webview(app: &tauri::App) {
    use tauri::Manager;

    let Some(window) = app.get_webview_window("main") else {
        log::warn!("chrome transparency: no 'main' window at setup time");
        return;
    };
    disable_chrome_window_shadow(&window);
}

#[cfg(test)]
mod profile_open_tests {
    use super::{open_storage_resilient, set_aside, ProfileFault};
    use std::fs;
    use std::path::PathBuf;
    use wis_storage::SqliteStorage;

    fn temp_data_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("wis-profile-open-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp data dir");
        dir
    }

    /// Stamp `user_version` straight into the SQLite header (bytes 60..64,
    /// big-endian), standing in for a profile written by a newer build. The
    /// header carries no checksum, so an in-place edit is safe.
    fn stamp_schema_version(db: &std::path::Path, version: u32) {
        let mut bytes = fs::read(db).expect("read DB");
        bytes[60..64].copy_from_slice(&version.to_be_bytes());
        fs::write(db, &bytes).expect("write stamped DB");
    }

    /// Set-aside database files, excluding their `-wal`/`-shm` sidecars.
    fn set_aside_files(dir: &std::path::Path) -> Vec<String> {
        fs::read_dir(dir)
            .expect("read data dir")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| {
                name.starts_with("wis.db.corrupt-")
                    && !name.ends_with("-wal")
                    && !name.ends_with("-shm")
            })
            .collect()
    }

    #[test]
    fn healthy_profile_opens() {
        let dir = temp_data_dir("healthy");
        let db = dir.join("wis.db");
        drop(SqliteStorage::open(&db).expect("create DB"));

        assert!(open_storage_resilient(&dir, &db).is_ok());
        assert!(set_aside_files(&dir).is_empty());
    }

    #[test]
    fn profile_from_newer_build_is_never_set_aside() {
        let dir = temp_data_dir("too-new");
        let db = dir.join("wis.db");
        drop(SqliteStorage::open(&db).expect("create DB"));
        stamp_schema_version(&db, 9_999);
        let before = fs::read(&db).expect("read DB");

        match open_storage_resilient(&dir, &db) {
            Err(ProfileFault::TooNew { found, .. }) => assert_eq!(found, 9_999),
            Err(ProfileFault::Unreadable(err)) => panic!("expected TooNew, got Unreadable: {err}"),
            Ok(_) => panic!("expected the open to be refused"),
        }

        // The whole point: the profile is still there, byte for byte.
        assert!(set_aside_files(&dir).is_empty(), "profile was set aside");
        assert_eq!(fs::read(&db).expect("read DB"), before);
    }

    #[test]
    fn set_aside_moves_the_wal_sidecars_instead_of_deleting_them() {
        let dir = temp_data_dir("sidecars");
        let db = dir.join("wis.db");
        fs::write(&db, b"database").expect("write DB");
        fs::write(dir.join("wis.db-wal"), b"pending commits").expect("write WAL");
        fs::write(dir.join("wis.db-shm"), b"shared memory").expect("write SHM");

        let target = set_aside(&dir, &db);

        // A WAL can hold committed transactions the main file never received,
        // so it has to travel with the database rather than be deleted.
        assert_eq!(
            fs::read(dir.join(format!("{target}-wal"))).expect("read kept WAL"),
            b"pending commits"
        );
        assert_eq!(
            fs::read(dir.join(format!("{target}-shm"))).expect("read kept SHM"),
            b"shared memory"
        );
        assert_eq!(
            fs::read(dir.join(&target)).expect("read kept DB"),
            b"database"
        );

        assert!(!db.exists());
        assert!(!dir.join("wis.db-wal").exists());
        assert!(!dir.join("wis.db-shm").exists());
    }

    #[test]
    fn corrupt_profile_is_set_aside_and_launch_continues() {
        let dir = temp_data_dir("corrupt");
        let db = dir.join("wis.db");
        drop(SqliteStorage::open(&db).expect("create DB"));

        let mut bytes = fs::read(&db).expect("read DB");
        bytes[0..16].copy_from_slice(b"not-a-sqlite-db!");
        fs::write(&db, &bytes).expect("write garbled DB");

        assert!(
            open_storage_resilient(&dir, &db).is_ok(),
            "a corrupt profile should still start fresh"
        );
        assert_eq!(
            set_aside_files(&dir).len(),
            1,
            "the corrupt profile should be kept, not deleted"
        );
    }
}
