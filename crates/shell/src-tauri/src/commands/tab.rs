use super::*;

#[tauri::command]
pub async fn create_tab(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    url: String,
) -> WisResult<Tab> {
    mutate(&app, &state, |s| s.with_new_tab(workspace_id, &url)).await
}

/// Append a tab that is always loose (outside any folder), ignoring the
/// workspace's selected folder. Backs the explicit "+ New tab" affordance, where
/// the user wants a fresh top-level tab rather than one filed into a group.
#[tauri::command]
pub async fn create_loose_tab(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    url: String,
) -> WisResult<Tab> {
    mutate(&app, &state, |s| s.with_new_loose_tab(workspace_id, &url)).await
}

/// Open `url` in a new tab without switching to it (the opener stays active).
/// Backs Cmd/middle-click "open link in a new background tab".
#[tauri::command]
pub async fn create_background_tab(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    url: String,
) -> WisResult<Tab> {
    mutate(&app, &state, |s| {
        s.with_new_background_tab(workspace_id, &url)
    })
    .await
}

/// Open a link a page handed to the chrome as a focused tab beside its opener
/// (the active tab). Backs `target=_blank` / `window.open` plain navigations and
/// widget-panel cross-site clicks.
#[tauri::command]
pub async fn create_related_tab(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    url: String,
) -> WisResult<Tab> {
    mutate(&app, &state, |s| {
        s.with_new_related_tab(workspace_id, &url, true)
    })
    .await
}

/// Split the workspace's focused pane along `dir`, opening `url` in the new
/// pane and focusing it. Returns the newly created tab. The frontend's webview
/// reconciler then spins up its native webview and tiles it into the pane.
#[tauri::command]
pub async fn split_pane(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    dir: SplitDir,
    place_new_first: bool,
    url: String,
) -> WisResult<Tab> {
    mutate(&app, &state, |s| {
        s.with_pane_split(workspace_id, dir, place_new_first, &url)
    })
    .await
}

/// Replace a pane's tab with an already-open `moved_tab` (drag to pane centre).
/// A throwaway placeholder in that pane is closed.
#[tauri::command]
pub async fn replace_pane_leaf(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    target_tab: TabId,
    moved_tab: TabId,
) -> WisResult<()> {
    mutate_unit(&app, &state, |s| {
        s.with_pane_leaf_replaced(workspace_id, target_tab, moved_tab)
    })
    .await
}

/// Split the `target_tab` pane along `dir` with the already-open `moved_tab`
/// (dragged from the tab list onto a pane). `place_new_first` puts the moved
/// tab in the left/top slot.
#[tauri::command]
pub async fn split_pane_with(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    target_tab: TabId,
    dir: SplitDir,
    place_new_first: bool,
    moved_tab: TabId,
) -> WisResult<()> {
    mutate_unit(&app, &state, |s| {
        s.with_pane_split_with(workspace_id, target_tab, dir, place_new_first, moved_tab)
    })
    .await
}

/// Collapse all splits in a workspace back to a single pane (the focused tab).
/// Other panes' tabs remain open as background tabs.
#[tauri::command]
pub async fn unsplit_panes(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
) -> WisResult<()> {
    mutate_unit(&app, &state, |s| s.with_panes_unsplit(workspace_id)).await
}

/// Set the divider ratio (permille, 0..=1000) of the split reached by `path`
/// from the workspace's pane-tree root.
#[tauri::command]
pub async fn set_pane_ratio(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    path: Vec<PaneBranch>,
    ratio: u16,
) -> WisResult<()> {
    mutate_unit(&app, &state, |s| {
        s.with_pane_ratio(workspace_id, &path, ratio)
    })
    .await
}

