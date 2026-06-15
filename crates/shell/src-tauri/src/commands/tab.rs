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

#[tauri::command]
pub async fn close_tab(app: AppHandle, state: State<'_, AppState>, id: TabId) -> WisResult<()> {
    /// Newest-first cap on the reopen-closed-tab ring.
    const CLOSED_RING_CAP: usize = 25;

    // Record the tab for "reopen closed tab" before it's removed.
    {
        let recorded = {
            let browser = state.browser.lock().await;
            browser.tab(id).ok().and_then(|tab| {
                let url = tab.url.to_string();
                let ephemeral = browser
                    .workspace(tab.workspace_id)
                    .map(|w| w.ephemeral)
                    .unwrap_or(true);

                (!ephemeral && !url.starts_with("about:")).then_some((
                    tab.workspace_id,
                    url,
                    tab.folder_id,
                ))
            })
        };

        if let Some((workspace_id, url, folder_id)) = recorded {
            // Capture the live session (history/scroll/forms) while the
            // webview still exists, so reopen restores instead of cold-loads.
            let interaction_state = app
                .webviews()
                .get(&format!("tab-{}", id.as_uuid()))
                .and_then(crate::interaction_state::read);

            let mut ring = state.closed_tabs.lock().await;
            ring.push(crate::state::ClosedTab {
                workspace_id,
                url,
                folder_id,
                interaction_state,
            });

            let overflow = ring.len().saturating_sub(CLOSED_RING_CAP);
            if overflow > 0 {
                ring.drain(0..overflow);
            }
        }
    }

    crate::audio::forget_tab(&app, id);

    mutate_unit_immediate(&app, &state, |s| s.with_tab_closed(id)).await
}

/// Reopen the most-recently-closed tab in its original workspace, activating it.
/// No-op when the ring is empty or that workspace no longer exists.
#[tauri::command]
pub async fn restore_closed_tab(app: AppHandle, state: State<'_, AppState>) -> WisResult<()> {
    // Inert in private workspaces.
    {
        let browser = state.browser.lock().await;
        if browser.active_workspace().ephemeral {
            return Ok(());
        }
    }

    loop {
        let entry = {
            let mut ring = state.closed_tabs.lock().await;
            ring.pop()
        };
        let Some(entry) = entry else {
            return Ok(());
        };

        let exists = {
            let browser = state.browser.lock().await;
            browser.workspace(entry.workspace_id).is_ok()
        };
        if !exists {
            continue;
        }

        let new_tab_id = mutate(&app, &state, |s| {
            // Loose creation bypasses the workspace's currently-selected folder.
            let (next, tab) = s.with_new_loose_tab(entry.workspace_id, &entry.url)?;
            let tab_id = tab.id;
            let next = next.with_active_workspace(entry.workspace_id)?;
            let original_folder_exists = entry.folder_id.is_some_and(|fid| {
                next.workspace(entry.workspace_id)
                    .map(|w| w.tab_folders.iter().any(|f| f.id == fid))
                    .unwrap_or(false)
            });
            let next = if original_folder_exists {
                next.with_tab_assigned_folder(tab_id, entry.folder_id)?
            } else {
                next
            };

            Ok((next, tab_id))
        })
        .await?;

        // Hand the captured session to the webview the frontend is about to
        // create for the reopened tab (consumed by `create_tab_webview`).
        if let Some(blob) = entry.interaction_state {
            state
                .pending_restore_states
                .lock()
                .await
                .insert(new_tab_id, blob);
        }

        return Ok(());
    }
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
    let captured = app
        .webviews()
        .get(&format!("tab-{}", tab_id.as_uuid()))
        .and_then(crate::interaction_state::read);

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
    let captured = app
        .webviews()
        .get(&format!("tab-{}", tab_id.as_uuid()))
        .and_then(crate::interaction_state::read);

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

    #[cfg(not(target_os = "macos"))]
    {
        let _ = webview.eval("window.location.reload()");
    }

    Ok(())
}

/// Adjust a tab's page zoom. `kind` is `"in"`, `"out"`, or `"reset"`. The new
/// factor is computed from the persisted zoom, stored on the tab,
/// then applied to the live webview.
#[tauri::command]
pub async fn tab_zoom(
    app: AppHandle,
    state: State<'_, AppState>,
    id: TabId,
    kind: String,
) -> WisResult<()> {
    let current = {
        let browser = state.browser.lock().await;
        browser.tab(id).map(|t| t.zoom).unwrap_or(1.0)
    };
    let next = match kind.as_str() {
        "in" => (current * 1.1).min(3.0),
        "out" => (current / 1.1).max(0.5),
        _ => 1.0,
    };
    // Snap to 3 d.p. to minimize float drift.
    let next = (next * 1000.0).round() / 1000.0;

    mutate_unit(&app, &state, |s| s.with_tab_zoom(id, next)).await?;
    apply_tab_zoom(&app, id, next);

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

    webview
        .eval("window.location.reload()")
        .map_err(|err| WisError::InvalidInput {
            message: format!("reload eval failed: {err}"),
        })
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
        })
}
