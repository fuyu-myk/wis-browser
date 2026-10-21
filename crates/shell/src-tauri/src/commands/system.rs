use super::*;

// ---------------------------------------------------------------------------
// Runtime window/dock icon swap
// ---------------------------------------------------------------------------

/// Variant of the wis owl shown as the window/dock icon. `"closed"` for
/// private/reader contexts; `"open"` (or anything else) for the default state.
/// Frontend hovers and private-mode toggles invoke this to keep the OS-level
/// brand surface in sync with the in-app state.
#[tauri::command]
pub async fn set_window_icon(
    app: AppHandle,
    window_label: String,
    variant: String,
) -> WisResult<()> {
    let file = match variant.as_str() {
        "closed" => "icons/runtime/mark-closed-512.png",
        _ => "icons/runtime/mark-open-512.png",
    };
    let path = app
        .path()
        .resolve(file, BaseDirectory::Resource)
        .map_err(|err| WisError::InvalidInput {
            message: format!("resolve icon {file}: {err}"),
        })?;
    let icon = Image::from_path(&path).map_err(|err| WisError::InvalidInput {
        message: format!("load icon {file}: {err}"),
    })?;
    let window = app
        .get_webview_window(&window_label)
        .ok_or_else(|| WisError::InvalidInput {
            message: format!("no window labelled {window_label}"),
        })?;

    window.set_icon(icon).map_err(|err| WisError::InvalidInput {
        message: format!("set_icon failed: {err}"),
    })
}

/// Set the macOS **Dock** icon (the application icon, distinct from the window
/// icon `set_window_icon` touches). `"closed"` overrides it with the
/// closed-eye owl for private browsing; any other value clears the override so
/// the system restores the bundled app icon (which keeps its Liquid Glass
/// treatment on macOS 26+). No-op on non-macOS platforms.
#[tauri::command]
pub async fn set_dock_icon(app: AppHandle, variant: String) -> WisResult<()> {
    #[cfg(target_os = "macos")]
    {
        let icon_path: Option<String> = if variant == "closed" {
            let path = app
                .path()
                .resolve("icons/runtime/mark-closed-512.png", BaseDirectory::Resource)
                .map_err(|err| WisError::InvalidInput {
                    message: format!("resolve dock icon: {err}"),
                })?;
            Some(path.to_string_lossy().to_string())
        } else {
            None
        };

        // The NSImage load and the Dock update (`setApplicationIconImage:` drives
        // `-[NSDockTile display]` -> CoreGraphics image rendering) must run on the
        // main thread.
        app.run_on_main_thread(move || {
            use objc2::runtime::AnyObject;
            use objc2::{class, msg_send};
            use std::ffi::CString;

            unsafe {
                let image: *mut AnyObject = match icon_path.and_then(|p| CString::new(p).ok()) {
                    Some(c) => {
                        let ns_path: *mut AnyObject =
                            msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()];
                        let alloc: *mut AnyObject = msg_send![class!(NSImage), alloc];
                        msg_send![alloc, initWithContentsOfFile: ns_path]
                    }
                    None => std::ptr::null_mut(),
                };
                let ns_app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
                let _: () = msg_send![ns_app, setApplicationIconImage: image];
            }
        })
        .map_err(|err| WisError::InvalidInput {
            message: format!("dock icon dispatch failed: {err}"),
        })?;
    }

    #[cfg(not(target_os = "macos"))]
    let _ = (app, variant);

    Ok(())
}

/// Import a `.user.js` file via the native open panel, parse its metadata,
/// and return the parsed script for the UI to add to settings. `None` when
/// the user cancels.
#[tauri::command]
pub async fn import_userscript(
    app: AppHandle,
) -> WisResult<Option<wis_core::userscript::Userscript>> {
    let Some(path) = crate::panels::run_open_panel(&app, &["js"])? else {
        return Ok(None);
    };

    let size = std::fs::metadata(&path)
        .map(|m| m.len())
        .unwrap_or(u64::MAX);
    if size > wis_core::userscript::MAX_SOURCE_BYTES as u64 {
        return Err(WisError::InvalidInput {
            message: "user script file is too large".to_string(),
        });
    }

    let source = std::fs::read_to_string(&path).map_err(|err| WisError::InvalidInput {
        message: format!("read user script: {err}"),
    })?;
    let script = wis_core::userscript::Userscript::from_source(&source)?;

    Ok(Some(script))
}