/// Close a tab. `return_to` (the window's MRU previous tab) is activated in
/// the same transition when this is a plain close of the active tab; a stale
/// or cross-workspace target falls back to core's tab_order neighbor pick.
/// Closing a pane in a split shows the most recent tab of `recent` not
/// already in a pane in its place.
#[tauri::command]
pub async fn close_tab(
    app: AppHandle,
    state: State<'_, AppState>,
    id: TabId,
    return_to: Option<TabId>,
    recent: Option<Vec<TabId>>,
) -> WisResult<()> {
    /// How many closed tabs stay reopenable.
    const CLOSED_TAB_CAP: u32 = 25;
    /// Far past the window's own history.
    const RECENT_CAP: usize = 64;

    let recent: Vec<TabId> = recent
        .unwrap_or_default()
        .into_iter()
        .take(RECENT_CAP)
        .collect();

    crate::page_source::forget(id);
    crate::signup::forget_field(&id.as_uuid().to_string());

    // Record the tab for "reopen closed tab" before it's removed.
    {
        let recorded = {
            let browser = state.browser.lock().await;
            browser.tab(id).ok().and_then(|tab| {
                let url = tab.url.to_string();
                let workspace = browser.workspace(tab.workspace_id).ok();
                let ephemeral = workspace.map(|w| w.ephemeral).unwrap_or(true);
                let position = workspace
                    .and_then(|w| w.tab_order.iter().position(|t| *t == id))
                    .and_then(|slot| i64::try_from(slot).ok())
                    .unwrap_or(APPEND_POSITION);

                let recordable = matches!(tab.url.scheme(), "http" | "https");

                (!ephemeral && recordable).then_some((
                    wis_storage::ClosedTabRecord {
                        id: 0,
                        workspace_id: tab.workspace_id,
                        url,
                        title: tab.title.clone(),
                        folder_id: tab.folder_id,
                        position,
                        pinned: tab.pinned,
                        closed_at: chrono::Utc::now(),
                    },
                    tab.load_error.is_some(),
                ))
            })
        };

        if let Some((record, load_failed)) = recorded {
            // Capture the live session (history/scroll/forms) while the webview
            // still exists, so a reopen in this run restores instead of
            // cold-loading.
            let interaction_state = if load_failed {
                None
            } else {
                crate::interaction_state::read_async(
                    app.webviews()
                        .get(&format!("tab-{}", id.as_uuid()))
                        .cloned(),
                )
                .await
            };

            let storage = state.storage.clone();
            let recorded_row = tokio::task::spawn_blocking(move || {
                let row = storage.record_closed_tab(&record)?;
                storage.prune_closed_tabs(CLOSED_TAB_CAP)?;
                let live: Vec<i64> = storage
                    .list_closed_tabs(CLOSED_TAB_CAP)?
                    .into_iter()
                    .map(|entry| entry.id)
                    .collect();

                wis_storage::StorageResult::Ok((row, live))
            })
            .await;

            match recorded_row {
                Ok(Ok((row, live))) => {
                    let mut sessions = state.closed_tab_sessions.lock().await;
                    if let Some(blob) = interaction_state {
                        sessions.insert(row, blob);
                    }
                    // Prune the oldest rows.
                    let live: std::collections::HashSet<i64> = live.into_iter().collect();
                    sessions.retain(|id, _| live.contains(id));
                }
                Ok(Err(err)) => log::warn!("recording a closed tab failed: {err}"),
                Err(err) => log::warn!("recording a closed tab panicked: {err}"),
            }
        }
    }

    crate::audio::forget_tab(&app, id);
    crate::tab_preview::forget(&app, &state, id);

    mutate_unit_immediate(&app, &state, move |s| {
        let workspace_id = s.tab(id)?.workspace_id;
        let plain_close = s
            .workspace(workspace_id)
            .map(|w| w.active_tab_id == Some(id) && w.pane_tree.is_none())
            .unwrap_or(false);
        let next = s.with_tab_closed_keeping_split(id, &recent)?;

        let Some(target) = return_to else {
            return Ok(next);
        };
        if !plain_close {
            return Ok(next);
        }

        let target_alive = next
            .tab(target)
            .map(|t| t.workspace_id == workspace_id)
            .unwrap_or(false);
        if !target_alive {
            return Ok(next);
        }

        next.with_active_tab(target)
    })
    .await
}

