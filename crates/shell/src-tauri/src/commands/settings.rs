use super::*;

use wis_network::Resolver;

#[tauri::command]
pub async fn update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    mut patch: UserSettingsPatch,
) -> WisResult<UserSettings> {
    if let Some(themes) = &patch.custom_themes {
        for theme in themes {
            theme.validate()?;
        }
    }

    if let Some(widgets) = &patch.sidebar_widgets {
        for widget in widgets {
            widget.validate()?;
        }
    }

    // Clamp boundary-supplied design values before they reach the
    // pure state transition.
    if let Some(design) = patch.design {
        patch.design = Some(design.sanitized());
    }

    // Normalize the homepage (an empty/invalid value clears it) and sanitize
    // launchpad shortcut tiles (both are user-controlled and navigated to).
    if let Some(raw) = patch.homepage.take() {
        patch.homepage = Some(wis_core::sanitize_homepage(&raw).unwrap_or_default());
    }
    if let Some(tiles) = patch.shortcuts.take() {
        patch.shortcuts = Some(wis_core::sanitize_shortcuts(tiles));
    }

    // Drop invalid custom search engines and enforce keyword uniqueness and caps.
    if let Some(engines) = patch.custom_search_engines.take() {
        patch.custom_search_engines = Some(wis_core::sanitize_search_engines(engines));
    }
    if let Some(map) = patch.builtin_search_keywords.take() {
        patch.builtin_search_keywords = Some(wis_core::sanitize_builtin_search_keywords(map));
    }

    if let Some(percent) = patch.default_zoom_percent {
        patch.default_zoom_percent = Some(wis_core::sane_zoom_percent(percent));
    }
    if let Some(map) = patch.zoom_site_overrides.take() {
        patch.zoom_site_overrides = Some(
            map.into_iter()
                .map(|(host, percent)| {
                    (
                        wis_core::user_agent::normalize_host(&host),
                        wis_core::sane_zoom_percent(percent),
                    )
                })
                .collect(),
        );
    }

    if let Some(map) = patch.autoplay_site_overrides.take() {
        patch.autoplay_site_overrides = Some(
            map.into_iter()
                .map(|(host, policy)| (wis_core::user_agent::normalize_host(&host), policy))
                .collect(),
        );
    }

    if let Some(map) = patch.site_protection_overrides.take() {
        patch.site_protection_overrides =
            Some(wis_core::site_protection::sanitized_site_protection(map));
    }

    let keybindings_touched = patch.keybindings.is_some();
    let userscripts_touched = patch.userscripts.is_some();
    let cosmetic_touched = patch.user_cosmetic_rules.is_some();
    let media_permissions_touched = patch.media_permissions.is_some();
    let app_launch_permissions_touched = patch.app_launch_permissions.is_some();
    let theme_touched = patch.theme.is_some();
    let persistence_disabled = patch.persist_sessions == Some(false);
    let fraud_warning_changed = patch.fraud_warning_enabled;
    let gpc_changed = patch.global_privacy_control;
    let zoom_touched = patch.default_zoom_percent.is_some() || patch.zoom_site_overrides.is_some();
    let autoplay_touched =
        patch.autoplay_policy.is_some() || patch.autoplay_site_overrides.is_some();
    let site_protection_touched = patch.site_protection_overrides.is_some();
    let media_extensions_changed = patch.media_extensions_enabled;
    let canvas_page_touched = patch.canvas_theme.is_some()
        || patch.layout.is_some()
        || patch.top_tabs_rail_side.is_some();
    let (settings, doh_changed, notice_list_changed) = mutate_immediate(&app, &state, |s| {
        let before = crate::state::doh_provider_of(s.settings.doh_resolver.as_deref());
        let notice_list_before = s.settings.notice_list();
        let next = s.with_settings(patch);
        let settings = next.settings.clone();
        let changed = crate::state::doh_provider_of(settings.doh_resolver.as_deref()) != before;
        let notice_list_changed = settings.notice_list() != notice_list_before;

        Ok((next, (settings, changed, notice_list_changed)))
    })
    .await?;

    if notice_list_changed {
        super::apply_notice_list(app.clone(), settings.notice_list());
    }

    if doh_changed {
        state.resolver.set(crate::state::build_resolver(
            settings.doh_resolver.as_deref(),
        ));
    }

    if keybindings_touched {
        let effective = wis_core::effective_keymap(&settings.keybindings);
        crate::keybind::apply_to_all_tabs(&app, &effective);
        let app_for_menu = app.clone();
        if let Err(err) = app.run_on_main_thread(move || {
            if let Err(err) = crate::menu::apply_menu(&app_for_menu, &effective) {
                log::warn!("menu rebuild after keybindings change failed: {err}");
            }
        }) {
            log::warn!("dispatching menu rebuild to the main thread failed: {err}");
        }
    }

    if canvas_page_touched {
        crate::canvas::push_page(&app, &settings);
    }

    if userscripts_touched {
        state.refresh_userscripts_cache(settings.userscripts.clone());
    }
    if cosmetic_touched {
        state.refresh_user_cosmetic_cache(settings.user_cosmetic_rules.clone());
    }
    if media_permissions_touched {
        state.refresh_media_permissions_cache(settings.media_permissions.clone());
    }
    if app_launch_permissions_touched {
        state.refresh_app_launch_permissions_cache(settings.app_launch_permissions.clone());
    }

    if theme_touched {
        apply_appearance_to_all_tabs(&app, settings.theme);
    }

    if let Some(enabled) = fraud_warning_changed {
        crate::webview_prefs::apply_fraud_warning_to_all_remote(&app, enabled);
    }

    if let Some(enabled) = gpc_changed {
        state.refresh_all_proxy_policies().await;
        crate::gpc::apply_to_all_remote(&app, enabled);
    }

    if zoom_touched {
        state.refresh_zoom_cache().await;
        crate::zoom::apply_to_all_tabs(&app);
    }

    if autoplay_touched {
        state.refresh_autoplay_cache(crate::autoplay::AutoplayCache {
            policy: settings.autoplay_policy,
            overrides: settings.autoplay_site_overrides.clone(),
        });

        #[cfg(target_os = "linux")]
        crate::autoplay::apply_to_all_remote(&app);
    }

    if site_protection_touched {
        state.refresh_site_protection_cache().await;
    }

    if let Some(enabled) = media_extensions_changed {
        crate::webview_prefs::apply_media_extensions_to_all_remote(&app, enabled);
    }

    // Keep the sync download-prefs mirror fresh for wry's synchronous
    // download callback.
    {
        let mut prefs = state
            .download_prefs
            .lock()
            .expect("download_prefs poisoned");
        prefs.ask_where = settings.download_ask_where;
    }

    // Turning off "stay logged in" wipes any already-persisted sessions.
    if persistence_disabled {
        crate::cookies::purge_persisted_sessions(&app, true).await;
    }

    Ok(settings)
}

