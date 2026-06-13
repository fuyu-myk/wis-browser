use super::*;

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

    let (ephemeral, user_agent, fraud_warning, media_extensions, nav_privacy_mode) = {
        let browser = state.browser.lock().await;
        let ephemeral = browser
            .workspace(workspace_id)
            .map(|w| w.ephemeral)
            .unwrap_or(false);
        let user_agent = wis_core::ua_for_host(
            parsed.host_str().unwrap_or_default(),
            &browser.settings.ua_site_overrides,
            &browser.settings.user_agent_profile,
            wis_core::DEFAULT_MAC_OS_VERSION,
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
        )
    };

    let proxy_port = state
        .ensure_proxy_for(workspace_id)
        .await
        .map_err(wis_core::WisError::from)?;
    let proxy_url = format!("http://127.0.0.1:{proxy_port}")
        .parse::<Url>()
        .expect("loopback proxy URL is valid");

    let mut builder = WebviewBuilder::new(label, WebviewUrl::External(initial_url))
        .focused(false)
        .proxy_url(proxy_url)
        .accept_first_mouse(true)
        // Rides the webview configuration so even an instantly-loading first
        // document (cache, localhost) gets it.
        .initialization_script(crate::video_popout::page_script());

    // Per-page scriptlet swap (uBO `+js` rules) before each main-frame
    // navigation commits; Permissive workspaces opt out of blocking.
    if nav_privacy_mode != wis_core::PrivacyMode::Permissive {
        let app_for_nav = app.clone();
        let label_for_nav = label_ident.clone();
        builder = builder.on_navigation(move |url| {
            crate::cosmetic::swap_scriptlets(&app_for_nav, &label_for_nav, url.as_str());
            true
        });
    }
    if let Some(ua) = &user_agent {
        builder = builder.user_agent(ua);
    }
    if ephemeral {
        builder = builder.incognito(true);
    } else {
        builder = builder.data_store_identifier(workspace_id_to_data_store_bytes(workspace_id));
    }

    // Document title changes: fires when the page sets `<title>` or
    // mutates `document.title` from JS.
    {
        let app = app.clone();
        builder = builder.on_document_title_changed(move |webview, title: String| {
            let app = app.clone();
            let live_url = webview.url().ok();
            tauri::async_runtime::spawn(async move {
                let state = app.state::<AppState>();
                let result =
                    mutate_unit(&app, &state, |s| s.with_tab_title(tab_id, title.clone())).await;
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
        builder = builder.on_page_load(move |_webview, payload: PageLoadPayload<'_>| {
            let event = payload.event();
            let loading = matches!(event, PageLoadEvent::Started);
            let url_string = payload.url().to_string();
            let page_url = payload.url().clone();

            // A popped-out tab navigating loses its pinned video.
            if matches!(event, PageLoadEvent::Started) {
                crate::video_popout::handle_navigation(&app, &label_for_load);
            }

            let app_for_state = app.clone();
            tauri::async_runtime::spawn(async move {
                let state = app_for_state.state::<AppState>();
                let result = mutate_unit(&app_for_state, &state, |s| {
                    s.with_tab_loading(tab_id, loading)
                        .and_then(|next| next.with_tab_url_synced(tab_id, &url_string))
                })
                .await;
                if let Err(err) = result {
                    log::warn!("page load update for {tab_id} failed: {err}");
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
                }
            }
        });
    }

    // Allow `window.open` / `target=_blank` popups (e.g. "Continue with Google"
    // and other OAuth flows).
    builder = builder.on_new_window(|_url, _features| tauri::webview::NewWindowResponse::Allow);

    // Downloads land in ~/Downloads with sanitised names; private-workspace
    // downloads are flagged so they never reach persistent history.
    builder = crate::downloads::attach_download_handler(builder, ephemeral);

    // Restore this workspace's saved session cookies into its data store before
    // the first page loads, so logins survive a restart. Once per workspace per run;
    // never for ephemeral (private) workspaces.
    if !ephemeral {
        let first = state.restored_cookies.lock().await.insert(workspace_id);
        if first {
            crate::cookies::restore_session_cookies(&app, workspace_id).await;
        }
    }

    let child = window
        .add_child(
            builder,
            tauri::LogicalPosition::new(x, y),
            tauri::LogicalSize::new(width, height),
        )
        .map_err(|err| WisError::InvalidInput {
            message: format!("add_child failed: {err}"),
        })?;

    // Restore stacking above the freshly added tab webview:
    // tabs < widget panels < overlays.
    crate::widget_panel::promote_panels_to_front(&app);
    crate::overlay::promote_overlays_to_front(&app);

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
    }

    // "Save password?" capture.
    if !ephemeral {
        crate::capture::install(&child);
    }

    // Opt+click "Peek" trigger (safe on all tabs; forwards only the link).
    crate::peek::install(&child);

    // Video popout channel (playing-state tracking + floating mini player).
    crate::video_popout::install(&child);

    // Per-site camera/microphone consent gate (prompts before getUserMedia).
    crate::permissions::install(&child);

    // Cosmetic filtering (element hiding). Permissive workspaces opt out of
    // blocking entirely, so they skip the channel too.
    if privacy_mode != wis_core::PrivacyMode::Permissive {
        crate::cosmetic::install(&child);
        // Seed the document-start scriptlet for the first URL now, before the
        // initial page's scripts run.
        crate::cosmetic::swap_scriptlets(&app, &label_ident, parsed.as_str());
    }

    // Plain-click navigation safety interstitial for risky links.
    crate::navguard::install(&child);

    // Restore the tab's persisted page zoom (if it isn't the default), and force
    // the page's `prefers-color-scheme` to match the browser theme.
    let (zoom, theme) = {
        let browser = state.browser.lock().await;
        (
            browser.tab(tab_id).map(|t| t.zoom).unwrap_or(1.0),
            browser.settings.theme,
        )
    };
    if (zoom - 1.0).abs() > f64::EPSILON {
        apply_tab_zoom(&app, tab_id, zoom);
    }
    apply_tab_appearance(&child, theme);

    Ok(())
}

pub(crate) fn workspace_id_to_data_store_bytes(workspace_id: WorkspaceId) -> [u8; 16] {
    *workspace_id.as_uuid().as_bytes()
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

/// Sync a tab's stored URL to match where the user actually navigated inside
/// the webview (in-page link clicks, history hops). Unlike `navigate_tab`
/// Apply a CSS-style corner radius to a tab's native webview surface, macOS only.
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
    {
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

    #[cfg(not(target_os = "macos"))]
    {
        let _ = webview;
        let _ = radius;
    }

    Ok(())
}
