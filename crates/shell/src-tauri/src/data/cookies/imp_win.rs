use super::{AppState, SiteDataKind};

use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Manager};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2Cookie, ICoreWebView2CookieManager, ICoreWebView2Profile2, ICoreWebView2_13,
    ICoreWebView2_2, COREWEBVIEW2_BROWSING_DATA_KINDS,
    COREWEBVIEW2_BROWSING_DATA_KINDS_CACHE_STORAGE, COREWEBVIEW2_BROWSING_DATA_KINDS_COOKIES,
    COREWEBVIEW2_BROWSING_DATA_KINDS_DISK_CACHE, COREWEBVIEW2_BROWSING_DATA_KINDS_FILE_SYSTEMS,
    COREWEBVIEW2_BROWSING_DATA_KINDS_INDEXED_DB, COREWEBVIEW2_BROWSING_DATA_KINDS_LOCAL_STORAGE,
    COREWEBVIEW2_BROWSING_DATA_KINDS_SERVICE_WORKERS, COREWEBVIEW2_BROWSING_DATA_KINDS_WEB_SQL,
};
use webview2_com::{ClearBrowsingDataCompletedHandler, CoTaskMemPWSTR, GetCookiesCompletedHandler};
use windows::core::{Interface, PCWSTR, PWSTR};
use wis_core::WorkspaceId;

/// A live tab webview of `workspace_id`, or `None` when the workspace
/// has no live webview.
async fn workspace_webview(
    app: &AppHandle,
    workspace_id: WorkspaceId,
) -> Option<tauri::webview::Webview<tauri::Wry>> {
    let state = app.state::<AppState>();
    let labels: Vec<String> = {
        let browser = state.browser.lock().await;
        let ws = browser.workspaces.iter().find(|w| w.id == workspace_id)?;
        ws.tab_order
            .iter()
            .map(|id| format!("tab-{}", id.as_uuid()))
            .collect()
    };

    labels
        .iter()
        .find_map(|label| app.webviews().get(label).cloned())
}

/// Await a UI-thread completion sent over `rx`, off the async runtime.
async fn recv<T: Send + 'static>(rx: mpsc::Receiver<T>, timeout: Duration) -> Option<T> {
    tokio::task::spawn_blocking(move || rx.recv_timeout(timeout).ok())
        .await
        .ok()
        .flatten()
}

/// A cookie's domain, normalized (leading dot stripped, lowercased).
fn read_domain(cookie: &ICoreWebView2Cookie) -> Option<String> {
    let mut d = PWSTR::null();
    // SAFETY: `Domain` writes a COM-allocated string that `CoTaskMemPWSTR`
    // then owns and frees; runs on the UI thread inside the handler.
    if unsafe { cookie.Domain(&mut d) }.is_err() {
        return None;
    }

    let s = CoTaskMemPWSTR::from(d).to_string();
    let s = s.trim_start_matches('.').trim().to_ascii_lowercase();

    (!s.is_empty()).then_some(s)
}

pub(super) async fn list(
    app: &AppHandle,
    workspace_id: WorkspaceId,
) -> Result<Vec<String>, String> {
    let Some(webview) = workspace_webview(app, workspace_id).await else {
        return Ok(Vec::new());
    };
    let (tx, rx) = mpsc::channel::<Vec<String>>();
    crate::platform::win_webview2::with_core_webview2(&webview, move |core| {
        let started = (|| -> windows::core::Result<()> {
            let core2: ICoreWebView2_2 = core.cast()?;
            // SAFETY: on the UI thread via `with_core_webview2`.
            let manager: ICoreWebView2CookieManager = unsafe { core2.CookieManager() }?;
            let tx_ok = tx.clone();
            let handler = GetCookiesCompletedHandler::create(Box::new(move |hr, list| {
                let mut domains: Vec<String> = Vec::new();
                if hr.is_ok() {
                    if let Some(list) = list {
                        let mut count: u32 = 0;
                        let _ = unsafe { list.Count(&mut count) };

                        for i in 0..count {
                            if let Ok(cookie) = unsafe { list.GetValueAtIndex(i) } {
                                if let Some(d) = read_domain(&cookie) {
                                    domains.push(d);
                                }
                            }
                        }
                    }
                }

                let _ = tx_ok.send(domains);
                Ok(())
            }));

            unsafe { manager.GetCookies(PCWSTR::null(), &handler) }
        })();

        if started.is_err() {
            let _ = tx.send(Vec::new());
        }
    });

    let mut domains = recv(rx, Duration::from_secs(10)).await.unwrap_or_default();
    domains.sort();
    domains.dedup();

    Ok(domains)
}

