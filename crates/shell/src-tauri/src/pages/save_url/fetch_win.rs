//! Save as on Windows, where WebView2 cannot start a download. Wis fetches
//! the URL itself through the workspace's proxy, sending the tab's own
//! cookies and user agent, and streams it into Downloads.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use reqwest::header::{
    HeaderValue, CONTENT_DISPOSITION, CONTENT_TYPE, COOKIE, LOCATION, SET_COOKIE,
};
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncWriteExt, BufWriter};
use url::Url;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2CookieList, ICoreWebView2Settings2, ICoreWebView2_2,
};
use webview2_com::{CoTaskMemPWSTR, GetCookiesCompletedHandler};
use windows::core::{Interface, BOOL, HSTRING, PWSTR};
use wis_core::{DownloadId, TabId, WisResult, WorkspaceId};

use super::invalid;
use crate::data::downloads;
use crate::platform::win_webview2::with_core_webview2;
use crate::state::AppState;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const READ_TIMEOUT: Duration = Duration::from_secs(60);
/// How long the tab's engine has to answer for its cookies or user agent.
const ENGINE_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_REDIRECTS: usize = 10;
const PROGRESS_EVERY: Duration = Duration::from_millis(500);
const WRITE_BUFFER_BYTES: usize = 256 * 1024;

/// Fetch `url` as the tab would, ask where it goes, then stream it into
/// Downloads in the background.
pub(super) async fn start(
    app: &AppHandle,
    tab_id: TabId,
    webview: &tauri::Webview,
    url: Url,
) -> WisResult<()> {
    let (workspace_id, private) = tab_workspace(app, tab_id).await?;
    let proxy_port = app
        .state::<AppState>()
        .ensure_proxy_for(workspace_id)
        .await?;
    let client = client(proxy_port, user_agent(webview).await)?;
    let response = follow(&client, webview, url.clone()).await?;
    if !response.status().is_success() {
        return Err(invalid(format!(
            "The server answered {}",
            response.status().as_u16()
        )));
    }

    let header = |name| {
        response
            .headers()
            .get(name)
            .map(|value: &HeaderValue| String::from_utf8_lossy(value.as_bytes()).into_owned())
    };
    let suggested = wis_core::suggested_filename(
        header(CONTENT_DISPOSITION).as_deref(),
        response.url(),
        header(CONTENT_TYPE).as_deref(),
    );
    let panel_app = app.clone();
    let chosen =
        tokio::task::spawn_blocking(move || downloads::ask_save_as(&panel_app, &suggested))
            .await
            .map_err(|err| invalid(format!("save panel: {err}")))?;
    let Some(path) = chosen else {
        return Ok(());
    };

    let file = tokio::fs::File::create(&path).await.map_err(|err| {
        log::warn!("save as {}: {err}", path.display());
        invalid("The file could not be written")
    })?;
    mark_of_the_web(&path, response.url(), private).await;
    let id = downloads::track(
        app,
        &url,
        path.clone(),
        response.content_length(),
        private,
        true,
    );
    tauri::async_runtime::spawn(stream(app.clone(), id, path, file, response));

    Ok(())
}

/// The tab's workspace, and whether it is private.
async fn tab_workspace(app: &AppHandle, tab_id: TabId) -> WisResult<(WorkspaceId, bool)> {
    let state = app.state::<AppState>();
    let browser = state.browser.lock().await;
    let workspace_id = browser.tab(tab_id)?.workspace_id;
    let private = browser.workspace(workspace_id)?.ephemeral;

    Ok((workspace_id, private))
}

/// A client through the workspace's proxy that follows no redirect itself.
fn client(proxy_port: u16, user_agent: Option<String>) -> WisResult<reqwest::Client> {
    let proxy = reqwest::Proxy::all(format!("http://127.0.0.1:{proxy_port}"))
        .map_err(|err| invalid(format!("proxy: {err}")))?;
    let user_agent = user_agent.unwrap_or_else(|| {
        wis_core::sites::user_agent::compatibility_user_agent(wis_core::UaPlatform::host())
    });

    reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(user_agent)
        .proxy(proxy)
        .build()
        .map_err(|err| invalid(format!("build client: {err}")))
}

/// Request `url`, following redirects by hand so each hop carries the
/// cookies the tab holds for it, and those the chain itself set there, and
/// no other site's.
async fn follow(
    client: &reqwest::Client,
    webview: &tauri::Webview,
    mut url: Url,
) -> WisResult<reqwest::Response> {
    let mut chain = wis_core::ChainCookies::default();
    for _ in 0..=MAX_REDIRECTS {
        let mut request = client.get(url.clone());
        let engine = cookies(webview, &url).await?;
        if let Some(header) = chain.header(&url, &engine) {
            let mut header = HeaderValue::from_str(&header)
                .map_err(|_| invalid("The tab's cookies could not be sent"))?;
            header.set_sensitive(true);
            request = request.header(COOKIE, header);
        }
        let response = request.send().await.map_err(|err| {
            log::warn!("save as {url}: {err}");
            invalid("The download could not start")
        })?;
        if !response.status().is_redirection() {
            return Ok(response);
        }

        for set_cookie in response.headers().get_all(SET_COOKIE) {
            chain.set(&url, &String::from_utf8_lossy(set_cookie.as_bytes()));
        }
        url = response
            .headers()
            .get(LOCATION)
            .map(|location| String::from_utf8_lossy(location.as_bytes()).into_owned())
            .and_then(|location| url.join(&location).ok())
            .filter(|next| matches!(next.scheme(), "http" | "https"))
            .ok_or_else(|| invalid("The download redirected somewhere it cannot follow"))?;
    }

    Err(invalid("The download redirected too many times"))
}

