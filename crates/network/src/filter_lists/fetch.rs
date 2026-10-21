//! Background filter-list fetches (EasyPrivacy, EasyList) + on-disk cache.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use super::directives::{include_url, preprocess};
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
    let body = fetch_list(&client(Duration::from_secs(30))?, RESOURCES_URL).await?;
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
    let client = client(Duration::from_secs(20))?;
    let body = tokio::time::timeout(Duration::from_secs(120), with_includes(&client, url))
        .await
        .map_err(|_| NetworkError::FilterList(format!("{url} took over two minutes")))??;

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

/// The list with its kept branches and every list it includes, which share
/// its cache file and its trust. As in uBO, one missing include fails the
/// whole refresh, keeping the cached copy.
async fn with_includes(client: &reqwest::Client, url: &str) -> NetworkResult<String> {
    const MAX_LISTS: usize = 32;
    const MAX_BYTES: usize = 64 << 20;
    let root = fetch_list(client, url).await?;
    if !looks_like_adblock_list(&root) {
        return Err(NetworkError::FilterList(format!(
            "{url} did not look like an Adblock Plus filter list"
        )));
    }

    let root = preprocess(&root);
    let mut text = root.text;
    let mut fetched = HashSet::from([url.to_owned()]);
    let mut pending: VecDeque<String> = included(url, root.includes).collect();
    while let Some(sub) = pending.pop_front() {
        if !fetched.insert(sub.clone()) {
            continue;
        }
        if fetched.len() > MAX_LISTS {
            return Err(NetworkError::FilterList(format!(
                "{url} brings more than {MAX_LISTS} lists"
            )));
        }

        let part = fetch_list(client, &sub).await?;
        if is_web_page(&part) {
            return Err(NetworkError::FilterList(format!("{sub} is a web page")));
        }

        let part = preprocess(&part);
        text.push_str(&part.text);
        if text.len() > MAX_BYTES {
            return Err(NetworkError::FilterList(format!(
                "{url} comes to over {MAX_BYTES} bytes"
            )));
        }

        pending.extend(included(&sub, part.includes));
    }

    Ok(text)
}

fn included(parent: &str, paths: Vec<String>) -> impl Iterator<Item = String> + '_ {
    paths
        .into_iter()
        .filter_map(move |path| include_url(parent, &path))
}

fn client(timeout: Duration) -> NetworkResult<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(timeout)
        .https_only(true)
        .user_agent("wis-browser/0.1 (+https://app.myk.wis)")
        .build()
        .map_err(|e| NetworkError::FilterList(format!("client build: {e}")))
}

async fn fetch_list(client: &reqwest::Client, url: &str) -> NetworkResult<String> {
    const MAX_BYTES: usize = 16 << 20;
    let too_big = || NetworkError::FilterList(format!("{url} is over {MAX_BYTES} bytes"));
    let mut resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| NetworkError::FilterList(format!("fetch: {e}")))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(NetworkError::FilterList(format!(
            "fetch {url} returned HTTP {status}"
        )));
    }
    if resp
        .content_length()
        .is_some_and(|length| length > MAX_BYTES as u64)
    {
        return Err(too_big());
    }

    let mut body = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| NetworkError::FilterList(format!("read body: {e}")))?
    {
        if body.len() + chunk.len() > MAX_BYTES {
            return Err(too_big());
        }
        body.extend_from_slice(&chunk);
    }

    Ok(String::from_utf8_lossy(&body).into_owned())
}

/// uBO's only test of an included list.
fn is_web_page(body: &str) -> bool {
    let body = body.trim();
    body.starts_with('<') && body.ends_with('>')
}