pub(super) async fn delete(
    app: &AppHandle,
    workspace_id: WorkspaceId,
    domain: String,
) -> Result<(), String> {
    let target = domain.trim_start_matches('.').trim().to_ascii_lowercase();
    if let Some(webview) = workspace_webview(app, workspace_id).await {
        delete_cookies(&webview, target.clone()).await;
    }
    if let Some(other) = other_route_webview(app, workspace_id).await? {
        delete_cookies(&other, target).await;
        close(&other);
    }
    Ok(())
}

/// Deletes the cookies `webview`'s profile holds for `target`.
async fn delete_cookies(webview: &tauri::webview::Webview<tauri::Wry>, target: String) {
    let (tx, rx) = mpsc::channel::<()>();
    crate::platform::win_webview2::with_core_webview2(webview, move |core| {
        let started = (|| -> windows::core::Result<()> {
            let core2: ICoreWebView2_2 = core.cast()?;
            let manager: ICoreWebView2CookieManager = unsafe { core2.CookieManager() }?;
            let manager_del = manager.clone();
            let tx_done = tx.clone();
            let handler = GetCookiesCompletedHandler::create(Box::new(move |hr, list| {
                if hr.is_ok() {
                    if let Some(list) = list {
                        let mut count: u32 = 0;
                        let _ = unsafe { list.Count(&mut count) };

                        for i in 0..count {
                            if let Ok(cookie) = unsafe { list.GetValueAtIndex(i) } {
                                if read_domain(&cookie).as_deref() == Some(target.as_str()) {
                                    // SAFETY: UI thread; cookie is from this list.
                                    let _ = unsafe { manager_del.DeleteCookie(&cookie) };
                                }
                            }
                        }
                    }
                }

                let _ = tx_done.send(());
                Ok(())
            }));

            unsafe { manager.GetCookies(PCWSTR::null(), &handler) }
        })();

        if started.is_err() {
            let _ = tx.send(());
        }
    });

    let _ = recv(rx, Duration::from_secs(10)).await;
}

/// Reaches a little past now so a record changed while the call is in
/// flight, or under a clock slightly behind the caller's, still goes.
const RANGE_END_SLACK_SECS: f64 = 60.0;

/// The WebView2 data kinds covered by `kinds`.
fn browsing_data_kinds(kinds: &[SiteDataKind]) -> COREWEBVIEW2_BROWSING_DATA_KINDS {
    let mut flags = COREWEBVIEW2_BROWSING_DATA_KINDS(0);
    if kinds.contains(&SiteDataKind::Cookies) {
        flags |= COREWEBVIEW2_BROWSING_DATA_KINDS_COOKIES;
    }
    if kinds.contains(&SiteDataKind::Cache) {
        flags |= COREWEBVIEW2_BROWSING_DATA_KINDS_DISK_CACHE
            | COREWEBVIEW2_BROWSING_DATA_KINDS_CACHE_STORAGE;
    }
    if kinds.contains(&SiteDataKind::Storage) {
        flags |= COREWEBVIEW2_BROWSING_DATA_KINDS_LOCAL_STORAGE
            | COREWEBVIEW2_BROWSING_DATA_KINDS_INDEXED_DB
            | COREWEBVIEW2_BROWSING_DATA_KINDS_WEB_SQL
            | COREWEBVIEW2_BROWSING_DATA_KINDS_FILE_SYSTEMS
            | COREWEBVIEW2_BROWSING_DATA_KINDS_SERVICE_WORKERS;
    }

    flags
}

pub(super) async fn clear(
    app: &AppHandle,
    workspace_id: WorkspaceId,
    kinds: &[SiteDataKind],
    since_unix: Option<i64>,
) -> Result<(), String> {
    let Some(webview) = workspace_webview(app, workspace_id).await else {
        return Err("open a tab in this workspace to clear its data".to_string());
    };
    let data_kinds = browsing_data_kinds(kinds);
    let range = since_unix.map(|secs| {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64());
        (secs as f64, now + RANGE_END_SLACK_SECS)
    });

    clear_profile(&webview, data_kinds, range).await?;
    if let Some(other) = other_route_webview(app, workspace_id).await? {
        let cleared = clear_profile(&other, data_kinds, range).await;
        close(&other);
        cleared?;
    }
    Ok(())
}

