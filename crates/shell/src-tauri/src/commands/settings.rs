use super::*;

#[tauri::command]
pub async fn update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    patch: UserSettingsPatch,
) -> WisResult<UserSettings> {
    if let Some(themes) = &patch.custom_themes {
        for theme in themes {
            theme.validate()?;
        }
    }

    let keybindings_touched = patch.keybindings.is_some();
    let userscripts_touched = patch.userscripts.is_some();
    let cosmetic_touched = patch.user_cosmetic_rules.is_some();
    let media_permissions_touched = patch.media_permissions.is_some();
    let theme_touched = patch.theme.is_some();
    let persistence_disabled = patch.persist_sessions == Some(false);
    let fraud_warning_changed = patch.fraud_warning_enabled;
    let media_extensions_changed = patch.media_extensions_enabled;
    let settings = mutate_immediate(&app, &state, |s| {
        let next = s.with_settings(patch);
        let settings = next.settings.clone();

        Ok((next, settings))
    })
    .await?;

    if keybindings_touched {
        let effective = wis_core::effective_keymap(&settings.keybindings);
        if let Err(err) = crate::menu::apply_menu(&app, &effective) {
            log::warn!("menu rebuild after keybindings change failed: {err}");
        }
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

    if theme_touched {
        apply_appearance_to_all_tabs(&app, settings.theme);
    }

    if let Some(enabled) = fraud_warning_changed {
        crate::webview_prefs::apply_fraud_warning_to_all_remote(&app, enabled);
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