/// Start the element picker ("zapper") on a tab's webview: the user clicks
/// an element to hide it, and the chosen selector is persisted per-host.
#[tauri::command]
pub async fn start_element_picker(app: AppHandle, id: wis_core::TabId) -> WisResult<()> {
    let label = format!("tab-{}", id.as_uuid());
    if let Some(webview) = app.webviews().get(&label) {
        crate::cosmetic::start_picker(webview);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Filter list refresh
// ---------------------------------------------------------------------------

/// How far a refresh has got: `done` of `total` steps, one per source and a
/// last one applying them.
pub const FILTER_REFRESH_PROGRESS_EVENT: &str = "wis-filter-refresh-progress";

#[derive(Clone, serde::Serialize)]
struct RefreshProgress {
    done: usize,
    total: usize,
}

enum FilterSource<'a> {
    EasyPrivacy,
    EasyList,
    Scriptlets,
    Subscription(&'a str),
}

impl FilterSource<'_> {
    async fn refresh(&self, cache_dir: &std::path::Path) -> wis_network::NetworkResult<String> {
        use wis_network::filter_lists::fetch;
        match self {
            Self::EasyPrivacy => fetch::refresh_easyprivacy(cache_dir).await,
            Self::EasyList => fetch::refresh_easylist(cache_dir).await,
            Self::Scriptlets => fetch::refresh_resources(cache_dir).await,
            Self::Subscription(url) => fetch::refresh_subscription(cache_dir, url).await,
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::EasyPrivacy => "easyprivacy",
            Self::EasyList => "easylist",
            Self::Scriptlets => "scriptlets",
            Self::Subscription(url) => url,
        }
    }
}

/// Force-refresh every filter source (EasyPrivacy, EasyList, the scriptlet
/// library, subscriptions) regardless of cache age, rebuild the engine, and
/// re-attach the WebKit rules to every existing tab, reporting each step.
/// Returns a short human-readable summary for the settings UI.
#[tauri::command]
pub async fn refresh_filter_lists(app: AppHandle, state: State<'_, AppState>) -> WisResult<String> {
    let _one_at_a_time = state.filter_refresh.lock().await;
    let cache_dir = state.filter_cache_dir.clone();
    let sub_urls = {
        let browser = state.browser.lock().await;
        browser.settings.subscribed_lists()
    };
    let sources: Vec<FilterSource> = [
        FilterSource::EasyPrivacy,
        FilterSource::EasyList,
        FilterSource::Scriptlets,
    ]
    .into_iter()
    .chain(sub_urls.iter().map(|url| FilterSource::Subscription(url)))
    .collect();
    let total = sources.len() + 1;
    let report = |done: usize| {
        if let Err(err) = app.emit(
            FILTER_REFRESH_PROGRESS_EVENT,
            RefreshProgress { done, total },
        ) {
            log::warn!("filter refresh progress event failed: {err}");
        }
    };

    let mut failed = 0usize;
    for (done, source) in sources.iter().enumerate() {
        report(done);
        if let Err(err) = source.refresh(&cache_dir).await {
            log::warn!("filter_lists: {} refresh failed: {err}", source.name());
            failed += 1;
        }
    }
    report(sources.len());
    let rebuilt = rebuild_filter_lists(&app, &state).await;
    report(total);
    rebuilt?;

    let updated = sources.len() - failed;
    Ok(if failed == 0 {
        format!("Updated {updated} sources")
    } else {
        format!("Updated {updated} sources, {failed} failed")
    })
}

/// Fetches the notice list a settings change switched to, unless cached, and
/// rebuilds with it. Runs in the background; failures only log.
pub(crate) fn apply_notice_list(app: AppHandle, notice_list: Option<&'static str>) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let cache_dir = &state.filter_cache_dir;
        if let Some(url) = notice_list {
            if wis_network::filter_lists::fetch::subscription_is_stale(cache_dir, url) {
                if let Err(err) =
                    wis_network::filter_lists::fetch::refresh_subscription(cache_dir, url).await
                {
                    log::warn!("filter_lists: notice list fetch failed: {err}");
                }
            }
        }
        if let Err(err) = rebuild_filter_lists(&app, &state).await {
            log::warn!("filter_lists: rebuild for the notice list failed: {err}");
        }
    });
}

/// Rebuilds from the cached lists and re-attaches the new WebKit rules to
/// every live tab.
pub(crate) async fn rebuild_filter_lists(app: &AppHandle, state: &AppState) -> WisResult<()> {
    let (text, _) = wis_network::filter_lists::load_cached_or_default(&state.filter_cache_dir);
    state
        .rebuild_filter_engine(text)
        .await
        .map_err(wis_core::WisError::from)?;

    let tabs: Vec<(wis_core::TabId, wis_core::PrivacyMode)> = {
        let browser = state.browser.lock().await;
        browser
            .tabs
            .iter()
            .filter_map(|tab| {
                browser
                    .workspace(tab.workspace_id)
                    .ok()
                    .map(|w| (tab.id, w.privacy_mode))
            })
            .collect()
    };
    for (tab_id, mode) in tabs {
        let label = format!("tab-{}", tab_id.as_uuid());
        let Some(webview) = app.webviews().get(&label).cloned() else {
            continue;
        };
        let specs = state.content_rule_specs(mode);
        if let Err(err) = crate::content_filter::replace_rule_lists(&webview, &specs) {
            log::warn!("content filter: re-attach for {label} skipped: {err}");
        }
    }

    Ok(())
}
