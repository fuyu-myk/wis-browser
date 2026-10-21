//! Favicon cache.
//!
//! This is used in favour of `google.com/s2/favicons`, which leaked every
//! navigation target to Google.
//!
//! Capture pipeline (called from `on_page_load(Finished)`):
//!   1. Resolve the page origin to a canonical URL.
//!   2. Hash the origin with SHA-256 (first 16 hex chars). Same-origin
//!      navigations reuse the cache entry without a network round trip.
//!   3. If the bytes are already on disk, just update `tab.favicon` with
//!      the hash.
//!   4. Otherwise GET `<origin>/favicon.ico` through the workspace's
//!      loopback proxy so DoH + tracker rules apply. Cap at 5 s + 256 KB.
//!      If that yields no usable image (404, HTML error page, CDN-hosted
//!      icons), fall back to fetching the page itself and following its
//!      `<link rel="icon">` declarations.
//!   5. Sniff magic bytes (PNG / ICO / SVG / JPEG / WebP / GIF). Unknown
//!      formats are rejected so a broken host can't dump arbitrary bytes
//!      into the cache.
//!   6. Write to `<app_data>/favicons/<hash>.<ext>` and update
//!      `tab.favicon`.
//!
//! Frontend reads cached entries via [`get_favicon_data_url`], which
//! returns a base64 `data:` URL. Avoids registering an asset-protocol
//! scope; favicons are small enough that the encoding cost is trivial.

use std::path::PathBuf;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use sha2::{Digest, Sha256};
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Emitter, Manager};
use url::Url;
use wis_core::{FaviconRef, TabId, WisError, WisResult, WorkspaceId};

use crate::state::AppState;

/// Broadcast whenever a freshly captured favicon is written to disk.
pub const FAVICON_CACHED_EVENT: &str = "favicon-cached";

const FAVICON_TIMEOUT: Duration = Duration::from_secs(5);
const FAVICON_MAX_BYTES: usize = 256 * 1024;
/// `<link rel="icon">` declarations live in `<head>`; scanning this much of
/// the page body is plenty while bounding memory and CPU.
const HTML_SCAN_BYTES: usize = 256 * 1024;
/// Hard cap on a fetched page body during the HTML fallback.
const HTML_MAX_BYTES: usize = 4 * 1024 * 1024;
/// How many `<link rel=icon>` candidates to try before giving up.
const MAX_ICON_CANDIDATES: usize = 3;

/// Disk location for cached favicons. Created on demand.
fn cache_dir(app: &AppHandle) -> Result<PathBuf, WisError> {
    let dir = app
        .path()
        .resolve("favicons", BaseDirectory::AppData)
        .map_err(|err| WisError::InvalidInput {
            message: format!("resolve favicons dir: {err}"),
        })?;
    if !dir.exists() {
        std::fs::create_dir_all(&dir).map_err(|err| WisError::InvalidInput {
            message: format!("create favicons dir {}: {err}", dir.display()),
        })?;
    }

    Ok(dir)
}

/// SHA-256 of the origin URL, truncated to 16 hex chars.
///
/// Truncation is safe here: collisions just mean two distinct origins
/// share a cache slot, which produces a visually wrong favicon.
fn origin_hash(origin: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(origin.as_bytes());
    let bytes = hasher.finalize();
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();

    hex[..16].to_string()
}

/// Canonical scheme+host+port string used as the hash input.
///
/// Strips a leading `www.` from the host so the bare domain and its
/// `www`-prefixed redirect target collapse to one cache entry.
fn canonical_origin(url: &Url) -> Option<String> {
    let scheme = url.scheme();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let host = url.host_str()?;
    let normalized_host = host.strip_prefix("www.").unwrap_or(host);
    let port_suffix = url.port().map(|p| format!(":{p}")).unwrap_or_default();

    Some(format!("{scheme}://{normalized_host}{port_suffix}"))
}

/// Public origin-hash entry point so the frontend can compute the same
/// hash as the Rust capture pipeline.
pub fn origin_hash_for_url(url: &Url) -> Option<String> {
    canonical_origin(url).map(|origin| origin_hash(&origin))
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Format {
    Png,
    Ico,
    Svg,
    Jpeg,
    Webp,
    Gif,
}

impl Format {
    pub(crate) fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Ico => "ico",
            Self::Svg => "svg",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
            Self::Gif => "gif",
        }
    }

    fn mime(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Ico => "image/x-icon",
            Self::Svg => "image/svg+xml",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
            Self::Gif => "image/gif",
        }
    }
}

