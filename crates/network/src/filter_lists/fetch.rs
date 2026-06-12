//! Background filter-list fetches (EasyPrivacy, EasyList) + on-disk cache.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::{NetworkError, NetworkResult};

/// Upstream EasyPrivacy URL.
pub const EASYPRIVACY_URL: &str = "https://easylist.to/easylist/easyprivacy.txt";

/// Upstream EasyList URL (ad rules incl. the cosmetic element-hiding bulk).
pub const EASYLIST_URL: &str = "https://easylist.to/easylist/easylist.txt";

/// Filename inside `cache_dir` that holds the latest fetched list.
pub const EASYPRIVACY_FILENAME: &str = "easyprivacy.txt";

/// Filename inside `cache_dir` that holds the latest fetched EasyList.
pub const EASYLIST_FILENAME: &str = "easylist.txt";

/// How long a cached list is considered fresh before we attempt a refresh.
pub const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

pub fn easyprivacy_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join(EASYPRIVACY_FILENAME)
}

pub fn easylist_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join(EASYLIST_FILENAME)
}

/// True when the on-disk EasyPrivacy cache is missing or older than
/// [`CACHE_TTL`].
pub fn cache_is_stale(cache_dir: &Path) -> bool {
    file_is_stale(&easyprivacy_path(cache_dir))
}

/// True when the on-disk EasyList cache is missing or older than
/// [`CACHE_TTL`].
pub fn easylist_cache_is_stale(cache_dir: &Path) -> bool {
    file_is_stale(&easylist_path(cache_dir))
}

fn file_is_stale(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return true;
    };
    match meta.modified().ok().and_then(|t| t.elapsed().ok()) {
        Some(age) => age >= CACHE_TTL,
        None => true,
    }
}

/// GET the EasyPrivacy list, validate it looks like Adblock-Plus syntax, and
/// write it to `<cache_dir>/easyprivacy.txt`. Returns the body so the caller
/// can hot-swap it without re-reading from disk.
pub async fn refresh_easyprivacy(cache_dir: &Path) -> NetworkResult<String> {
    refresh_list(EASYPRIVACY_URL, &easyprivacy_path(cache_dir), cache_dir).await
}

/// GET the EasyList list (cosmetic + ad network rules; consumed by the
/// engine only, never by the WKContentRuleList compiler, whose rule cap it
/// would exceed) and cache it as `<cache_dir>/easylist.txt`.
pub async fn refresh_easylist(cache_dir: &Path) -> NetworkResult<String> {
    refresh_list(EASYLIST_URL, &easylist_path(cache_dir), cache_dir).await
}

async fn refresh_list(url: &str, path: &Path, cache_dir: &Path) -> NetworkResult<String> {
    tracing::info!("filter_lists: fetching {url}");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent("wis-browser/0.1 (+https://app.myk.wis)")
        .build()
        .map_err(|e| NetworkError::FilterList(format!("client build: {e}")))?;

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| NetworkError::FilterList(format!("fetch: {e}")))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(NetworkError::FilterList(format!(
            "fetch returned HTTP {status}"
        )));
    }
    let body = resp
        .text()
        .await
        .map_err(|e| NetworkError::FilterList(format!("read body: {e}")))?;

    if !looks_like_adblock_list(&body) {
        return Err(NetworkError::FilterList(
            "response did not look like an Adblock Plus filter list".into(),
        ));
    }

    std::fs::create_dir_all(cache_dir)
        .map_err(|e| NetworkError::FilterList(format!("create cache dir: {e}")))?;
    std::fs::write(path, &body)
        .map_err(|e| NetworkError::FilterList(format!("write cache: {e}")))?;
    tracing::info!(
        "filter_lists: cached {} bytes at {}",
        body.len(),
        path.display()
    );

    Ok(body)
}

fn looks_like_adblock_list(body: &str) -> bool {
    // EasyPrivacy starts with a `[Adblock Plus 2.0]` header followed by `!`
    // comment lines.
    let first = body.lines().next().unwrap_or("").trim();
    !body.is_empty() && (first.starts_with('!') || first.starts_with('['))
}
