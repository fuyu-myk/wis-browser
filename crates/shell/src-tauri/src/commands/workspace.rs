use super::*;

#[tauri::command]
pub async fn create_workspace(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    accent: AccentColor,
) -> WisResult<Workspace> {
    mutate(&app, &state, |s| Ok(s.with_new_workspace(name, accent))).await
}

/// Create a private (ephemeral) workspace. Its tabs use a non-persistent
/// WebView data store and the workspace is never written to disk.
#[tauri::command]
pub async fn create_private_workspace(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    accent: AccentColor,
) -> WisResult<Workspace> {
    mutate(&app, &state, |s| {
        Ok(s.with_new_private_workspace(name, accent))
    })
    .await
}

#[tauri::command]
pub async fn rename_workspace(
    app: AppHandle,
    state: State<'_, AppState>,
    id: WorkspaceId,
    name: String,
) -> WisResult<Workspace> {
    mutate(&app, &state, |s| {
        let next = s.with_renamed_workspace(id, name)?;
        let renamed = next.workspace(id)?.clone();

        Ok((next, renamed))
    })
    .await
}

#[tauri::command]
pub async fn delete_workspace(
    app: AppHandle,
    state: State<'_, AppState>,
    id: WorkspaceId,
) -> WisResult<()> {
    mutate_unit_immediate(&app, &state, |s| s.with_workspace_removed(id)).await
}

/// Set or clear a workspace's custom flat dot colour (validated hex).
#[tauri::command]
pub async fn set_workspace_custom_accent(
    app: AppHandle,
    state: State<'_, AppState>,
    id: WorkspaceId,
    color: Option<String>,
) -> WisResult<Workspace> {
    mutate(&app, &state, |s| {
        let next = s.with_workspace_custom_accent(id, color.clone())?;
        let updated = next.workspace(id)?.clone();

        Ok((next, updated))
    })
    .await
}

/// Set or clear a workspace's custom background (validated in core before it
/// enters state).
#[tauri::command]
pub async fn set_workspace_background(
    app: AppHandle,
    state: State<'_, AppState>,
    id: WorkspaceId,
    background: Option<wis_core::Background>,
) -> WisResult<Workspace> {
    mutate(&app, &state, |s| {
        let next = s.with_workspace_background(id, background.clone())?;
        let updated = next.workspace(id)?.clone();

        Ok((next, updated))
    })
    .await
}

#[tauri::command]
pub async fn recolor_workspace(
    app: AppHandle,
    state: State<'_, AppState>,
    id: WorkspaceId,
    accent: AccentColor,
) -> WisResult<Workspace> {
    mutate(&app, &state, |s| {
        let next = s.with_recoloured_workspace(id, accent)?;
        let recoloured = next.workspace(id)?.clone();

        Ok((next, recoloured))
    })
    .await
}

/// Persist the primary (and validate `id`, erroring before the map is
/// touched). With one window this is the main window; with secondary windows
/// they will update the per-window map only and leave the persisted
/// primary alone. The emit from `mutate_unit` makes every window refetch its
/// own active workspace.
#[tauri::command]
pub async fn set_active_workspace(
    window: tauri::Window,
    app: AppHandle,
    state: State<'_, AppState>,
    id: WorkspaceId,
) -> WisResult<()> {
    // One workspace per window: if another window already shows this workspace,
    // focus that window.
    if let Some(owner) = state.window_for_workspace(id) {
        if owner != window.label() {
            crate::windows::focus_window(&app, &owner);
            return Ok(());
        }
    }

    mutate_unit(&app, &state, |s| s.with_active_workspace(id)).await?;
    state.set_window_workspace(window.label(), id);
    Ok(())
}