/// Position recorded for a tab whose slot could not be determined; a reopen
/// appends instead of inserting.
const APPEND_POSITION: i64 = -1;

/// Reopen the most-recently-closed tab in its original workspace, activating
/// it. No-op when nothing is left to reopen. Walks backwards past entries
/// whose workspace has since been deleted.
#[tauri::command]
pub async fn restore_closed_tab(app: AppHandle, state: State<'_, AppState>) -> WisResult<()> {
    if active_workspace_is_private(&state).await {
        return Ok(());
    }

    loop {
        let storage = state.storage.clone();
        let entry = run_storage(move || storage.take_latest_closed_tab()).await?;
        let Some(entry) = entry else {
            return Ok(());
        };
        if reopen_entry(&app, &state, entry).await? {
            return Ok(());
        }
    }
}

/// Reopen one specific closed tab, chosen from the browsable list. Taking it
/// removes it, so the same row cannot be reopened twice.
#[tauri::command]
pub async fn reopen_closed_tab(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> WisResult<()> {
    if active_workspace_is_private(&state).await {
        return Ok(());
    }

    let storage = state.storage.clone();
    let entry = run_storage(move || storage.take_closed_tab(id)).await?;
    if let Some(entry) = entry {
        reopen_entry(&app, &state, entry).await?;
    }

    Ok(())
}

/// The closed tabs worth offering, newest first.
#[tauri::command]
pub async fn list_closed_tabs(
    state: State<'_, AppState>,
    limit: u32,
) -> WisResult<Vec<wis_storage::ClosedTabRecord>> {
    let storage = state.storage.clone();
    run_storage(move || storage.list_closed_tabs(limit)).await
}

/// Private workspaces neither record closed tabs nor reopen them.
async fn active_workspace_is_private(state: &State<'_, AppState>) -> bool {
    state.browser.lock().await.active_workspace().ephemeral
}

/// Run a blocking storage call off the async runtime, mapping both the join
/// failure and the storage error onto [`WisError::PersistenceFailed`].
async fn run_storage<T, F>(op: F) -> WisResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> wis_storage::StorageResult<T> + Send + 'static,
{
    tokio::task::spawn_blocking(op)
        .await
        .map_err(|err| wis_core::WisError::PersistenceFailed {
            reason: format!("closed-tab task panicked: {err}"),
        })?
        .map_err(|err| wis_core::WisError::PersistenceFailed {
            reason: err.to_string(),
        })
}

/// Put one closed tab back where it was. Returns false when its workspace has
/// since been deleted.
async fn reopen_entry(
    app: &AppHandle,
    state: &State<'_, AppState>,
    entry: wis_storage::ClosedTabRecord,
) -> WisResult<bool> {
    let exists = {
        let browser = state.browser.lock().await;
        browser.workspace(entry.workspace_id).is_ok()
    };
    if !exists {
        return Ok(false);
    }

    let slot = usize::try_from(entry.position).unwrap_or(usize::MAX);
    let new_tab_id = mutate(app, state, |s| {
        // Loose creation bypasses the workspace's currently-selected folder.
        let (next, tab) = s.with_new_loose_tab(entry.workspace_id, &entry.url)?;
        let tab_id = tab.id;
        let next = next.with_active_workspace(entry.workspace_id)?;

        // Creation appended the tab; move it back to its recorded slot.
        let order = reinsert_at(
            next.workspace(entry.workspace_id)?.tab_order.clone(),
            tab_id,
            slot,
        );
        let next = next.with_tab_order(entry.workspace_id, order)?;

        let next = if entry.pinned {
            // Re-pin (pinning forces loose, so no folder branch applies).
            next.with_tab_pinned(tab_id, true)?.0
        } else {
            let original_folder_exists = entry.folder_id.is_some_and(|fid| {
                next.workspace(entry.workspace_id)
                    .map(|w| w.tab_folders.iter().any(|f| f.id == fid))
                    .unwrap_or(false)
            });
            if original_folder_exists {
                next.with_tab_assigned_folder(tab_id, entry.folder_id)?
            } else {
                next
            }
        };

        Ok((next, tab_id))
    })
    .await?;

    // Hand the session captured at close to the webview the frontend is about
    // to create (consumed by `create_tab_webview`). Absent after a restart.
    let blob = state.closed_tab_sessions.lock().await.remove(&entry.id);
    if let Some(blob) = blob {
        state
            .pending_restore_states
            .lock()
            .await
            .insert(new_tab_id, blob);
    }

    Ok(true)
}

/// Detach a tab into a brand-new workspace shown in its own OS window. One workspace
/// per window, so the tab leaves its current workspace; its live session (history/scroll/forms)
/// is carried across via the captured interaction state and restored when the new
/// window builds the tab's webview. A tab from a private (ephemeral) workspace detaches
/// into a new *private* window, so its content is never persisted. `name`/`accent`
/// seed the new workspace (the chrome picks the next free accent).
#[tauri::command]
pub async fn move_tab_to_new_window(
    app: AppHandle,
    state: State<'_, AppState>,
    tab_id: TabId,
    name: String,
    accent: AccentColor,
) -> WisResult<()> {
    let captured = crate::interaction_state::read_async(
        app.webviews()
            .get(&format!("tab-{}", tab_id.as_uuid()))
            .cloned(),
    )
    .await;

    let (url, source_ephemeral) = {
        let browser = state.browser.lock().await;
        let tab = browser.tab(tab_id)?;
        let ephemeral = browser
            .workspace(tab.workspace_id)
            .map(|w| w.ephemeral)
            .unwrap_or(false);

        (tab.url.clone(), ephemeral)
    };

    // New workspace + the detached tab, with the source tab removed, atomically.
    let (new_ws_id, new_tab_id) = mutate(&app, &state, |s| {
        let (s, ws) = if source_ephemeral {
            s.with_new_private_workspace(name, accent)
        } else {
            s.with_new_workspace(name, accent)
        };
        let (s, tab) = s.with_new_loose_tab(ws.id, url.as_str())?;
        let s = s.with_tab_closed(tab_id)?;

        Ok((s, (ws.id, tab.id)))
    })
    .await?;

    crate::audio::forget_tab(&app, tab_id);

    // Hand the captured session to the webview the new window is about to create
    // (consumed by `create_tab_webview`).
    if let Some(blob) = captured {
        state
            .pending_restore_states
            .lock()
            .await
            .insert(new_tab_id, blob);
    }

    crate::windows::spawn_window(&app, &state, new_ws_id).await?;
    Ok(())
}

/// Move a tab into an existing workspace.
/// Like the new-window detach, the tab is recreated in the target (a fresh id,
/// so there is never a cross-window `tab-*` label clash) and its live session is
/// carried across via the captured interaction state. Refuses to move a private
/// tab into a persistent workspace (that would persist private content); the
/// menu only offers persistent targets.
#[tauri::command]
pub async fn move_tab_to_workspace(
    app: AppHandle,
    state: State<'_, AppState>,
    tab_id: TabId,
    workspace_id: WorkspaceId,
) -> WisResult<()> {
    let captured = crate::interaction_state::read_async(
        app.webviews()
            .get(&format!("tab-{}", tab_id.as_uuid()))
            .cloned(),
    )
    .await;

    let url = {
        let browser = state.browser.lock().await;
        let tab = browser.tab(tab_id)?;
        if tab.workspace_id == workspace_id {
            return Ok(());
        }

        let source_ephemeral = browser
            .workspace(tab.workspace_id)
            .map(|w| w.ephemeral)
            .unwrap_or(false);
        if source_ephemeral && !browser.workspace(workspace_id)?.ephemeral {
            return Err(wis_core::WisError::InvalidInput {
                message: "refusing to move a private tab into a persistent workspace".into(),
            });
        }

        tab.url.to_string()
    };

    let new_tab_id = mutate(&app, &state, |s| {
        let (s, tab) = s.with_new_loose_tab(workspace_id, &url)?;
        let s = s.with_tab_closed(tab_id)?;
        Ok((s, tab.id))
    })
    .await?;

    crate::audio::forget_tab(&app, tab_id);

    if let Some(blob) = captured {
        state
            .pending_restore_states
            .lock()
            .await
            .insert(new_tab_id, blob);
    }

    Ok(())
}

/// Reload a tab bypassing the HTTP cache (WKWebView `reloadFromOrigin`).
#[tauri::command]
pub async fn tab_hard_reload(app: AppHandle, id: TabId) -> WisResult<()> {
    let label = format!("tab-{}", id.as_uuid());
    let Some(webview) = app.webviews().get(&label).cloned() else {
        return Ok(());
    };

    #[cfg(target_os = "macos")]
    {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        webview
            .with_webview(|w| unsafe {
                let wk: *mut AnyObject = w.inner() as *mut AnyObject;
                if !wk.is_null() {
                    let _: *mut AnyObject = msg_send![wk, reloadFromOrigin];
                }
            })
            .map_err(|err| WisError::InvalidInput {
                message: format!("hard reload failed: {err}"),
            })?;
    }

    #[cfg(target_os = "linux")]
    {
        webview
            .with_webview(|w| {
                use webkit2gtk::WebViewExt;
                w.inner().reload_bypass_cache();
            })
            .map_err(|err| WisError::InvalidInput {
                message: format!("hard reload failed: {err}"),
            })?;
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = webview.eval("window.location.reload()");
    }

    Ok(())
}

/// Adjust the page zoom for the site a tab is showing. The new percentage is
/// remembered for that host, so every tab on it and every later visit match;
/// `Reset` drops the host's own zoom to follow the default again. A page with
/// no host is left alone.
///
/// The whole read-step-write runs inside the transition, which holds the
/// browser lock.
#[tauri::command]
pub async fn tab_zoom(
    app: AppHandle,
    state: State<'_, AppState>,
    id: TabId,
    kind: wis_core::ZoomStep,
) -> WisResult<()> {
    let host = {
        let live = app
            .webviews()
            .get(&format!("tab-{}", id.as_uuid()))
            .and_then(|webview| webview.url().ok());
        let url = match live {
            Some(url) => url,
            None => state.browser.lock().await.tab(id)?.url.clone(),
        };
        wis_core::user_agent::normalize_host(url.host_str().unwrap_or_default())
    };
    if host.is_empty() {
        return Ok(());
    }

    let factor = mutate(&app, &state, move |s| {
        let default_percent = s.settings.default_zoom_percent;
        let current = wis_core::zoom_percent_for_host(
            &host,
            &s.settings.zoom_site_overrides,
            default_percent,
        );
        let next = wis_core::stepped_zoom_percent(current, kind, default_percent);

        let mut overrides = s.settings.zoom_site_overrides.clone();
        if kind == wis_core::ZoomStep::Reset {
            overrides.remove(&host);
        } else {
            overrides.insert(host, next);
        }

        let next_state = s.with_settings(wis_core::UserSettingsPatch {
            zoom_site_overrides: Some(overrides),
            ..Default::default()
        });

        Ok((next_state, wis_core::zoom_factor(next)))
    })
    .await?;

    state.refresh_zoom_cache().await;
    apply_tab_zoom(&app, id, factor);

    Ok(())
}

/// Forget one site's zoom.
#[tauri::command]
pub async fn zoom_forget_site(
    app: AppHandle,
    state: State<'_, AppState>,
    host: String,
) -> WisResult<()> {
    let host = wis_core::user_agent::normalize_host(&host);
    mutate_unit(&app, &state, move |s| {
        let mut overrides = s.settings.zoom_site_overrides.clone();
        overrides.remove(&host);
        Ok(s.with_settings(wis_core::UserSettingsPatch {
            zoom_site_overrides: Some(overrides),
            ..Default::default()
        }))
    })
    .await?;

    state.refresh_zoom_cache().await;
    crate::zoom::apply_to_all_tabs(&app);

    Ok(())
}

#[tauri::command]
pub async fn reorder_tabs(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    order: Vec<TabId>,
) -> WisResult<()> {
    mutate_unit(&app, &state, |s| s.with_tab_order(workspace_id, order)).await
}

#[tauri::command]
pub async fn pin_tab(
    app: AppHandle,
    state: State<'_, AppState>,
    id: TabId,
    pinned: bool,
) -> WisResult<Tab> {
    mutate(&app, &state, |s| s.with_tab_pinned(id, pinned)).await
}

// ---------------------------------------------------------------------------
// Tab folders (live-tab grouping)
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn create_tab_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    name: String,
    accent: AccentColor,
) -> WisResult<TabFolder> {
    mutate(&app, &state, |s| {
        s.with_new_tab_folder(workspace_id, name, accent)
    })
    .await
}