/// Identify an image format by its magic bytes.
pub(crate) fn sniff_format(bytes: &[u8]) -> Option<Format> {
    if bytes.len() >= 8 && &bytes[..8] == b"\x89PNG\r\n\x1a\n" {
        return Some(Format::Png);
    }
    if bytes.len() >= 4 && &bytes[..4] == b"\x00\x00\x01\x00" {
        return Some(Format::Ico);
    }
    if bytes.len() >= 3 && &bytes[..3] == b"\xff\xd8\xff" {
        return Some(Format::Jpeg);
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some(Format::Webp);
    }
    if bytes.len() >= 6 && (&bytes[..6] == b"GIF87a" || &bytes[..6] == b"GIF89a") {
        return Some(Format::Gif);
    }
    let head = &bytes[..bytes.len().min(512)];
    if let Ok(text) = std::str::from_utf8(head) {
        if text.contains("<svg") {
            return Some(Format::Svg);
        }
    }

    None
}

/// Scan the cache dir for a file matching `<hash>.<ext>`. Returns the
/// first match because the hash is unique per origin.
fn favicon_path(dir: &std::path::Path, hash: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if let Some((stem, _ext)) = name_str.rsplit_once('.') {
            if stem == hash {
                return Some(entry.path());
            }
        }
    }

    None
}

/// GET `url` and validate it as a favicon image: 2xx, non-empty, within the
/// size cap, and a recognised magic-byte format.
async fn fetch_image(client: &reqwest::Client, url: &str) -> Result<(Vec<u8>, Format), String> {
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|err| format!("GET {url}: {err}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("GET {url}: status {status}"));
    }

    let bytes = resp
        .bytes()
        .await
        .map_err(|err| format!("read body: {err}"))?;
    if bytes.is_empty() {
        return Err("empty body".into());
    }
    if bytes.len() > FAVICON_MAX_BYTES {
        return Err(format!("body too large: {} bytes", bytes.len()));
    }
    let format = sniff_format(&bytes).ok_or_else(|| "unknown image format".to_string())?;

    Ok((bytes.to_vec(), format))
}

/// The page read for `<link rel=icon>` when `/favicon.ico` fails.
fn icon_page(origin: &str) -> Option<Url> {
    Url::parse(&format!("{origin}/")).ok()
}

/// Fetch the page itself and try the icons it declares via `<link rel=icon>`.
/// Hrefs are resolved against the response's final URL (after redirects) so
/// relative, root-relative, and protocol-relative paths all work.
async fn icon_from_page_html(
    client: &reqwest::Client,
    page_url: &Url,
) -> Result<(Vec<u8>, Format), String> {
    let resp = client
        .get(page_url.clone())
        .send()
        .await
        .map_err(|err| format!("GET {page_url}: {err}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("GET {page_url}: status {status}"));
    }

    let final_url = resp.url().clone();
    let body = resp
        .bytes()
        .await
        .map_err(|err| format!("read page body: {err}"))?;
    if body.len() > HTML_MAX_BYTES {
        return Err(format!("page too large: {} bytes", body.len()));
    }

    let head = String::from_utf8_lossy(&body[..body.len().min(HTML_SCAN_BYTES)]);
    let candidates = extract_icon_hrefs(&head);
    if candidates.is_empty() {
        return Err("no <link rel=icon> in page".into());
    }

    let mut last_err = String::from("no candidate tried");
    for href in candidates.iter().take(MAX_ICON_CANDIDATES) {
        let Ok(icon_url) = final_url.join(href) else {
            continue;
        };
        if !matches!(icon_url.scheme(), "http" | "https") {
            continue;
        }
        match fetch_image(client, icon_url.as_str()).await {
            Ok(found) => return Ok(found),
            Err(err) => last_err = err,
        }
    }

    Err(last_err)
}

/// Pull icon hrefs out of a page's `<link>` tags, best first: standard
/// `rel="icon"` / `rel="shortcut icon"` declarations, then
/// `apple-touch-icon` variants as a fallback. Pure string scan; tolerates
/// attribute order, quoting style, and case.
fn extract_icon_hrefs(html: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let mut standard = Vec::new();
    let mut touch = Vec::new();

    let mut at = 0;
    while let Some(found) = lower[at..].find("<link") {
        let start = at + found;
        let Some(close) = lower[start..].find('>') else {
            break;
        };

        let end = start + close;
        at = end + 1;

        let tag_lower = &lower[start..end];
        let tag_source = &html[start..end];
        let Some(rel) = attr_value(tag_lower, tag_lower, "rel") else {
            continue;
        };
        let Some(href) = attr_value(tag_lower, tag_source, "href") else {
            continue;
        };
        if href.is_empty() || href.starts_with("data:") {
            continue;
        }

        let tokens: Vec<&str> = rel.split_ascii_whitespace().collect();
        if tokens.contains(&"icon") {
            standard.push(href.to_string());
        } else if tokens.iter().any(|t| t.starts_with("apple-touch-icon")) {
            touch.push(href.to_string());
        }
    }

    standard.extend(touch);
    standard
}

/// Value of attribute `name` inside a single tag. `tag_lower` drives the
/// (case-insensitive) search; the value is sliced out of `tag_source` at the
/// same byte offsets, which line up because ASCII lowercasing preserves
/// length. Handles `name="v"`, `name='v'`, and unquoted `name=v`.
fn attr_value<'a>(tag_lower: &str, tag_source: &'a str, name: &str) -> Option<&'a str> {
    let bytes = tag_lower.as_bytes();
    let mut search = 0;
    loop {
        let idx = tag_lower[search..].find(name)? + search;
        search = idx + name.len();

        let boundary_before = idx > 0 && bytes[idx - 1].is_ascii_whitespace();
        let mut j = idx + name.len();
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if !boundary_before || j >= bytes.len() || bytes[j] != b'=' {
            continue;
        }
        j += 1;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if j >= bytes.len() {
            return None;
        }

        return match bytes[j] {
            quote @ (b'"' | b'\'') => {
                let value_start = j + 1;
                let close = tag_lower[value_start..].find(quote as char)? + value_start;
                Some(tag_source[value_start..close].trim())
            }
            _ => {
                let mut k = j;
                while k < bytes.len() && !bytes[k].is_ascii_whitespace() {
                    k += 1;
                }
                Some(tag_source[j..k].trim())
            }
        };
    }
}