/// Change the privacy mode for a specific workspace. The workspace proxy is
/// dropped so the next navigation rebuilds it with the new policy, and the
/// engine-level rule list on every open tab in the workspace is re-applied.
#[tauri::command]
pub async fn set_workspace_privacy_mode(
    app: AppHandle,
    state: State<'_, AppState>,
    id: WorkspaceId,
    mode: PrivacyMode,
) -> WisResult<Workspace> {
    let workspace = mutate(&app, &state, |s| {
        let next = s.with_workspace_privacy_mode(id, mode)?;
        let updated = next.workspace(id)?.clone();

        Ok((next, updated))
    })
    .await?;

    let _ = state.update_proxy_policy(id, false).await;

    let spec = state.content_rule_spec(mode);
    let tab_labels: Vec<String> = {
        let browser = state.browser.lock().await;
        browser
            .tabs_for(id)
            .map(|tabs| {
                tabs.iter()
                    .map(|tab| format!("tab-{}", tab.id.as_uuid()))
                    .collect()
            })
            .unwrap_or_default()
    };
    let total = tab_labels.len();
    let mut reattached = 0usize;
    for label in tab_labels {
        let Some(webview) = app.webviews().get(&label).cloned() else {
            continue;
        };
        let _ = crate::content_filter::clear_rule_lists(&webview);
        if let Some(spec) = &spec {
            match crate::content_filter::attach_rule_list(&webview, spec) {
                Ok(()) => reattached += 1,
                Err(err) => log::warn!("content filter: re-attach for {label} skipped: {err}"),
            }
        }
    }
    log::info!("workspace {id} privacy mode now {mode:?}: re-attached {reattached}/{total} tabs");

    Ok(workspace)
}

/// Set (or clear) a workspace's upstream proxy. The running proxy's policy is
/// updated in place so open tabs route through the new upstream on their next
/// request without losing their connection.
#[tauri::command]
pub async fn set_workspace_upstream(
    app: AppHandle,
    state: State<'_, AppState>,
    id: WorkspaceId,
    upstream: Option<UpstreamProxy>,
) -> WisResult<Workspace> {
    let upstream = match upstream {
        Some(proxy) => {
            let proxy = proxy.normalized();
            proxy.validate()?;
            Some(proxy)
        }
        None => None,
    };

    state
        .resolve_upstream(upstream.clone())
        .await
        .map_err(WisError::from)?;

    let workspace = mutate(&app, &state, |s| {
        let next = s.with_workspace_upstream_proxy(id, upstream.clone())?;
        let updated = next.workspace(id)?.clone();

        Ok((next, updated))
    })
    .await?;

    state
        .update_proxy_policy(id, true)
        .await
        .map_err(WisError::from)?;

    let tab_labels: Vec<String> = {
        let browser = state.browser.lock().await;
        browser
            .tabs_for(id)
            .map(|tabs| {
                tabs.iter()
                    .filter(|tab| !crate::commands::is_chrome_url(tab.url.as_str()))
                    .map(|tab| format!("tab-{}", tab.id.as_uuid()))
                    .collect()
            })
            .unwrap_or_default()
    };
    for label in tab_labels {
        if let Some(webview) = app.webviews().get(&label).cloned() {
            let _ = webview.eval("window.location.reload()");
        }
    }

    log::info!("workspace {id} upstream proxy updated");

    Ok(workspace)
}

/// Whether a URL is a chrome page (no native webview / not reloadable as a site).
pub(crate) fn is_chrome_url(url: &str) -> bool {
    url.starts_with("about:")
}

/// Probe an upstream proxy for reachability (TCP connect, plus a SOCKS5
/// handshake) so Settings can show a connection status. Does not change any
/// workspace; returns `Ok(())` when reachable, or an error message. Built-in
/// Tor is "probed" by bootstrapping the embedded client.
#[tauri::command]
pub async fn test_upstream(state: State<'_, AppState>, upstream: UpstreamProxy) -> WisResult<()> {
    let upstream = upstream.normalized();
    upstream.validate()?;
    match upstream {
        UpstreamProxy::Tor => {
            // Probe the client's currently listening port.
            let port = state
                .ensure_tor()
                .await
                .map_err(|e| WisError::InvalidInput {
                    message: e.to_string(),
                })?;
            wis_network::probe_upstream(&UpstreamProxy::Socks5 {
                host: "127.0.0.1".to_string(),
                port,
            })
            .await
            .map_err(|reason| WisError::InvalidInput { message: reason })
        }
        other => wis_network::probe_upstream(&other)
            .await
            .map_err(|reason| WisError::InvalidInput { message: reason }),
    }
}

/// Whether the built-in Tor client is compiled into this build.
#[tauri::command]
pub fn tor_available() -> bool {
    cfg!(feature = "tor")
}