#[tauri::command]
pub async fn rename_tab_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    folder_id: TabFolderId,
    name: String,
) -> WisResult<()> {
    mutate_unit(&app, &state, |s| {
        s.with_tab_folder_renamed(workspace_id, folder_id, name)
    })
    .await
}

#[tauri::command]
pub async fn recolor_tab_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    folder_id: TabFolderId,
    accent: AccentColor,
) -> WisResult<()> {
    mutate_unit(&app, &state, |s| {
        s.with_tab_folder_recolored(workspace_id, folder_id, accent)
    })
    .await
}

/// Set or clear a folder's icon (validated encoded reference).
#[tauri::command]
pub async fn set_tab_folder_icon(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    folder_id: TabFolderId,
    icon: Option<String>,
) -> WisResult<()> {
    mutate_unit(&app, &state, |s| {
        s.with_tab_folder_icon(workspace_id, folder_id, icon.clone())
    })
    .await
}

/// Set or clear a workspace's icon (validated encoded reference).
#[tauri::command]
pub async fn set_workspace_icon(
    app: AppHandle,
    state: State<'_, AppState>,
    id: WorkspaceId,
    icon: Option<String>,
) -> WisResult<()> {
    mutate_unit(&app, &state, |s| s.with_workspace_icon(id, icon.clone())).await
}