/// Re-apply the saved keyboard shortcuts to the native menu. The menu is also
/// rebuilt automatically whenever a binding changes; this backs an explicit
/// "Apply" button in Settings -> Shortcuts so the user can force the live menu
/// to pick up their edits on demand.
#[tauri::command]
pub async fn apply_shortcuts(app: AppHandle, state: State<'_, AppState>) -> WisResult<()> {
    let keybindings = state.browser.lock().await.settings.keybindings.clone();
    let effective = wis_core::effective_keymap(&keybindings);
    crate::keybind::apply_to_all_tabs(&app, &effective);
    let app_for_menu = app.clone();

    app.run_on_main_thread(move || {
        if let Err(err) = crate::menu::apply_menu(&app_for_menu, &effective) {
            log::warn!("apply_shortcuts: menu rebuild failed: {err}");
        }
    })
    .map_err(|err| WisError::InvalidInput {
        message: format!("apply_shortcuts dispatch: {err}"),
    })
}

// ---------------------------------------------------------------------------
// Privacy decision engine (read-only inspector)
// ---------------------------------------------------------------------------

/// Ask the privacy engine what it would do with the given request, without
/// actually sending it.
///
/// The active workspace's `PrivacyMode` drives the decision; per-site
/// overrides will be plumbed through once they're modelled in `BrowserState`.
#[tauri::command]
pub async fn check_url_policy(
    state: State<'_, AppState>,
    url: String,
    source_url: String,
    request_type: RequestType,
) -> WisResult<Decision> {
    let parsed_url = Url::parse(&url).map_err(|_| WisError::InvalidUrl { url })?;
    let parsed_source =
        Url::parse(&source_url).map_err(|_| WisError::InvalidUrl { url: source_url })?;

    let policy = state.request_policy_for_active_workspace().await;
    policy
        .decide(&OutboundRequest {
            url: &parsed_url,
            source_url: &parsed_source,
            request_type,
        })
        .map_err(|err| WisError::InvalidInput {
            message: err.to_string(),
        })
}

/// Resolve `hostname` via the configured DoH resolver and return the
/// addresses as strings.
#[tauri::command]
pub async fn resolve_hostname(
    state: State<'_, AppState>,
    hostname: String,
) -> WisResult<Vec<String>> {
    let resolver = state.resolver.clone();
    resolver
        .resolve(&hostname)
        .await
        .map(|ips| ips.into_iter().map(|ip| ip.to_string()).collect())
        .map_err(|err| WisError::InvalidInput {
            message: err.to_string(),
        })
}
