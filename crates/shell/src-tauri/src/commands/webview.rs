use super::*;

/// A macOS provisional page-load failure surfaced through the wry side
/// channel: `(NSError code, error domain, failing leaf-certificate DER)`.
type NsPageLoadFailure = (i64, String, Option<Vec<u8>>, Option<String>);

/// Create a Tauri child webview for a tab using `WebviewBuilder` directly.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn create_tab_webview(
    app: AppHandle,
    state: State<'_, AppState>,
    window_label: String,
    label: String,
    url: String,
    workspace_id: WorkspaceId,
    tab_id: TabId,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    restore_state: Option<String>,
) -> WisResult<()> {
    let window = app
        .get_window(&window_label)
        .ok_or_else(|| WisError::InvalidInput {
            message: format!("no window labelled {window_label}"),
        })?;

    let parsed = Url::parse(&url).map_err(|_| WisError::InvalidUrl { url: url.clone() })?;
    // A canvas tab loads its bundled page and may read its workspace's canvases.
    let canvas_tab = parsed.scheme() == crate::canvas::SCHEME;
    // Session restore: the frontend passes a blob when waking a slept tab; a
    // reopened (Cmd+Shift+T) tab's blob waits Rust-side keyed by tab id.
    let restore_state = match restore_state {
        Some(blob) => Some(blob),
        None => state.pending_restore_states.lock().await.remove(&tab_id),
    };
    let initial_url = if restore_state.is_some() {
        Url::parse("about:blank").expect("about:blank is a valid URL")
    } else {
        parsed.clone()
    };

    let label_ident = label.clone();

    let (
        ephemeral,
        user_agent,
        fraud_warning,
        media_extensions,
        nav_privacy_mode,
        global_privacy_control,
    ) = {
        let browser = state.browser.lock().await;
        let ephemeral = browser
            .workspace(workspace_id)
            .map(|w| w.ephemeral)
            .unwrap_or(false);
        let user_agent = wis_core::ua_for_host(
            parsed.host_str().unwrap_or_default(),
            &browser.settings.ua_site_overrides,
            &browser.settings.user_agent_profile,
            wis_core::UaPlatform::host(),
        );
        let nav_privacy_mode = browser
            .workspace(workspace_id)
            .map(|w| w.privacy_mode)
            .unwrap_or_default();

        (
            ephemeral,
            user_agent,
            browser.settings.fraud_warning_enabled,
            browser.settings.media_extensions_enabled,
            nav_privacy_mode,
            browser.settings.global_privacy_control,
        )
    };

    // The swipe-nav indicator colour; a new tab bakes the last
    // value the chrome pushed (`set_swipe_accent`).
    let swipe_accent = state
        .swipe_accent
        .lock()
        .expect("swipe_accent poisoned")
        .clone();

    let proxy_port = state
        .ensure_proxy_for(workspace_id)
        .await
        .map_err(wis_core::WisError::from)?;
    let proxy_url = format!("http://127.0.0.1:{proxy_port}")
        .parse::<Url>()
        .expect("loopback proxy URL is valid");

    let mut builder = WebviewBuilder::new(label, WebviewUrl::External(initial_url))
        .initialization_script(tauri_runtime_wry::SKIP_TAURI_SCRIPTS)
        .devtools(!ephemeral)
        .focused(false)
        .proxy_url(proxy_url)
        .accept_first_mouse(true)
        .disable_drag_drop_handler()
        // Global Privacy Control's page signal, in every frame.
        .initialization_script_for_all_frames(crate::gpc::page_script(global_privacy_control))
        // Swipe back/forward: publish the indicator colour, then the gesture+UI.
        .initialization_script(crate::swipe_nav::accent_script(&swipe_accent));

    // Ride the webview configuration so even an instantly-loading first
    // document (cache, localhost, the restored startup page) gets them. macOS
    // and Linux run these in Wis's own script world instead (`content_world`).
    #[cfg(target_os = "linux")]
    {
        builder = builder.initialization_script(crate::video_popout::page_script());
    }

    // Windows has no such world, so its copies post through a private sender
    // instead (`win_dispatch::private_script`).
    #[cfg(windows)]
    let token = crate::win_dispatch::token(&label_ident);
    #[cfg(windows)]
    {
        let private = |source: &str| crate::win_dispatch::private_script(&token, source, &[]);
        builder = builder
            .initialization_script(private(crate::video_popout::page_script()))
            .initialization_script(private(crate::peek::page_script()))
            .initialization_script(private(crate::link_open::page_script()))
            .initialization_script(private(&crate::page_menu::page_script()))
            .initialization_script(private(crate::signup::page_script()));
    }

    // The scroll-phase monitor.
    #[cfg(target_os = "macos")]
    {
        builder = builder.initialization_script(crate::swipe_nav::phase_flag_script());
    }
    builder = builder.initialization_script(crate::swipe_nav::page_script());

    // Windows multiplexes every channel over WebView2's single message pipe: the
    // shim recreates `window.webkit.messageHandlers` for the page-world ones, and
    // the acting scripts carry their own private sender (see `win_dispatch`). The
    // camera/mic consent gate and the save-password capture script, which macOS
    // injects via each feature's `install` (a Windows no-op), ride the builder
    // here instead. The gate applies on every tab; capture skips private ones.
    #[cfg(windows)]
    if canvas_tab {
        builder = builder.scroll_bar_style(tauri::webview::ScrollBarStyle::FluentOverlay);
        builder = builder.initialization_script(crate::canvas::shim_script(&label_ident));
        let keybindings = state.browser.lock().await.settings.keybindings.clone();
        let effective = wis_core::effective_keymap(&keybindings);
        builder = builder.initialization_script(crate::win_dispatch::private_script(
            &token,
            &crate::keybind::page_script(&effective),
            &["__wisKeybindUpdate"],
        ));
    } else {
        // Fluent overlay scrollbars. WebView2 only.
        builder = builder.scroll_bar_style(tauri::webview::ScrollBarStyle::FluentOverlay);
        builder = builder
            .initialization_script(crate::win_dispatch::SHIM_SCRIPT)
            .initialization_script(crate::permissions::gate_script())
            .initialization_script(crate::video_popout::pip_flag_script());
        if !ephemeral {
            builder = builder.initialization_script(crate::win_dispatch::private_script(
                &token,
                crate::capture::capture_script(),
                &[],
            ));
        }
        // Cosmetic element-hiding page script (macOS injects it via
        // `cosmetic::install`); Permissive workspaces opt out of blocking.
        if nav_privacy_mode != wis_core::PrivacyMode::Permissive {
            builder = builder.initialization_script(crate::cosmetic::page_script());
        }
        // Find-in-page engine (CSS Custom Highlight reimplementation; macOS uses
        // the native WebKit find SPI instead).
        builder = builder.initialization_script(crate::find::engine_script());
        // Navigation-guard click interceptor (macOS injects it via
        // `navguard::install`); applies on every tab like macOS.
        builder = builder.initialization_script(crate::win_dispatch::private_script(
            &token,
            crate::navguard::page_script(),
            &[],
        ));
        // Page-side keyboard shortcuts.
        {
            let keybindings = state.browser.lock().await.settings.keybindings.clone();
            let effective = wis_core::effective_keymap(&keybindings);
            builder = builder.initialization_script(crate::win_dispatch::private_script(
                &token,
                &crate::keybind::page_script(&effective),
                &["__wisKeybindUpdate"],
            ));
        }
    }

    // Linux (WebKitGTK): inject the page-world scripts for the channels
    // `gtk_dispatch` receives; the rest it installs in Wis's own world.
    #[cfg(target_os = "linux")]
    if !canvas_tab {
        builder = builder.initialization_script(crate::permissions::gate_script());

        // Cosmetic element-hiding page script; Permissive workspaces opt out.
        if nav_privacy_mode != wis_core::PrivacyMode::Permissive {
            builder = builder.initialization_script(crate::cosmetic::page_script());
        }
    }

    #[cfg(target_os = "linux")]
    let keybind_script = {
        let keybindings = state.browser.lock().await.settings.keybindings.clone();
        crate::keybind::page_script(&wis_core::effective_keymap(&keybindings))
    };

    // The single navigation hook carries two concerns: custom-scheme
    // navigations are cancelled on every workspace and offered as an app
    // handoff, and the per-page scriptlet swap (uBO `+js` rules) runs before
    // each main-frame navigation commits, except on Permissive workspaces.
    {
        let app_for_nav = app.clone();
        let label_for_nav = label_ident.clone();
        let scriptlets = nav_privacy_mode != wis_core::PrivacyMode::Permissive;
        builder = builder.on_navigation(move |url| {
            if !crate::canvas::navigation_allowed(canvas_tab, url) {
                return false;
            }
            if canvas_tab {
                return true;
            }

            if crate::app_launch::should_intercept(url) {
                crate::app_launch::intake_from_tab(&app_for_nav, tab_id, url.clone());
                return false;
            }

            if scriptlets {
                crate::cosmetic::swap_scriptlets(&app_for_nav, &label_for_nav, url.as_str());
            }

            true
        });
    }
    if let Some(ua) = &user_agent {
        builder = builder.user_agent(ua);
    }
    #[cfg(target_os = "macos")]
    let mut extended = false;
    if canvas_tab {
        #[cfg(target_os = "macos")]
        {
            builder = crate::canvas_webview::with_canvas_configuration(&app, builder).await?;
            builder = builder.transparent(true);
        }
        // One store for every canvas tab.
        #[cfg(target_os = "linux")]
        {
            builder = builder.data_directory(canvas_webview_data_dir(&app));
        }
        #[cfg(windows)]
        {
            builder = builder.incognito(true).data_directory(if ephemeral {
                ephemeral_webview_data_dir(&app, workspace_id)
            } else {
                workspace_webview_data_dir(&app, workspace_id)
            });
        }
    } else if ephemeral {
        #[cfg(target_os = "macos")]
        {
            builder =
                crate::private_store::with_workspace_store(&app, workspace_id, builder).await?;
        }

        #[cfg(not(target_os = "macos"))]
        {
            builder = builder.incognito(true);
        }

        #[cfg(windows)]
        {
            builder = builder.data_directory(ephemeral_webview_data_dir(&app, workspace_id));
        }
    } else {
        builder = builder.data_store_identifier(workspace_id_to_data_store_bytes(workspace_id));
        // WKWebView honours `data_store_identifier`; WebView2 and WebKitGTK
        // ignore it and key cookie/storage isolation off the user-data folder
        // instead, so each persistent workspace needs its own directory or they
        // all collapse into one profile (and one shared cookie jar).
        #[cfg(any(windows, target_os = "linux"))]
        {
            builder = builder.data_directory(workspace_webview_data_dir(&app, workspace_id));
        }
    }

    // Document title changes: fires when the page sets `<title>` or
    // mutates `document.title` from JS.
    {
        let app = app.clone();
        builder = builder.on_document_title_changed(move |webview, title: String| {
            let nav = crate::swipe_nav::push_nav_state(&webview);
            let app = app.clone();
            let live_url = webview.url().ok();
            tauri::async_runtime::spawn(async move {
                let state = app.state::<AppState>();
                let result = mutate_unit(&app, &state, |s| {
                    s.with_tab_title(tab_id, title.clone())
                        .and_then(|next| match nav {
                            Some((back, fwd)) => next.with_tab_nav_state(tab_id, back, fwd),
                            None => Ok(next),
                        })
                })
                .await;
                if let Err(err) = result {
                    log::warn!("title update for {tab_id} failed: {err}");
                }
                if let Some(url) = live_url {
                    note_in_page_navigation(&app, tab_id, workspace_id, url, title).await;
                }
            });
        });
    }

    {
        let app = app.clone();
        let label_for_load = label_ident.clone();
        builder = builder.on_page_load(move |webview, payload: PageLoadPayload<'_>| {
            let event = payload.event();
            let loading = matches!(event, PageLoadEvent::Started);
            let url_string = crate::canvas::stored_url(payload.url().clone()).to_string();
            let page_url = payload.url().clone();

            #[cfg(target_os = "macos")]
            let ns_failure: Option<NsPageLoadFailure> = if matches!(event, PageLoadEvent::Finished)
            {
                wry::take_last_page_load_error()
            } else {
                None
            };

            #[cfg(not(target_os = "macos"))]
            let ns_failure: Option<NsPageLoadFailure> = None;

            let load_failed = ns_failure.is_some();
            let policy_interrupted = ns_failure.as_ref().is_some_and(|(code, domain, _, _)| {
                *code == FRAME_LOAD_INTERRUPTED && domain == "WebKitErrorDomain"
            });
            let shown = policy_interrupted
                .then(|| {
                    webview
                        .url()
                        .ok()
                        .filter(|url| url.as_str() != "about:blank")
                })
                .flatten()
                .map(|url| (url.to_string(), read_document_title(&webview)));

            // A popped-out tab navigating loses its pinned video.
            if matches!(event, PageLoadEvent::Started) {
                crate::signup::forget_field(&tab_id.as_uuid().to_string());
                if let Err(err) = crate::find::clear_webview(&webview) {
                    log::debug!("clearing find on navigation failed: {err}");
                }
                #[cfg(target_os = "linux")]
                crate::autoplay::apply_for_page_load(&app, &webview, payload.url().as_str());
                crate::commands::apply_zoom_to_webview(
                    &webview,
                    wis_core::zoom_factor(
                        app.state::<AppState>()
                            .zoom_percent_for_url(payload.url().as_str()),
                    ),
                );
                crate::video_popout::handle_navigation(&app, &label_for_load);
                crate::load_progress::start(&app, tab_id);
            }

            let nav = crate::swipe_nav::push_nav_state(&webview);

            let app_for_state = app.clone();
            tauri::async_runtime::spawn(async move {
                let state = app_for_state.state::<AppState>();
                let result = mutate_unit(&app_for_state, &state, |s| {
                    if policy_interrupted {
                        return match &shown {
                            Some((url, title)) => {
                                let next = s
                                    .with_tab_loading(tab_id, false)?
                                    .with_tab_url_synced(tab_id, url)?;
                                match title {
                                    Some(title) => next.with_tab_title(tab_id, title.clone()),
                                    None => Ok(next),
                                }
                            }
                            None => with_blank_tab_dropped(s, tab_id),
                        };
                    }

                    let failure =
                        ns_failure
                            .as_ref()
                            .and_then(|(code, domain, _cert, failing_url)| {
                                let url = match failing_url {
                                    Some(url) => url.clone(),
                                    None => s.tab(tab_id).ok()?.url.to_string(),
                                };
                                wis_core::LoadError::from_ns_error(domain, *code, url)
                            });

                    let next = s.with_tab_loading(tab_id, loading)?;
                    let next = match (&ns_failure, &failure) {
                        (None, _) => next.with_tab_url_synced(tab_id, &url_string)?,
                        (Some(_), Some(err)) => next.with_tab_url_synced(tab_id, &err.url)?,
                        (Some(_), None) => next,
                    };
                    let next = if loading {
                        next.with_tab_reader(tab_id, false)?
                    } else {
                        next
                    };
                    // Clear on load start; set on a failed finish.
                    let next = if loading {
                        next.with_tab_load_error(tab_id, None)?
                    } else if let Some(err) = failure {
                        next.with_tab_load_error(tab_id, Some(err))?
                    } else {
                        next
                    };

                    match nav {
                        Some((back, fwd)) => next.with_tab_nav_state(tab_id, back, fwd),
                        None => Ok(next),
                    }
                })
                .await;

                if let Err(err) = result {
                    log::warn!("page load update for {tab_id} failed: {err}");
                }
                if policy_interrupted && shown.is_none() {
                    crate::audio::forget_tab(&app_for_state, tab_id);
                    crate::tab_preview::forget(&app_for_state, &state, tab_id);
                }
            });

            // Fallback safety guard for navigations not caught by the in-page
            // click interceptor (address bar, JS redirects).
            if matches!(event, PageLoadEvent::Started) {
                let app_for_guard = app.clone();
                let guard_url = page_url.clone();
                tauri::async_runtime::spawn(async move {
                    crate::navguard::maybe_guard(app_for_guard, tab_id, guard_url).await;
                });
            }

            if matches!(event, PageLoadEvent::Finished) {
                crate::webview_prefs::keep_inactive_selection(&webview);
                if page_url.as_str() != "about:blank" {
                    crate::content_world::cover_first_document(&webview);
                }
            }

            if matches!(event, PageLoadEvent::Finished) && !load_failed {
                // Re-apply the live swipe-nav colour.
                let color = app
                    .state::<AppState>()
                    .swipe_accent
                    .lock()
                    .expect("swipe_accent poisoned")
                    .clone();
                let _ = webview.eval(crate::swipe_nav::accent_script(&color));

                let app_for_favicon = app.clone();
                let page_url_for_history = page_url.clone();
                tauri::async_runtime::spawn(async move {
                    crate::favicons::capture_favicon(
                        app_for_favicon,
                        tab_id,
                        workspace_id,
                        page_url,
                    )
                    .await;
                });

                let app_for_history = app.clone();
                tauri::async_runtime::spawn(async move {
                    record_history_visit(
                        &app_for_history,
                        tab_id,
                        workspace_id,
                        &page_url_for_history,
                    )
                    .await;
                });

                // Snapshot session cookies after each load so a fresh login
                // (e.g. the redirect that completes an SSO sign-in) is saved
                // promptly, before any quit. Skipped for private workspaces.
                if !ephemeral {
                    let app_for_cookies = app.clone();
                    tauri::async_runtime::spawn(async move {
                        crate::cookies::persist_session_cookies(&app_for_cookies, workspace_id)
                            .await;
                    });

                    // Capture this tab's serialized session (back/forward
                    // history + scroll) to disk, debounced, so it restores
                    // across a full restart. Private tabs never persist.
                    app.state::<AppState>()
                        .tab_session_saver
                        .schedule(app.clone(), tab_id);
                }
            }
        });
    }

    // Engine new-window requests. A scripted popup that asks for geometry
    // ("Continue with Google" and other OAuth consent windows) stays a native
    // popup, whose shared configuration preserves the `window.opener` link the
    // flow needs, decorated with a read-only address bar. A plain navigation
    // (`target=_blank`, featureless `window.open`) to the opener's own site
    // replaces the current tab; a cross-site one opens a foreground tab in the
    // focused window.
    builder = builder.on_new_window({
        let app = app.clone();
        let opener_label = label_ident.clone();
        move |url, features| {
            if crate::canvas::is_canvas_url(&url) {
                return tauri::webview::NewWindowResponse::Deny;
            }
            if crate::app_launch::should_intercept(&url) {
                crate::app_launch::intake_from_tab(&app, tab_id, url.clone());
                return tauri::webview::NewWindowResponse::Deny;
            }

            if crate::popup_chrome::wants_popup(&url, &features) {
                crate::popup_chrome::ensure_running(&app, url.as_str());
                return tauri::webview::NewWindowResponse::Allow;
            }

            if let Some(wv) = app.webviews().get(&opener_label) {
                let in_place = wv
                    .url()
                    .ok()
                    .is_some_and(|cur| crate::popup_chrome::same_site(&cur, &url));
                if in_place {
                    if let Err(err) = wv.navigate(url) {
                        log::warn!("same-site new-window navigate failed: {err}");
                    }
                    return tauri::webview::NewWindowResponse::Deny;
                }
            }

            let _ = tauri::Emitter::emit(
                &app,
                crate::link_open::OPEN_TAB_FOREGROUND_EVENT,
                url.to_string(),
            );

            tauri::webview::NewWindowResponse::Deny
        }
    });

    // Downloads land in ~/Downloads with sanitised names; private-workspace
    // downloads are flagged so they never reach persistent history.
    builder = crate::downloads::attach_download_handler(builder, ephemeral);

    // Restore this workspace's saved session cookies into its data store before
    // the first page loads, so logins survive a restart. Once per workspace per run;
    // never for ephemeral (private) workspaces or canvas tabs.
    if !ephemeral && !canvas_tab {
        let first = state.restored_cookies.lock().await.insert(workspace_id);
        if first {
            crate::cookies::restore_session_cookies(&app, workspace_id).await;
        }
    }

    // After the cookies, so an extension's first requests carry the session.
    #[cfg(target_os = "macos")]
    if !ephemeral && !canvas_tab {
        (builder, extended) =
            crate::extensions::with_extensions(&app, &state, workspace_id, proxy_port, builder)
                .await?;
    }

    if canvas_tab {
        crate::canvas::register(
            &label_ident,
            crate::canvas_store::CanvasTab {
                workspace: workspace_id,
                ephemeral,
            },
        );
        let script = crate::canvas::page_script(&state.browser.lock().await.settings);
        builder = builder.initialization_script(script);
        #[cfg(target_os = "linux")]
        {
            builder = builder.initialization_script(crate::canvas::token_script(&label_ident));
        }
    }
    let child = window
        .add_child(
            builder,
            tauri::LogicalPosition::new(x, y),
            tauri::LogicalSize::new(width, height),
        )
        .map_err(|err| {
            crate::canvas::forget(&app, &label_ident);
            WisError::InvalidInput {
                message: format!("add_child failed: {err}"),
            }
        })?;

    crate::pane_container::wrap(&child);

    #[cfg(target_os = "macos")]
    if extended {
        crate::extensions::tab_opened(&child, workspace_id);
    }

    // Restore stacking above the freshly added tab webview:
    // tabs < widget panels < overlays.
    crate::widget_panel::promote_panels_to_front(&app, &window_label);
    crate::overlay::promote_overlays_to_front(&app, &window_label);

    // Stamp the native WKWebView so the pane-focus mouse monitor can identify
    // which tab a click landed in (see `pane_focus`).
    #[cfg(target_os = "macos")]
    {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        let ident = label_ident.clone();
        let _ = child.with_webview(move |w| unsafe {
            let wk: *mut AnyObject = w.inner() as *mut AnyObject;
            if wk.is_null() {
                return;
            }
            let Ok(c) = std::ffi::CString::new(ident.as_str()) else {
                return;
            };
            let ns: *mut AnyObject =
                msg_send![objc2::class!(NSString), stringWithUTF8String: c.as_ptr()];
            let _: () = msg_send![wk, setAccessibilityIdentifier: ns];
        });
    }

    #[cfg(not(target_os = "macos"))]
    let _ = label_ident;

    let privacy_mode = {
        let browser = state.browser.lock().await;
        browser
            .workspace(workspace_id)
            .map(|w| w.privacy_mode)
            .unwrap_or_default()
    };

    // Engine-level filtering is best-effort: a failure must not block the tab.
    if let Some(spec) = state.content_rule_spec(privacy_mode) {
        if let Err(err) = crate::content_filter::attach_rule_list(&child, &spec) {
            log::warn!("content filter: attach skipped: {err}");
        }
    }

    // Apple-proxied Safe Browsing warning.
    crate::webview_prefs::apply_fraud_warning(&child, fraud_warning);

    // MSE + EME/FairPlay (adaptive streaming + DRM key system).
    crate::webview_prefs::apply_media_extensions(&child, media_extensions);

    // Autoplay: macOS applies per navigation through the wry hook.
    #[cfg(target_os = "linux")]
    crate::autoplay::apply(&child, state.autoplay_policy_for(parsed.as_str()));

    // A muted tab stays muted across sleep/wake webview recreation.
    {
        let muted = state
            .muted_tabs
            .lock()
            .expect("muted_tabs poisoned")
            .contains(&tab_id.as_uuid().to_string());
        if muted {
            crate::audio::apply_page_muted(&child, true);
        }
    }

    // Wake-from-sleep: restore the serialized session (history, scroll,
    // forms). The restore performs its own navigation from about:blank.
    if let Some(state_b64) = &restore_state {
        crate::interaction_state::apply(&child, state_b64);

        let fallback_webview = child.clone();
        let fallback_url = parsed.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            let stranded = matches!(fallback_webview.url(), Ok(u) if u.as_str() == "about:blank");
            if stranded {
                if let Err(err) = fallback_webview.navigate(fallback_url) {
                    log::warn!("tab session restore fallback navigation failed: {err}");
                }
            }
        });
    }

    // "Save password?" capture.
    if !ephemeral {
        crate::capture::install(&child);
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    if canvas_tab {
        crate::canvas::install(&child, label_ident.clone());
    }

    // Opt+click "Peek" trigger (safe on all tabs; forwards only the link).
    crate::peek::install(&child);

    // AI page-text channel (inert until the chrome requests extraction).
    crate::page_text::install(&child);
    crate::page_source::install(&child);
    crate::signup::install(&child);

    // Right-click: report the target so the chrome can draw its own menu.
    crate::page_menu::install(&child);

    // Cmd/middle-click -> open link in a new background tab.
    crate::link_open::install(&child);

    // Native fallback for page-first editing keys (undo/redo/select-all).
    crate::edit_keys::install(&child);

    // Page-first shortcuts.
    #[cfg(target_os = "macos")]
    {
        let keybindings = state.browser.lock().await.settings.keybindings.clone();
        crate::keybind::install(&child, &wis_core::effective_keymap(&keybindings));
    }

    // Video popout channel (playing-state tracking + floating mini player).
    crate::video_popout::install(&child);

    // Per-site camera/microphone consent gate (prompts before getUserMedia).
    // Canvas tabs carry neither it nor cosmetic filtering.
    if !canvas_tab {
        crate::permissions::install(&child);
    }

    // Cosmetic filtering (element hiding). Permissive workspaces opt out of
    // blocking entirely, so they skip the channel too.
    if privacy_mode != wis_core::PrivacyMode::Permissive && !canvas_tab {
        crate::cosmetic::install(&child);
        // Seed the document-start scriptlet for the first URL now, before the
        // initial page's scripts run.
        crate::cosmetic::swap_scriptlets(&app, &label_ident, parsed.as_str());
    }

    // Plain-click navigation safety interstitial for risky links.
    crate::navguard::install(&child);

    // Windows: one receiver multiplexes every page->chrome channel registered
    // above (those per-feature installs are macOS no-ops there). `ephemeral`
    // gates the save-password channel off and the privacy flag gates cosmetic/zap,
    // matching macOS not installing those handlers in those cases.
    #[cfg(windows)]
    crate::win_dispatch::install(
        &child,
        ephemeral,
        privacy_mode != wis_core::PrivacyMode::Permissive && !canvas_tab,
        canvas_tab,
    );

    // Linux: register the same page->chrome channels natively on the WebKitGTK
    // UserContentManager.
    #[cfg(target_os = "linux")]
    crate::gtk_dispatch::install(
        &child,
        ephemeral,
        privacy_mode != wis_core::PrivacyMode::Permissive && !canvas_tab,
        canvas_tab,
        keybind_script,
    );

    // Windows/Linux report page-load failures through their own native
    // callbacks.
    #[cfg(any(windows, target_os = "linux"))]
    crate::load_failure::install(&child, tab_id);

    // Windows click-to-focus is per-controller (no app-wide mouse monitor like
    // macOS); a no-op elsewhere.
    crate::pane_focus::install_controller(&child);

    // Zoom belongs to the site, so a restored or woken tab takes the zoom of
    // the page it shows rather than whatever this tab was last set to. Also
    // force the page's `prefers-color-scheme` to match the browser theme.
    let theme = {
        let browser = state.browser.lock().await;
        if canvas_tab {
            crate::canvas::appearance(browser.settings.canvas_theme)
        } else {
            browser.settings.theme
        }
    };
    let zoom = wis_core::zoom_factor(state.zoom_percent_for_url(parsed.as_str()));
    if (zoom - 1.0).abs() > f64::EPSILON {
        apply_tab_zoom(&app, tab_id, zoom);
    }
    apply_tab_appearance(&child, theme);

    Ok(())
}