#[tauri::command]
pub async fn set_tab_folder_collapsed(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    folder_id: TabFolderId,
    collapsed: bool,
) -> WisResult<()> {
    mutate_unit(&app, &state, |s| {
        s.with_tab_folder_collapsed(workspace_id, folder_id, collapsed)
    })
    .await
}

#[tauri::command]
pub async fn delete_tab_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    folder_id: TabFolderId,
) -> WisResult<()> {
    mutate_unit_immediate(&app, &state, |s| {
        s.with_tab_folder_removed(workspace_id, folder_id)
    })
    .await
}

#[tauri::command]
pub async fn reorder_tab_folders(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    order: Vec<TabFolderId>,
) -> WisResult<()> {
    mutate_unit(&app, &state, |s| {
        s.with_tab_folders_reordered(workspace_id, order)
    })
    .await
}

#[tauri::command]
pub async fn assign_tab_to_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    tab_id: TabId,
    folder_id: Option<TabFolderId>,
) -> WisResult<()> {
    mutate_unit(&app, &state, |s| {
        s.with_tab_assigned_folder(tab_id, folder_id)
    })
    .await
}

#[tauri::command]
pub async fn select_tab_folder(
    app: AppHandle,
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    folder_id: Option<TabFolderId>,
) -> WisResult<()> {
    mutate_unit(&app, &state, |s| {
        s.with_selected_tab_folder(workspace_id, folder_id)
    })
    .await
}