/// Clears `data_kinds` (within `range`, in seconds) from `webview`'s profile.
async fn clear_profile(
    webview: &tauri::webview::Webview<tauri::Wry>,
    data_kinds: COREWEBVIEW2_BROWSING_DATA_KINDS,
    range: Option<(f64, f64)>,
) -> Result<(), String> {
    let (tx, rx) = mpsc::channel::<bool>();
    crate::platform::win_webview2::with_core_webview2(webview, move |core| {
        let started = (|| -> windows::core::Result<()> {
            let core13: ICoreWebView2_13 = core.cast()?;
            // SAFETY: on the UI thread via `with_core_webview2`.
            let profile = unsafe { core13.Profile() }?;
            let profile2: ICoreWebView2Profile2 = profile.cast()?;
            let tx_ok = tx.clone();
            let handler = ClearBrowsingDataCompletedHandler::create(Box::new(move |hr| {
                let _ = tx_ok.send(hr.is_ok());
                Ok(())
            }));

            match range {
                None => unsafe { profile2.ClearBrowsingData(data_kinds, &handler) },
                Some((start, end)) => unsafe {
                    profile2.ClearBrowsingDataInTimeRange(data_kinds, start, end, &handler)
                },
            }
        })();

        if started.is_err() {
            let _ = tx.send(false);
        }
    });

    match recv(rx, Duration::from_secs(15)).await {
        Some(true) => Ok(()),
        _ => Err("clearing site data failed".to_string()),
    }
}

/// A hidden webview in the workspace's other route's folder, for reaching its
/// data: Tor and SOCKS keep theirs apart on Windows (`proxied_folder`). `None`
/// for a private workspace or a folder never made.
async fn other_route_webview(
    app: &AppHandle,
    workspace_id: WorkspaceId,
) -> Result<Option<tauri::webview::Webview<tauri::Wry>>, String> {
    let state = app.state::<AppState>();
    let proxied = {
        let browser = state.browser.lock().await;
        match browser.workspace(workspace_id) {
            Ok(ws) if !ws.ephemeral => ws.upstream_proxy.is_some(),
            _ => return Ok(None),
        }
    };
    let dir = crate::commands::workspace_webview_data_dir(app, workspace_id, !proxied);
    if !tokio::fs::try_exists(&dir).await.unwrap_or(false) {
        return Ok(None);
    }

    let port = state.ensure_proxy_for(workspace_id).await.map_err(|err| {
        log::warn!("site data: proxy for {workspace_id} unavailable: {err:?}");
        "the other route's data could not be reached".to_string()
    })?;
    let window_label = state
        .window_workspaces
        .lock()
        .expect("window_workspaces poisoned")
        .iter()
        .find(|(_, ws)| **ws == workspace_id)
        .map(|(label, _)| label.clone());
    let window = window_label
        .and_then(|label| app.get_window(&label))
        .or_else(|| app.windows().into_values().next())
        .ok_or("no window to clear from")?;

    let label = format!("wis-clear-{}", uuid::Uuid::new_v4());
    let blank = "about:blank".parse().expect("about:blank is a valid URL");
    let proxy = format!("http://127.0.0.1:{port}")
        .parse()
        .expect("loopback proxy URL is valid");
    let builder = tauri::webview::WebviewBuilder::new(&label, tauri::WebviewUrl::External(blank))
        .data_directory(dir)
        .focused(false)
        .proxy_url(proxy)
        .scroll_bar_style(tauri::webview::ScrollBarStyle::FluentOverlay);
    let webview = window
        .add_child(
            builder,
            tauri::LogicalPosition::new(-10_000.0, 0.0),
            tauri::LogicalSize::new(1.0, 1.0),
        )
        .map_err(|err| format!("the other route's folder would not open: {err}"))?;
    let _ = webview.hide();
    Ok(Some(webview))
}

fn close(webview: &tauri::webview::Webview<tauri::Wry>) {
    if let Err(err) = webview.close() {
        log::warn!("site data: a clearing webview stayed open: {err}");
    }
}