pub(crate) fn workspace_id_to_data_store_bytes(workspace_id: WorkspaceId) -> [u8; 16] {
    *workspace_id.as_uuid().as_bytes()
}

/// The store every canvas tab shares on Linux.
#[cfg(target_os = "linux")]
fn canvas_webview_data_dir(app: &AppHandle) -> std::path::PathBuf {
    app.path()
        .app_local_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("app.myk.wis"))
        .join("webviews")
        .join("canvas")
}

/// The engine user-data folder for a persistent workspace (Windows WebView2 /
/// Linux WebKitGTK, both of which isolate by folder). Derived from the
/// workspace uuid so it is stable across restarts (cookies/storage persist);
/// the engine creates the folder if it is missing. The macOS analogue is
/// [`workspace_id_to_data_store_bytes`] via `data_store_identifier`.
#[cfg(any(windows, target_os = "linux"))]
pub(crate) fn workspace_webview_data_dir(
    app: &AppHandle,
    workspace_id: WorkspaceId,
) -> std::path::PathBuf {
    let base = app
        .path()
        .app_local_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("app.myk.wis"));
    base.join("webviews")
        .join(workspace_id.as_uuid().to_string())
}

/// The WebView2 user-data folder for an ephemeral (private) workspace's
/// webviews (Windows). InPrivate controllers keep browsing data out of it, but
/// WebView2 still needs a dedicated folder so the environment's browser
/// arguments (`--proxy-server`) never collide with the default folder's
/// already-running browser process. Ephemeral workspace uuids are minted per
/// session, so [`sweep_ephemeral_webview_data`] clears stale trees at startup.
#[cfg(windows)]
pub(crate) fn ephemeral_webview_data_dir(
    app: &AppHandle,
    workspace_id: WorkspaceId,
) -> std::path::PathBuf {
    let base = app
        .path()
        .app_local_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("app.myk.wis"));
    base.join("webviews-ephemeral")
        .join(workspace_id.as_uuid().to_string())
}

