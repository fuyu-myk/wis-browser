//! Tying an extension installed from a folder or `.zip` to its Chrome Web
//! Store listing, so it updates like one from the store: linking a listing the
//! user confirms, and the daily search that suggests one. A listing counts
//! when its package has the same name and, at the same version, the same files.

use serde::Serialize;
use tauri::{AppHandle, State};
use wis_core::WisResult;

use crate::state::AppState;

#[cfg(not(target_os = "macos"))]
use super::unavailable;
#[cfg(target_os = "macos")]
use super::{host, install, store, updates};
#[cfg(target_os = "macos")]
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use tauri::Manager;
#[cfg(target_os = "macos")]
use wis_core::extension_listing::{
    listing_slug, listings, same_name, wants_search, with_declined_listing, with_store_link,
    with_store_searched, with_store_suggestion, StoreSuggestion,
};
#[cfg(target_os = "macos")]
use wis_core::extensions::{self, InstalledExtension};
#[cfg(target_os = "macos")]
use wis_core::WisError;

#[cfg(target_os = "macos")]
static SEARCHING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Search results looked at for each extension.
#[cfg(target_os = "macos")]
const CANDIDATES: usize = 3;
#[cfg(target_os = "macos")]
const SEARCH_URL: &str = "https://chromewebstore.google.com/search/";

/// What linking did: tied the listing, and maybe updated from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Linked {
    Current,
    Updated,
    /// Its update asks for more and awaits approval in Settings.
    Waiting,
}

/// Ties an extension from a folder or `.zip` to the listing `store_id`, once
/// the listing's package proves to be the same extension, and updates it.
#[tauri::command]
pub async fn extension_link_store(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    store_id: String,
) -> WisResult<Linked> {
    #[cfg(target_os = "macos")]
    {
        super::require_support()?;
        if !extensions::is_store_id(&store_id) {
            return Err(refuse("not a Chrome Web Store id"));
        }

        let _checking = updates::CHECKING.lock().await;
        let ext = installed(&state, &id).await?;
        if ext.store_id.is_some() {
            return Err(refuse(
                "that extension already gets its updates from the store",
            ));
        }

        let port = updates::proxy_for(&app, &ext)
            .await?
            .ok_or_else(|| refuse("no regular workspace to ask the store from"))?;
        let client = store::client(port)?;
        let installed_facts = install::read_facts(&app, root_of(&app, &id)?).await?;
        let listed =
            listed_facts(&app, &client, &store_id, |facts, _| Ok(facts.name.clone())).await?;
        if !same_name(&listed, &installed_facts.name) {
            return Err(refuse(format!(
                "this listing's extension is called {listed}, not {}",
                ext.name
            )));
        }

        let (link_id, link_store) = (id.clone(), store_id.clone());
        install::change(app.clone(), state, move |list| {
            with_store_link(list, &link_id, &link_store)
        })
        .await?;
        let linked = InstalledExtension {
            store_id: Some(store_id),
            ..ext
        };

        Ok(match updates::update(&app, &linked, None).await {
            Ok(updates::Outcome::Current) => Linked::Current,
            Ok(updates::Outcome::Updated) => Linked::Updated,
            Ok(updates::Outcome::Waiting) => Linked::Waiting,
            Err(err) => {
                log::warn!("{} was linked but not updated: {err:?}", linked.name);
                Linked::Current
            }
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, state, id, store_id);
        Err(unavailable())
    }
}

/// Remembers that the listing `store_id` is not `id`'s.
#[tauri::command]
pub async fn extension_decline_listing(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    store_id: String,
) -> WisResult<()> {
    #[cfg(target_os = "macos")]
    {
        super::require_support()?;
        install::change(app, state, move |list| {
            with_declined_listing(list, &id, &store_id)
        })
        .await
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, state, id, store_id);
        Err(unavailable())
    }
}

#[cfg(target_os = "macos")]
fn refuse(message: impl Into<String>) -> WisError {
    WisError::InvalidInput {
        message: message.into(),
    }
}

#[cfg(target_os = "macos")]
async fn installed(state: &AppState, id: &str) -> WisResult<InstalledExtension> {
    state
        .browser
        .lock()
        .await
        .settings
        .extensions
        .iter()
        .find(|e| e.id == id)
        .cloned()
        .ok_or_else(|| refuse("that extension is not installed"))
}

