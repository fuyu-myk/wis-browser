//! Background filter-list fetches (EasyPrivacy, EasyList) + on-disk cache.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
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

/// uBO's scriptlet library as Ghostery's adblocker publishes it (see
/// [`crate::scriptlet_library`]), which expands `+js(...)` rules.
pub const RESOURCES_URL: &str =
    "https://raw.githubusercontent.com/ghostery/adblocker/master/packages/adblocker/assets/ublock-origin/resources.json";

pub const RESOURCES_FILENAME: &str = "ubo-scriptlets.json";
/// Brave's library, which held none of uBO's scriptlets.
const BRAVE_RESOURCES_FILENAME: &str = "resources.json";

/// Scriptlet resources change slower than filter lists.
pub const RESOURCES_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

pub fn easyprivacy_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join(EASYPRIVACY_FILENAME)
}

pub fn easylist_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join(EASYLIST_FILENAME)
}

pub fn resources_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join(RESOURCES_FILENAME)
}

/// Cache file for a user filter-list subscription, keyed by URL hash so any
/// URL maps to a stable, traversal-safe filename.
pub fn subscription_path(cache_dir: &Path, url: &str) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut hasher);
    cache_dir.join(format!("sub-{:016x}.txt", hasher.finish()))
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

/// True when a subscription's cache is missing or older than [`CACHE_TTL`].
pub fn subscription_is_stale(cache_dir: &Path, url: &str) -> bool {
    file_is_stale(&subscription_path(cache_dir, url))
}

/// True when the scriptlet resource cache is missing or older than
/// [`RESOURCES_TTL`].
pub fn resources_are_stale(cache_dir: &Path) -> bool {
    file_is_stale_with_ttl(&resources_path(cache_dir), RESOURCES_TTL)
}

fn file_is_stale(path: &Path) -> bool {
    file_is_stale_with_ttl(path, CACHE_TTL)
}

fn file_is_stale_with_ttl(path: &Path, ttl: Duration) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return true;
    };
    match meta.modified().ok().and_then(|t| t.elapsed().ok()) {
        Some(age) => age >= ttl,
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

/// GET a user subscription list (https only) and cache it under its URL
/// hash. The body must look like an Adblock Plus list.
pub async fn refresh_subscription(cache_dir: &Path, url: &str) -> NetworkResult<String> {
    if !url.starts_with("https://") {
        return Err(NetworkError::FilterList(
            "subscription URLs must be https".into(),
        ));
    }

    refresh_list(url, &subscription_path(cache_dir, url), cache_dir).await
}

/// GET the scriptlet library and cache it as `<cache_dir>/ubo-scriptlets.json`.
pub async fn refresh_resources(cache_dir: &Path) -> NetworkResult<String> {
    tracing::info!("filter_lists: fetching {RESOURCES_URL}");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("wis-browser/0.1 (+https://app.myk.wis)")
        .build()
        .map_err(|e| NetworkError::FilterList(format!("client build: {e}")))?;
    let resp = client
        .get(RESOURCES_URL)
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
    crate::scriptlet_library::resources(&body)?;

    std::fs::create_dir_all(cache_dir)
        .map_err(|e| NetworkError::FilterList(format!("create cache dir: {e}")))?;
    let path = resources_path(cache_dir);
    write_cache(&path, &body)?;
    match std::fs::remove_file(cache_dir.join(BRAVE_RESOURCES_FILENAME)) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
            tracing::warn!("filter_lists: removing Brave's old library failed: {err}");
        }
        _ => {}
    }
    tracing::info!(
        "filter_lists: cached {} bytes at {}",
        body.len(),
        path.display()
    );

    Ok(body)
}

fn write_cache(path: &Path, body: &str) -> NetworkResult<()> {
    static PARTIALS: AtomicU64 = AtomicU64::new(0);
    let partial = path.with_extension(format!(
        "partial{}",
        PARTIALS.fetch_add(1, Ordering::Relaxed)
    ));
    let written = std::fs::write(&partial, body).and_then(|()| std::fs::rename(&partial, path));
    if let Err(err) = written {
        match std::fs::remove_file(&partial) {
            Err(left) if left.kind() != std::io::ErrorKind::NotFound => {
                tracing::warn!("filter_lists: {} left behind: {left}", partial.display());
            }
            _ => {}
        }
        return Err(NetworkError::FilterList(format!("write cache: {err}")));
    }
    Ok(())
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
    write_cache(path, &body)?;
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