/// Best-effort removal of ephemeral webview data folders left by previous
/// runs (a crash, or WebView2 still flushing at quit). Runs off the setup
/// path; failures only log.
#[cfg(windows)]
pub(crate) fn sweep_ephemeral_webview_data(app: &AppHandle) {
    let Ok(base) = app.path().app_local_data_dir() else {
        return;
    };
    let dir = base.join("webviews-ephemeral");
    tauri::async_runtime::spawn_blocking(move || {
        if dir.exists() {
            if let Err(err) = std::fs::remove_dir_all(&dir) {
                log::warn!("sweep ephemeral webview data: {err}");
            }
        }
    });
}

/// Destroy a tab's native webview, closing its WebKit page first so the
/// content process exits (see `webview_close`); the JS `Webview.close` alone
/// leaks it on macOS. Tab labels only: widget panels and peek tear down
/// through their own commands. A missing webview is a silent no-op.
#[tauri::command]
pub async fn close_tab_webview(app: AppHandle, label: String) -> WisResult<()> {
    if !label.starts_with("tab-") {
        return Err(WisError::InvalidInput {
            message: "close_tab_webview expects a tab webview label".into(),
        });
    }

    let Some(webview) = app.webviews().get(&label).cloned() else {
        return Ok(());
    };

    crate::webview_close::terminate_page(&webview);
    crate::content_world::forget(&label);
    crate::canvas::forget(&app, &label);
    #[cfg(windows)]
    crate::win_dispatch::forget(&label);
    webview.close().map_err(|err| WisError::InvalidInput {
        message: format!("close webview failed: {err}"),
    })?;

    Ok(())
}