#[cfg(target_os = "macos")]
fn root_of(app: &AppHandle, id: &str) -> WisResult<PathBuf> {
    Ok(install::extensions_dir(app)?.join(id))
}

/// Downloads and checks the listing's package, and answers `judge` from what
/// WebKit makes of it and where it was unpacked; the files go afterwards.
#[cfg(target_os = "macos")]
async fn listed_facts<T: Send + 'static>(
    app: &AppHandle,
    client: &reqwest::Client,
    store_id: &str,
    judge: impl FnOnce(&host::Facts, &Path) -> WisResult<T> + Send + 'static,
) -> WisResult<T> {
    let url = store::store_url(&[("response", "redirect")], store_id, None)?;
    let package = store::fetch(client, url).await?;
    let archive = store::verified(package, store_id).await?;
    let (_, unpacked) =
        install::unpack(app, install::Source::Store(archive, store_id.to_string())).await?;
    let root = unpacked.root.clone();
    let judged = match install::read_facts(app, root.clone()).await {
        Ok(facts) => tauri::async_runtime::spawn_blocking(move || judge(&facts, &root))
            .await
            .map_err(|err| refuse(err.to_string()))
            .and_then(|judged| judged),
        Err(err) => Err(err),
    };
    install::discard_dir(unpacked.dir).await;
    judged
}

/// Looks for the listings of extensions not tied to one, and keeps the best
/// match as a suggestion for Settings.
#[cfg(target_os = "macos")]
pub(super) async fn suggest(app: &AppHandle, unlinked: &[InstalledExtension]) {
    let Ok(_searching) = SEARCHING.try_lock() else {
        return;
    };
    let allowed = app
        .state::<AppState>()
        .browser
        .lock()
        .await
        .settings
        .extension_store_search;
    if !allowed {
        return;
    }

    let now = updates::now();
    for ext in unlinked.iter().filter(|e| wants_search(e, now)) {
        let found = search(app, ext).await.unwrap_or_else(|err| {
            log::warn!("the store was not searched for {}: {err:?}", ext.name);
            None
        });
        let (id, name) = (ext.id.clone(), ext.name.clone());
        let saved = install::change(app.clone(), app.state::<AppState>(), move |list| {
            let list = with_store_searched(list, &id, now)?;
            with_store_suggestion(&list, &id, found)
        })
        .await;
        if let Err(err) = saved {
            log::warn!("the store search for {name} was not saved: {err:?}");
        }
    }
}

/// What a listing's package says about an installed extension.
#[cfg(target_os = "macos")]
enum Verdict {
    /// Another extension.
    Other,
    /// The same name: a match, at another version or with the same files.
    Same { same_files: bool },
    /// The same name and version but with possibly untrusted different files.
    Differs,
}

#[cfg(target_os = "macos")]
async fn search(app: &AppHandle, ext: &InstalledExtension) -> WisResult<Option<StoreSuggestion>> {
    let Some(slug) = listing_slug(&ext.name) else {
        return Ok(None);
    };
    let Some(port) = updates::proxy_for(app, ext).await? else {
        return Ok(None);
    };
    let client = store::client(port)?;
    let mut url = reqwest::Url::parse(SEARCH_URL).map_err(|err| refuse(err.to_string()))?;
    url.path_segments_mut()
        .map_err(|()| refuse("store search address"))?
        .pop_if_empty()
        .push(&ext.name);
    let page = store::fetch(&client, url).await?;
    let installed_root = root_of(app, &ext.id)?;
    let installed = install::read_facts(app, installed_root.clone()).await?;
    let taken: Vec<String> = {
        let state = app.state::<AppState>();
        let browser = state.browser.lock().await;
        browser
            .settings
            .extensions
            .iter()
            .filter_map(|e| e.store_id.clone())
            .collect()
    };

    let candidates = listings(&String::from_utf8_lossy(&page))
        .into_iter()
        .filter(|l| l.slug.as_deref() == Some(slug.as_str()))
        .filter(|l| !ext.declined_listings.contains(&l.store_id) && !taken.contains(&l.store_id))
        .take(CANDIDATES);
    for listing in candidates {
        let (name, version, root) = (
            installed.name.clone(),
            installed.version.clone(),
            installed_root.clone(),
        );
        let judged = listed_facts(app, &client, &listing.store_id, move |facts, listed| {
            if !same_name(&facts.name, &name) {
                return Ok(Verdict::Other);
            }
            if facts.version != version {
                return Ok(Verdict::Same { same_files: false });
            }

            let same = same_files(&root, listed).map_err(|err| refuse(err.to_string()))?;
            Ok(if same {
                Verdict::Same { same_files: true }
            } else {
                Verdict::Differs
            })
        })
        .await;
        match judged {
            Ok(Verdict::Other) => {}
            Ok(Verdict::Same { same_files }) => {
                return Ok(Some(StoreSuggestion {
                    store_id: listing.store_id,
                    same_files,
                }))
            }
            Ok(Verdict::Differs) => return Ok(None),
            Err(err) => log::warn!("listing {} was not checked: {err:?}", listing.store_id),
        }
    }

    Ok(None)
}