/// The cookies the tab would send to `url`, from its engine's store.
async fn cookies(webview: &tauri::Webview, url: &Url) -> WisResult<Vec<(String, String)>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let uri = url.to_string();
    let withhold_secure = url.scheme() != "https";
    with_core_webview2(webview, move |core| {
        let asked = (|| -> windows::core::Result<()> {
            let core: ICoreWebView2_2 = core.cast()?;
            // SAFETY: on the UI thread via `with_core_webview2`.
            let manager = unsafe { core.CookieManager() }?;
            let handler = GetCookiesCompletedHandler::create(Box::new(move |hr, list| {
                let pairs = list
                    .as_ref()
                    .map(|list| pairs_of(list, withhold_secure))
                    .unwrap_or_default();
                let _ = tx.send(hr.is_ok().then_some(pairs));
                Ok(())
            }));
            // SAFETY: as above; WebView2 copies the URI before returning.
            unsafe { manager.GetCookies(&HSTRING::from(uri), &handler) }
        })();
        if let Err(err) = asked {
            log::warn!("save as: asking the tab for its cookies: {err}");
        }
    });

    match tokio::time::timeout(ENGINE_TIMEOUT, rx).await {
        Ok(Ok(Some(pairs))) => Ok(pairs),
        _ => Err(invalid("The tab's cookies could not be read")),
    }
}

/// The list's `name=value` pairs, leaving out secure cookies when the
/// request is not over HTTPS.
fn pairs_of(list: &ICoreWebView2CookieList, withhold_secure: bool) -> Vec<(String, String)> {
    let mut count = 0u32;
    // SAFETY: on the UI thread inside the completion handler, where the list
    // and its cookies are live.
    if unsafe { list.Count(&mut count) }.is_err() {
        return Vec::new();
    }

    (0..count)
        .filter_map(|index| {
            // SAFETY: as above, and `index` is within `Count`.
            let cookie = unsafe { list.GetValueAtIndex(index) }.ok()?;
            let mut secure = BOOL(0);
            // SAFETY: as above; plain getters.
            unsafe { cookie.IsSecure(&mut secure) }.ok()?;
            if secure.as_bool() && withhold_secure {
                return None;
            }
            let name = read(|out| unsafe { cookie.Name(out) })?;
            let value = read(|out| unsafe { cookie.Value(out) })?;
            Some((name, value))
        })
        .collect()
}

/// The user agent the tab sends, `None` when its engine cannot say.
async fn user_agent(webview: &tauri::Webview) -> Option<String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    with_core_webview2(webview, move |core| {
        // SAFETY: on the UI thread via `with_core_webview2`; plain getters.
        let agent = unsafe { core.Settings() }
            .and_then(|settings| settings.cast::<ICoreWebView2Settings2>())
            .ok()
            .and_then(|settings| read(|out| unsafe { settings.UserAgent(out) }));
        let _ = tx.send(agent);
    });

    tokio::time::timeout(ENGINE_TIMEOUT, rx).await.ok()?.ok()?
}

/// A string a WebView2 getter writes, freed once read.
fn read(get: impl FnOnce(*mut PWSTR) -> windows::core::Result<()>) -> Option<String> {
    let mut raw = PWSTR::null();
    get(&mut raw).ok()?;
    Some(CoTaskMemPWSTR::from(raw).to_string())
}

/// Mark the file as from the internet, so Windows warns before running it.
/// A volume without alternate streams (FAT) cannot hold the mark.
async fn mark_of_the_web(path: &Path, source: &Url, private: bool) {
    let mut stream = path.as_os_str().to_owned();
    stream.push(":Zone.Identifier");
    let mark = wis_core::zone_identifier(source, private);
    if let Err(err) = tokio::fs::write(&stream, mark).await {
        log::warn!("mark of the web on {}: {err}", path.display());
    }
}

/// Write the body to the file, then end the download. A transfer that fails
/// removes its partial file.
async fn stream(
    app: AppHandle,
    id: DownloadId,
    path: PathBuf,
    file: tokio::fs::File,
    response: reqwest::Response,
) {
    let mut file = BufWriter::with_capacity(WRITE_BUFFER_BYTES, file);
    let written = write_body(&app, id, &mut file, response).await;
    drop(file);
    if let Err(err) = &written {
        log::warn!("download {}: {err}", path.display());
        if let Err(err) = tokio::fs::remove_file(&path).await {
            log::debug!("partial file remove: {err}");
        }
    }

    downloads::finish(&app, id, written.is_ok());
}

async fn write_body(
    app: &AppHandle,
    id: DownloadId,
    file: &mut BufWriter<tokio::fs::File>,
    response: reqwest::Response,
) -> Result<(), String> {
    let mut body = response.bytes_stream();
    let mut received = 0u64;
    let mut reported = Instant::now();
    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(|err| err.to_string())?;
        file.write_all(&chunk)
            .await
            .map_err(|err| err.to_string())?;
        received += chunk.len() as u64;
        if reported.elapsed() >= PROGRESS_EVERY {
            downloads::progress(app, id, received);
            reported = Instant::now();
        }
    }

    file.flush().await.map_err(|err| err.to_string())
}