/// Set a tab webview's position and size in a single IPC round-trip, applying
/// both atomically. Resolving the webview is best-effort: a not-yet-created webview is
/// a silent no-op.
#[tauri::command]
pub async fn set_tab_webview_bounds(
    app: AppHandle,
    label: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> WisResult<()> {
    let Some(webview) = app.webviews().get(&label).cloned() else {
        return Ok(());
    };
    if crate::pane_container::set_bounds(&webview, x, y, width, height) {
        return Ok(());
    }

    webview
        .set_position(tauri::LogicalPosition::new(x, y))
        .map_err(|err| WisError::InvalidInput {
            message: format!("set webview position failed: {err}"),
        })?;
    webview
        .set_size(tauri::LogicalSize::new(width, height))
        .map_err(|err| WisError::InvalidInput {
            message: format!("set webview size failed: {err}"),
        })?;

    #[cfg(windows)]
    crate::win_webview2::reapply_corner_radius(&webview, (width, height));

    Ok(())
}

/// Read the current URL of a tab's native webview. Returns `None` if the
/// webview hasn't been created yet (chrome tab or not yet reconciled).
#[tauri::command]
pub async fn webview_url(app: AppHandle, label: String) -> WisResult<Option<String>> {
    let Some(webview) = app.webviews().get(&label).cloned() else {
        return Ok(None);
    };

    match webview.url() {
        Ok(url) => Ok(Some(url.to_string())),
        Err(err) => Err(WisError::InvalidInput {
            message: format!("read webview url failed: {err}"),
        }),
    }
}

/// How far a canvas tab reaches past its pane, and its corner radius, so the
/// page can paint its panel inside the pane. Ignored for other tabs.
#[tauri::command]
pub async fn set_canvas_ground(
    app: AppHandle,
    label: String,
    bottom: f64,
    radius: f64,
) -> WisResult<()> {
    crate::canvas::set_ground(&app, &label, bottom, radius);
    Ok(())
}

/// Apply a CSS-style corner radius to a tab's native webview surface
/// (macOS CALayer / Windows WebView2 region; no-op elsewhere). The chrome
/// derives the radius from the Corners design setting via `--radius-scale`.
#[tauri::command]
pub async fn set_webview_corner_radius(
    app: AppHandle,
    label: String,
    radius: f64,
) -> WisResult<()> {
    let webview = app
        .webviews()
        .get(&label)
        .cloned()
        .ok_or_else(|| WisError::InvalidInput {
            message: format!("no webview labelled {label}"),
        })?;

    #[cfg(target_os = "macos")]
    if !crate::pane_container::set_corner_radius(&webview, radius) {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;

        webview
            .with_webview(move |w| unsafe {
                let wkwebview: *mut AnyObject = w.inner() as *mut AnyObject;
                if wkwebview.is_null() {
                    return;
                }

                // Force the view to be layer-backed.
                let _: () = msg_send![wkwebview, setWantsLayer: true];

                let layer: *mut AnyObject = msg_send![wkwebview, layer];
                if layer.is_null() {
                    return;
                }

                // Round + clip.
                let _: () = msg_send![layer, setCornerRadius: radius];
                let _: () = msg_send![layer, setMasksToBounds: true];
            })
            .map_err(|err| WisError::InvalidInput {
                message: format!("with_webview failed: {err}"),
            })?;
    }

    #[cfg(windows)]
    crate::win_webview2::set_corner_radius(&webview, radius);

    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = webview;
        let _ = radius;
    }

    Ok(())
}