/// Manifest keys the store adds to the packages it serves.
#[cfg(target_os = "macos")]
const STORE_KEYS: [&str; 3] = ["update_url", "key", "differential_fingerprint"];

/// Files the store adds or rewrites, left out when comparing.
#[cfg(target_os = "macos")]
fn left_out(relative: &Path) -> bool {
    relative == Path::new("manifest.json")
        || relative.starts_with("_metadata")
        || relative.file_name().is_some_and(|n| n == ".DS_Store")
}

#[cfg(target_os = "macos")]
fn file_hashes(root: &Path) -> std::io::Result<std::collections::BTreeMap<PathBuf, Vec<u8>>> {
    use sha2::Digest;

    let mut found = std::collections::BTreeMap::new();
    let mut folders = vec![PathBuf::new()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(root.join(&folder))? {
            let entry = entry?;
            let relative = folder.join(entry.file_name());
            if left_out(&relative) {
                continue;
            }

            let kind = entry.file_type()?;
            if kind.is_dir() {
                folders.push(relative);
            } else if kind.is_file() {
                let mut hasher = sha2::Sha256::new();
                std::io::copy(&mut std::fs::File::open(root.join(&relative))?, &mut hasher)?;
                found.insert(relative, hasher.finalize().to_vec());
            }
        }
    }

    Ok(found)
}

#[cfg(target_os = "macos")]
fn manifest_without_store_keys(root: &Path) -> std::io::Result<serde_json::Value> {
    let text = std::fs::read(root.join("manifest.json"))?;
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&text).map_err(std::io::Error::other)?;
    if let Some(fields) = manifest.as_object_mut() {
        for key in STORE_KEYS {
            fields.remove(key);
        }
    }
    Ok(manifest)
}

/// Whether two extension folders hold the same extension, apart from what
/// the store adds to a package.
#[cfg(target_os = "macos")]
fn same_files(a: &Path, b: &Path) -> std::io::Result<bool> {
    Ok(
        manifest_without_store_keys(a)? == manifest_without_store_keys(b)?
            && file_hashes(a)? == file_hashes(b)?,
    )
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            let dir = std::env::temp_dir().join(format!("wis-linking-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn write(&self, relative: &str, text: &str) {
            let path = self.0.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn build(manifest: &str) -> Scratch {
        let scratch = Scratch::new();
        scratch.write("manifest.json", manifest);
        scratch.write("background/index.js", "start()");
        scratch.write("ui/popup/index.html", "<p>hi</p>");
        scratch
    }

    #[test]
    fn the_store_build_of_the_same_files_matches() {
        let zip = build(r#"{"name":"Dark Reader","version":"4.9.133"}"#);
        let store = build(r#"{"version":"4.9.133","update_url":"x","name":"Dark Reader"}"#);
        store.write("_metadata/verified_contents.json", "[]");
        zip.write(".DS_Store", "finder");
        assert!(same_files(&zip.0, &store.0).unwrap());
    }

    #[test]
    fn a_manifest_that_asks_for_more_does_not_match() {
        let zip = build(r#"{"name":"Dark Reader","permissions":["storage"]}"#);
        let store = build(r#"{"name":"Dark Reader","permissions":["storage","cookies"]}"#);
        assert!(!same_files(&zip.0, &store.0).unwrap());
    }

    #[test]
    fn a_changed_extra_or_missing_file_does_not_match() {
        let zip = build("{}");
        let changed = build("{}");
        changed.write("background/index.js", "start(); track()");
        assert!(!same_files(&zip.0, &changed.0).unwrap());

        let extra = build("{}");
        extra.write("ads.js", "show()");
        assert!(!same_files(&zip.0, &extra.0).unwrap());
        assert!(!same_files(&extra.0, &zip.0).unwrap());
    }
}