#[tauri::command]
pub async fn set_active_tab(
    app: AppHandle,
    state: State<'_, AppState>,
    id: TabId,
) -> WisResult<()> {
    mutate_unit(&app, &state, |s| s.with_active_tab(id)).await
}

/// Navigate an existing tab's child webview to `url`, and persist the new URL
/// in state. If the webview hasn't been created yet (the frontend reconciler
/// will pick it up on the next render), only state is updated: the URL takes
/// effect when the webview comes into being.
#[tauri::command]
pub async fn navigate_tab(
    app: AppHandle,
    state: State<'_, AppState>,
    id: TabId,
    url: String,
) -> WisResult<()> {
    // Update in-memory + persisted state.
    mutate_unit(&app, &state, |s| {
        s.with_navigated_tab(id, &url).map(|(next, _)| next)
    })
    .await?;

    // Drive the actual native webview navigation.
    let label = format!("tab-{}", id.as_uuid());
    if let Some(webview) = app.webviews().get(&label).cloned() {
        let parsed = Url::parse(&url).map_err(|_| WisError::InvalidUrl { url: url.clone() })?;
        webview
            .navigate(parsed)
            .map_err(|err| WisError::InvalidInput {
                message: format!("webview navigate failed: {err}"),
            })?;
    }

    Ok(())
}