/// `WebKitErrorFrameLoadInterruptedByPolicyChange`: the engine abandoned a
/// provisional navigation on a policy decision (it became a download, or was
/// cancelled), so nothing commits and the tab keeps showing its current page.
const FRAME_LOAD_INTERRUPTED: i64 = 102;

/// The webview's live document title (macOS; `None` elsewhere or when empty).
fn read_document_title<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;

        let (tx, rx) = std::sync::mpsc::sync_channel::<String>(1);
        let _ = webview.with_webview(move |w| unsafe {
            let wk: *mut AnyObject = w.inner() as *mut AnyObject;
            if wk.is_null() {
                return;
            }

            let title: *mut AnyObject = msg_send![wk, title];
            if title.is_null() {
                return;
            }

            let utf8: *const std::os::raw::c_char = msg_send![title, UTF8String];
            if utf8.is_null() {
                return;
            }

            let _ = tx.send(
                std::ffi::CStr::from_ptr(utf8)
                    .to_string_lossy()
                    .into_owned(),
            );
        });

        rx.recv_timeout(std::time::Duration::from_millis(100))
            .ok()
            .filter(|title| !title.is_empty())
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = webview;
        None
    }
}

/// Drop a tab whose only navigation was abandoned before anything rendered:
/// closed when the workspace has other tabs, else reset to the new-tab page.
fn with_blank_tab_dropped(
    s: &wis_core::BrowserState,
    tab_id: TabId,
) -> WisResult<wis_core::BrowserState> {
    let workspace_id = s.tab(tab_id)?.workspace_id;
    if s.workspace(workspace_id)?.tab_order.len() > 1 {
        s.with_tab_closed(tab_id)
    } else {
        s.with_navigated_tab(tab_id, "about:blank")
            .map(|(next, _)| next)
    }
}
