// macOS is the primary platform and keeps the strict dead-code gate.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

mod ai;
mod ai_fetch;
mod app_launch;
mod audio;
mod autoplay;
mod backup;
mod biometrics;
mod bookmarks;
mod capture;
mod clipboard;
mod commands;
mod content_filter;
mod cookies;
mod cosmetic;
mod default_browser;
mod devtools;
mod downloads;
mod edit_keys;
mod error_compat;
mod external;
mod favicons;
mod file_io;
mod find;
mod gpc;
#[cfg(target_os = "linux")]
mod gtk_dispatch;
mod hover_guard;
mod interaction_state;
#[cfg(any(windows, target_os = "linux"))]
mod keybind;
mod link_open;
#[cfg(any(windows, target_os = "linux"))]
mod load_failure;
mod load_progress;
mod memory;
mod menu;
mod navguard;
mod overlay;
mod page_archive;
mod page_image;
mod page_menu;
mod page_text;
mod pane_focus;
mod panels;
mod peek;
mod permissions;
mod popup_chrome;
mod print;
mod reader;
mod save_coalescer;
mod screenshot;
mod selection_color;
mod state;
mod suggest;
mod swipe_nav;
mod tab_preview;
mod tab_session;
mod themes;
mod vault;
mod video_popout;
mod wallpapers;
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
            // EasyList + user subscriptions (cosmetic, scriptlet and ad
            // rules) join the engine when fresh caches exist; they never
            // feed the WKContentRuleList compiler.
            let easylist_text = filter_lists::load_cached_easylist(&filter_cache_dir);
            let sub_urls = browser_state.settings.filter_subscriptions.clone();
            let sub_texts = filter_lists::load_cached_subscriptions(&filter_cache_dir, &sub_urls);
            let mut engine_texts: Vec<&str> = vec![&filter_text];
            if let Some(t) = easylist_text.as_deref() {
                engine_texts.push(t);
            }
            for t in &sub_texts {
                engine_texts.push(t);
            }
            let tracker_blocker = match TrackerBlocker::from_filter_texts_with_cache(
                &engine_texts,
                &filter_cache_dir,
            ) {
                Ok(mut b) => {
                    if let Some(json) = filter_lists::load_cached_resources(&filter_cache_dir) {
                        if let Err(err) = b.load_resources(&json) {
                            log::warn!("filter_lists: scriptlet resources load failed: {err}");
                        }
                    }
                    Arc::new(b)
                }
                Err(err) => {
                    log::error!(
                        "failed to build tracker blocker ({err}); falling back to empty list"
                    );
                    Arc::new(TrackerBlocker::empty())
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
                &filter_text,
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
                    let (text, _) = filter_lists::load_cached_or_default(&filter_cache_dir);
                    let state = handle.state::<AppState>();
                    if let Err(err) = state.rebuild_filter_engine(text).await {
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
            autoplay::install_hook(app.handle().clone());

            // App handle for the AI page-text channel's native handler.
            page_text::set_app_handle(app.handle().clone());
            page_menu::set_app_handle(app.handle().clone());

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

            // App handle for the camera/microphone consent message handler.
            permissions::set_app_handle(app.handle().clone());

            // App handle for the popup open-in-app navigation delegate.
            app_launch::set_app_handle(app.handle().clone());

            // App handle for the find-in-page match-count delegate.
            find::set_app_handle(app.handle().clone());

            // App handle for the page-side keyboard-shortcut forwarder.
            #[cfg(any(windows, target_os = "linux"))]
            keybind::set_app_handle(app.handle().clone());

            // App handle for the native page-load failure callbacks.
            #[cfg(any(windows, target_os = "linux"))]
            load_failure::set_app_handle(app.handle().clone());

            // Per-tab audibility poll (tab audio icons + click-to-mute).
            audio::start_poll(app.handle().clone());

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
            vault::vault_list,
            vault::vault_save,
            vault::vault_reveal,
            vault::generate_password,
            vault::vault_check_breaches,
            vault::vault_delete,
            vault::vault_has_credentials,
            vault::vault_import_csv,
            vault::vault_export_csv,
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
            commands::move_tab_to_new_window,
            commands::move_tab_to_workspace,
            commands::tab_hard_reload,
            commands::tab_zoom,
            commands::zoom_forget_site,
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
            clipboard::copy_text,
            clipboard::clean_link,
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
            ai_fetch::ai_web_search,
            ai_fetch::ai_web_fetch,
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