/// Note: this does not call `webview.navigate(...)`.
#[tauri::command]
pub async fn sync_tab_url(
    app: AppHandle,
    state: State<'_, AppState>,
    id: TabId,
    url: String,
) -> WisResult<()> {
    mutate_unit(&app, &state, |s| s.with_tab_url_synced(id, &url)).await
}

// ---------------------------------------------------------------------------
// Navigation event bridge IPC + nav controls
// ---------------------------------------------------------------------------

/// Set the tab's title from a programmatic source (e.g. a chrome-injected
/// rename).
#[tauri::command]
pub async fn update_tab_title(
    app: AppHandle,
    state: State<'_, AppState>,
    id: TabId,
    title: String,
) -> WisResult<()> {
    mutate_unit(&app, &state, |s| s.with_tab_title(id, title)).await
}

/// Set a tab's `loading` flag explicitly.
#[tauri::command]
pub async fn update_tab_loading(
    app: AppHandle,
    state: State<'_, AppState>,
    id: TabId,
    loading: bool,
) -> WisResult<()> {
    mutate_unit(&app, &state, |s| s.with_tab_loading(id, loading)).await
}

/// Reload a tab's webview.
#[tauri::command]
pub async fn tab_reload(app: AppHandle, id: TabId) -> WisResult<()> {
    let label = format!("tab-{}", id.as_uuid());
    let webview = app
        .webviews()
        .get(&label)
        .cloned()
        .ok_or_else(|| WisError::InvalidInput {
            message: format!("no webview labelled {label}"),
        })?;
    log::info!("tab_reload: {label}");

    // Native reload useful for hung pages.
    webview.reload().map_err(|err| WisError::InvalidInput {
        message: format!("reload failed: {err}"),
    })
}

/// Stop a tab's in-flight page load. Clears `loading` directly so the
/// stop/reload toggle flips back even if the engine skips a finish event.
#[tauri::command]
pub async fn tab_stop(app: AppHandle, state: State<'_, AppState>, id: TabId) -> WisResult<()> {
    crate::navguard::stop_tab_load(&app, id);
    mutate_unit(&app, &state, |s| s.with_tab_loading(id, false)).await
}