/// Capture a favicon for the given tab. Best-effort: any failure logs a
/// warning and leaves `tab.favicon` untouched.
pub async fn capture_favicon(
    app: AppHandle,
    tab_id: TabId,
    workspace_id: WorkspaceId,
    page_url: Url,
) {
    let site = page_url.origin().ascii_serialization();
    if let Err(err) = capture_favicon_inner(app, tab_id, workspace_id, page_url).await {
        log::info!("favicon capture for {site} skipped: {err}");
    }
}

/// Ensure the favicon for `page_url`'s origin is cached on disk.
/// Returns the origin hash on success. Cache-hit fast path returns
/// without touching the network. Shared by [`capture_favicon`] (during
/// page-load) and [`prefetch_favicon`] (launchpad pre-warm).
async fn ensure_favicon_cached(
    app: &AppHandle,
    workspace_id: WorkspaceId,
    page_url: Url,
) -> Result<String, String> {
    let origin = canonical_origin(&page_url).ok_or_else(|| "non-http origin".to_string())?;
    let hash = origin_hash(&origin);
    let dir = cache_dir(app).map_err(|err| err.to_string())?;

    if favicon_path(&dir, &hash).is_some() {
        return Ok(hash);
    }

    let state = app.state::<AppState>();
    let proxy_port = state
        .ensure_proxy_for(workspace_id)
        .await
        .map_err(|err| format!("ensure_proxy: {err:?}"))?;
    let proxy_url = format!("http://127.0.0.1:{proxy_port}");

    let proxy = reqwest::Proxy::all(&proxy_url).map_err(|err| format!("proxy: {err}"))?;
    let client = reqwest::Client::builder()
        .timeout(FAVICON_TIMEOUT)
        .user_agent(wis_core::user_agent::compatibility_user_agent(
            wis_core::UaPlatform::host(),
        ))
        .proxy(proxy)
        .build()
        .map_err(|err| format!("build client: {err}"))?;

    // Try the conventional root path first, then fall back to the icons the
    // page itself declares.
    let favicon_url = format!("{origin}/favicon.ico");
    let (bytes, format) = match fetch_image(&client, &favicon_url).await {
        Ok(found) => found,
        Err(root_err) => {
            let page = icon_page(&origin).ok_or_else(|| format!("{root_err}; no site root"))?;
            icon_from_page_html(&client, &page)
                .await
                .map_err(|html_err| format!("{root_err}; html fallback: {html_err}"))?
        }
    };

    let target = dir.join(format!("{hash}.{}", format.extension()));
    std::fs::write(&target, &bytes).map_err(|err| format!("write {}: {err}", target.display()))?;
    log::info!(
        "cached favicon for {origin} ({} bytes -> {})",
        bytes.len(),
        target.display(),
    );

    if let Err(err) = app.emit(FAVICON_CACHED_EVENT, hash.clone()) {
        log::warn!("emit {FAVICON_CACHED_EVENT} failed: {err}");
    }

    Ok(hash)
}

async fn capture_favicon_inner(
    app: AppHandle,
    tab_id: TabId,
    workspace_id: WorkspaceId,
    page_url: Url,
) -> Result<(), String> {
    let hash = ensure_favicon_cached(&app, workspace_id, page_url).await?;
    attach_favicon_to_tab(&app, tab_id, &hash).await;
    Ok(())
}

