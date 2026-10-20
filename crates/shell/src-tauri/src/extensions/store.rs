//! Adding an extension from the Chrome Web Store: the offer on a store item
//! page, and its package, downloaded over the window's workspace proxy and
//! checked against Google's signature before the usual review.

use tauri::{AppHandle, State};
#[cfg(target_os = "macos")]
use wis_core::extensions;
#[cfg(target_os = "macos")]
use wis_core::WisError;
use wis_core::WisResult;

use super::StagedExtension;
use crate::state::AppState;

#[cfg(not(target_os = "macos"))]
use super::unavailable;
#[cfg(target_os = "macos")]
use super::{crx, install};

#[cfg(target_os = "macos")]
const UPDATE_URL: &str = "https://clients2.google.com/service/update2/crx";
/// Recent enough Chrome version.
#[cfg(target_os = "macos")]
const CHROME_VERSION: &str = "140.0.0.0";
#[cfg(target_os = "macos")]
const MAX_PACKAGE: u64 = 100 * 1024 * 1024;
#[cfg(target_os = "macos")]
const MAX_REDIRECTS: usize = 5;
#[cfg(target_os = "macos")]
const DOWNLOAD_WAIT: std::time::Duration = std::time::Duration::from_secs(120);

/// The store id of the extension the window's active tab shows, unless it,
/// or one going by the name the page gives, is installed already.
#[tauri::command]
pub async fn extension_store_offer(
    window: tauri::Window,
    state: State<'_, AppState>,
) -> WisResult<Option<String>> {
    #[cfg(target_os = "macos")]
    {
        if !super::host::supported() {
            return Ok(None);
        }

        let Some(workspace) = state.window_workspace(window.label()) else {
            return Ok(None);
        };
        let browser = state.browser.lock().await;
        let Some(tab) = browser
            .workspaces
            .iter()
            .find(|w| w.id == workspace)
            .and_then(|w| w.active_tab_id)
            .and_then(|id| browser.tabs.iter().find(|t| t.id == id))
        else {
            return Ok(None);
        };
        let Some(store_id) = extensions::store_id_from_url(tab.url.as_str()) else {
            return Ok(None);
        };
        let installed = extensions::duplicate_of(
            &browser.settings.extensions,
            extensions::store_page_name(&tab.title),
            Some(&store_id),
        );

        Ok(installed.is_none().then_some(store_id))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (window, state);
        Ok(None)
    }
}

/// Downloads a store extension, checks its package and stages it for review.
#[tauri::command]
pub async fn extension_stage_from_store(
    app: AppHandle,
    window: tauri::Window,
    state: State<'_, AppState>,
    store_id: String,
) -> WisResult<StagedExtension> {
    #[cfg(target_os = "macos")]
    {
        super::require_support()?;
        let refuse = |message: String| WisError::InvalidInput { message };
        if !extensions::is_store_id(&store_id) {
            return Err(refuse("not a Chrome Web Store id".into()));
        }

        let workspace = state
            .window_workspace(window.label())
            .ok_or_else(|| refuse("this window has no workspace".into()))?;
        if let Some(installed) = extensions::duplicate_of(
            &state.browser.lock().await.settings.extensions,
            None,
            Some(&store_id),
        ) {
            return Err(refuse(format!("{} is already installed", installed.name)));
        }

        let proxy_port = state
            .ensure_proxy_for(workspace)
            .await
            .map_err(WisError::from)?;
        let client = client(proxy_port)?;
        let package = fetch(
            &client,
            store_url(&[("response", "redirect")], &store_id, None)?,
        )
        .await?;
        let archive = verified(package, &store_id).await?;
        install::stage_from(&app, install::Source::Store(archive, store_id)).await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, window, state, store_id);
        Err(unavailable())
    }
}

#[cfg(target_os = "macos")]
fn failed(what: &str, err: &dyn std::fmt::Display) -> WisError {
    WisError::InvalidInput {
        message: format!("{what}: {err}"),
    }
}

/// A client through a workspace's proxy that follows only https redirects.
#[cfg(target_os = "macos")]
pub(super) fn client(proxy_port: u16) -> WisResult<reqwest::Client> {
    let proxy = reqwest::Proxy::all(format!("http://127.0.0.1:{proxy_port}"))
        .map_err(|err| failed("proxy", &err))?;
    reqwest::Client::builder()
        .proxy(proxy)
        .timeout(DOWNLOAD_WAIT)
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() > MAX_REDIRECTS {
                return attempt.error("too many redirects");
            }
            if attempt.url().scheme() != "https" {
                return attempt.error("the store redirected off https");
            }
            attempt.follow()
        }))
        .build()
        .map_err(|err| failed("http client", &err))
}

/// The store's update address for one extension, as Chrome asks: for its
/// package on demand, or whether a newer one than `version` exists.
#[cfg(target_os = "macos")]
pub(super) fn store_url(
    extra: &[(&str, &str)],
    store_id: &str,
    version: Option<&str>,
) -> WisResult<reqwest::Url> {
    let request = match version {
        Some(version) => format!("id={store_id}&v={version}&uc"),
        None => format!("id={store_id}&installsource=ondemand&uc"),
    };
    let mut params = vec![("prodversion", CHROME_VERSION), ("acceptformat", "crx3")];
    params.extend_from_slice(extra);
    params.push(("x", request.as_str()));
    reqwest::Url::parse_with_params(UPDATE_URL, params).map_err(|err| failed("store address", &err))
}

/// A response body, refused past the package cap.
#[cfg(target_os = "macos")]
pub(super) async fn fetch(client: &reqwest::Client, url: reqwest::Url) -> WisResult<Vec<u8>> {
    let too_large = || WisError::InvalidInput {
        message: "the extension package is too large".into(),
    };
    let mut response = client
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|err| failed("the Chrome Web Store could not be reached", &err))?;
    if response.content_length().is_some_and(|n| n > MAX_PACKAGE) {
        return Err(too_large());
    }

    let expected = response.content_length().unwrap_or(0).min(MAX_PACKAGE);
    let mut body = Vec::with_capacity(usize::try_from(expected).unwrap_or(0));
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| failed("the download stopped", &err))?
    {
        body.extend_from_slice(&chunk);
        if body.len() as u64 > MAX_PACKAGE {
            return Err(too_large());
        }
    }

    Ok(body)
}

/// The ZIP inside a store package, once its signatures hold for `store_id`.
#[cfg(target_os = "macos")]
pub(super) async fn verified(package: Vec<u8>, store_id: &str) -> WisResult<Vec<u8>> {
    let id = store_id.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        crx::verified_archive(&package, &id, &crx::STORE_PUBLISHER).map(|start| {
            let mut archive = package;
            archive.drain(..start);
            archive
        })
    })
    .await
    .map_err(|err| failed("the check stopped", &err))?
    .map_err(|err| failed("the package was refused", &err))
}