/// Toggle reader mode on a tab: transform the live page into a clean article
/// (`enabled`) or reload to restore the original page. The transform runs
/// inside the tab's own webview, so no untrusted markup reaches the chrome.
/// The `reader` flag mirrors this for the UI; a real navigation resets it
/// (see the page-load handler in `commands::webview`).
#[tauri::command]
pub async fn tab_set_reader(
    app: AppHandle,
    state: State<'_, AppState>,
    id: TabId,
    enabled: bool,
) -> WisResult<()> {
    let label = format!("tab-{}", id.as_uuid());
    let webview = app
        .webviews()
        .get(&label)
        .cloned()
        .ok_or_else(|| WisError::InvalidInput {
            message: format!("no webview labelled {label}"),
        })?;

    let js = if enabled {
        crate::reader::reader_script()
    } else {
        "window.location.reload()"
    };
    webview.eval(js).map_err(|err| WisError::InvalidInput {
        message: format!("reader eval failed: {err}"),
    })?;

    mutate_unit(&app, &state, |s| s.with_tab_reader(id, enabled)).await
}

/// Navigate the tab back in its history.
#[tauri::command]
pub async fn tab_go_back(app: AppHandle, id: TabId) -> WisResult<()> {
    eval_history_step(&app, id, "back")
}

/// Counterpart to [`tab_go_back`].
#[tauri::command]
pub async fn tab_go_forward(app: AppHandle, id: TabId) -> WisResult<()> {
    eval_history_step(&app, id, "forward")
}

fn eval_history_step(app: &AppHandle, id: TabId, direction: &str) -> WisResult<()> {
    let label = format!("tab-{}", id.as_uuid());
    let webview = app
        .webviews()
        .get(&label)
        .cloned()
        .ok_or_else(|| WisError::InvalidInput {
            message: format!("no webview labelled {label}"),
        })?;

    webview
        .eval(format!("window.history.{direction}()"))
        .map_err(|err| WisError::InvalidInput {
            message: format!("history.{direction}() eval failed: {err}"),
        })?;

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        let Some(webview) = app.webviews().get(&label).cloned() else {
            return;
        };

        if let Some((back, fwd)) = crate::swipe_nav::push_nav_state(&webview) {
            let state = app.state::<AppState>();
            let result = mutate_unit(&app, &state, |s| s.with_tab_nav_state(id, back, fwd)).await;
            if let Err(err) = result {
                log::warn!("nav-state refresh for {id} failed: {err}");
            }
        }
    });

    Ok(())
}

/// Move `tab_id` (freshly appended by creation) to `index`, clamped to the
/// list; the rest of the order is preserved.
fn reinsert_at(mut order: Vec<TabId>, tab_id: TabId, index: usize) -> Vec<TabId> {
    order.retain(|t| *t != tab_id);
    let at = index.min(order.len());
    order.insert(at, tab_id);
    order
}

#[cfg(test)]
mod tests {
    use super::reinsert_at;
    use wis_core::TabId;

    #[test]
    fn reinserts_appended_tab_at_recorded_index() {
        let (a, b, c) = (TabId::new(), TabId::new(), TabId::new());
        let reopened = TabId::new();

        let order = reinsert_at(vec![a, b, c, reopened], reopened, 1);
        assert_eq!(order, vec![a, reopened, b, c]);
    }

    #[test]
    fn clamps_index_beyond_len_to_append() {
        let (a, b) = (TabId::new(), TabId::new());
        let reopened = TabId::new();

        let order = reinsert_at(vec![a, b, reopened], reopened, usize::MAX);
        assert_eq!(order, vec![a, b, reopened]);
    }

    #[test]
    fn index_zero_moves_to_front() {
        let (a, b) = (TabId::new(), TabId::new());
        let reopened = TabId::new();

        let order = reinsert_at(vec![a, b, reopened], reopened, 0);
        assert_eq!(order, vec![reopened, a, b]);
    }
}