/// Pre-warm the cache for `url`. Used by the launchpad to populate its
/// curated quicklink favicons on first boot without waiting for the
/// user to navigate to each site. Cache-hit fast path returns
/// immediately. Failures are logged but never raised.
///
/// Uses the currently-active workspace's proxy, so per-workspace
/// privacy mode + tracker rules apply.
#[tauri::command]
pub async fn prefetch_favicon(app: AppHandle, url: String) -> WisResult<Option<String>> {
    let parsed = match Url::parse(&url) {
        Ok(u) => u,
        Err(err) => {
            log::info!("prefetch_favicon: invalid url {url}: {err}");
            return Ok(None);
        }
    };

    let workspace_id = {
        let state = app.state::<AppState>();
        let browser = state.browser.lock().await;
        browser.active_workspace_id
    };

    match ensure_favicon_cached(&app, workspace_id, parsed).await {
        Ok(hash) => Ok(Some(hash)),
        Err(err) => {
            log::info!("prefetch_favicon for {url} skipped: {err}");
            Ok(None)
        }
    }
}

async fn attach_favicon_to_tab(app: &AppHandle, tab_id: TabId, hash: &str) {
    let state = app.state::<AppState>();
    let favicon = Some(FaviconRef {
        hash: hash.to_string(),
    });
    let result =
        crate::commands::mutate_unit_for_favicon(app, &state, tab_id, favicon.clone()).await;

    if let Err(err) = result {
        log::warn!("attach favicon for {tab_id} failed: {err}");
    }
}

/// Return a `data:` URL for the cached favicon, or `None` if the hash
/// isn't in the cache.
#[tauri::command]
pub async fn get_favicon_data_url(app: AppHandle, hash: String) -> WisResult<Option<String>> {
    let dir = cache_dir(&app)?;
    let Some(path) = favicon_path(&dir, &hash) else {
        return Ok(None);
    };
    let bytes = std::fs::read(&path).map_err(|err| WisError::InvalidInput {
        message: format!("read favicon {}: {err}", path.display()),
    })?;
    let format = sniff_format(&bytes).unwrap_or(Format::Png);
    let encoded = BASE64.encode(&bytes);

    Ok(Some(format!("data:{};base64,{encoded}", format.mime())))
}

#[tauri::command]
pub async fn favicon_origin_hash(url: String) -> WisResult<Option<String>> {
    let parsed = match Url::parse(&url) {
        Ok(u) => u,
        Err(_) => return Ok(None),
    };

    Ok(origin_hash_for_url(&parsed))
}

#[cfg(test)]
mod tests {
    use super::extract_icon_hrefs;

    #[test]
    fn the_icon_fallback_reads_the_site_root_never_the_visited_page() {
        let visited = url::Url::parse(
            "https://www.quora.com/email_optout/qemail_unsubscribe?token=abc&email_track_id=1",
        )
        .unwrap();
        let origin = super::canonical_origin(&visited).unwrap();
        assert_eq!(
            super::icon_page(&origin).map(String::from),
            Some("https://quora.com/".to_string())
        );
    }

    #[test]
    fn finds_standard_icon_links() {
        let html = r#"<head>
            <link rel="stylesheet" href="/app.css">
            <link rel="icon" href="/assets/fav.png" type="image/png">
            <link rel="shortcut icon" href="https://cdn.example.com/ico.ico">
        </head>"#;
        assert_eq!(
            extract_icon_hrefs(html),
            vec!["/assets/fav.png", "https://cdn.example.com/ico.ico"]
        );
    }

    #[test]
    fn tolerates_case_quoting_and_attribute_order() {
        let html = r#"<LINK HREF='/i.svg' REL=ICON><link rel=icon href=/plain.png>"#;
        assert_eq!(extract_icon_hrefs(html), vec!["/i.svg", "/plain.png"]);
    }

    #[test]
    fn apple_touch_icons_rank_after_standard_ones() {
        let html = r#"
            <link rel="apple-touch-icon" sizes="180x180" href="/touch.png">
            <link rel="icon" href="/fav.ico">
        "#;
        assert_eq!(extract_icon_hrefs(html), vec!["/fav.ico", "/touch.png"]);
    }

    #[test]
    fn skips_non_icon_rels_data_urls_and_mask_icons() {
        let html = r##"
            <link rel="preconnect" href="https://cdn.example.com">
            <link rel="mask-icon" href="/mask.svg" color="#000">
            <link rel="icon" href="data:image/png;base64,AAAA">
        "##;
        assert!(extract_icon_hrefs(html).is_empty());
    }

    #[test]
    fn href_preserves_original_case() {
        let html = r#"<link rel="icon" href="/Assets/Fav.PNG">"#;
        assert_eq!(extract_icon_hrefs(html), vec!["/Assets/Fav.PNG"]);
    }
}