fn looks_like_adblock_list(body: &str) -> bool {
    // EasyPrivacy starts with a `[Adblock Plus 2.0]` header followed by `!`
    // comment lines.
    let first = body.lines().next().unwrap_or("").trim();
    !body.is_empty() && (first.starts_with('!') || first.starts_with('['))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    /// Serves `lists` over plain HTTP and records each path asked for.
    async fn serve(lists: HashMap<String, String>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let asked = Arc::new(Mutex::new(Vec::new()));
        let log = asked.clone();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut request = vec![0; 4096];
                let read = stream.read(&mut request).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&request[..read]).into_owned();
                let path = request.split_whitespace().nth(1).unwrap_or("").to_owned();
                log.lock().unwrap().push(path.clone());
                let response = match lists.get(&path) {
                    Some(body) => format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    ),
                    None => {
                        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .into()
                    }
                };
                stream.write_all(response.as_bytes()).await.ok();
            }
        });
        (origin, asked)
    }

    fn lists(entries: &[(&str, &str)]) -> HashMap<String, String> {
        entries
            .iter()
            .map(|(path, body)| (path.to_string(), body.to_string()))
            .collect()
    }

    fn client() -> reqwest::Client {
        reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap()
    }

    #[tokio::test]
    async fn a_list_brings_its_kept_includes_once_each() {
        let (origin, asked) = serve(lists(&[
            (
                "/lists/root.txt",
                "! root\n||root.test^\n!#include sub/a.txt\n!#if env_mobile\n!#include mobile.txt\n!#endif\n!#include sub/a.txt\n!#include ../up.txt\n",
            ),
            ("/lists/sub/a.txt", "! a\n||a.test^\n!#include b.txt\n"),
            ("/lists/sub/b.txt", "! b\n||b.test^\n!#include a.txt\n"),
        ]))
        .await;

        let list = with_includes(&client(), &format!("{origin}/lists/root.txt"))
            .await
            .unwrap();
        for rule in ["||root.test^", "||a.test^", "||b.test^"] {
            assert_eq!(list.matches(rule).count(), 1, "{rule} in {list}");
        }
        assert!(!list.contains("!#include"));
        assert_eq!(
            *asked.lock().unwrap(),
            ["/lists/root.txt", "/lists/sub/a.txt", "/lists/sub/b.txt"]
        );
    }

    /// uBO refuses only an HTML page; an include need not start with a
    /// comment, and may be empty.
    #[tokio::test]
    async fn an_include_is_refused_only_when_it_is_a_web_page() {
        let (origin, _) = serve(lists(&[
            (
                "/root.txt",
                "! root\n!#include empty.txt\n!#include bare.txt\n",
            ),
            ("/empty.txt", ""),
            ("/bare.txt", "\n||bare.test^\n"),
            ("/page.txt", "! page\n!#include page.html\n"),
            ("/page.html", "<html><body>Not found</body></html>"),
        ]))
        .await;

        let list = with_includes(&client(), &format!("{origin}/root.txt"))
            .await
            .unwrap();
        assert!(list.contains("||bare.test^"));
        let err = with_includes(&client(), &format!("{origin}/page.txt"))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("is a web page"), "{err}");
    }

    #[tokio::test]
    async fn one_missing_include_fails_the_whole_list() {
        let (origin, _) = serve(lists(&[(
            "/lists/root.txt",
            "! root\n||root.test^\n!#include gone.txt\n",
        )]))
        .await;

        let err = with_includes(&client(), &format!("{origin}/lists/root.txt"))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("404"), "{err}");
    }

    #[tokio::test]
    async fn a_list_may_bring_at_most_32_lists() {
        let mut entries: Vec<(String, String)> = (0..40)
            .map(|n| (format!("/l{n}.txt"), "! sub\n".to_owned()))
            .collect();
        let root: String = std::iter::once("! root\n".to_owned())
            .chain((0..40).map(|n| format!("!#include l{n}.txt\n")))
            .collect();
        entries.push(("/root.txt".to_owned(), root));
        let (origin, asked) = serve(entries.into_iter().collect()).await;

        let err = with_includes(&client(), &format!("{origin}/root.txt"))
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("brings more than 32 lists"),
            "{err}"
        );
        assert_eq!(asked.lock().unwrap().len(), 32);
    }
}
