//! Keeping Chrome Web Store extensions up to date. Updates are checked about
//! once a day over a workspace proxy, and a newer package is checked and put
//! in place under the same id, unless it requires more permissions than the
//! version installed. The latter waits in Settings for approval.

use serde::Serialize;
#[cfg(target_os = "macos")]
use tauri::Manager;
use tauri::{AppHandle, State};
#[cfg(target_os = "macos")]
use wis_core::extensions::installed::{self, InstalledExtension};
#[cfg(target_os = "macos")]
use wis_core::extensions::updates::{
    added_abilities, is_newer, wants_more, with_pending_update, PendingUpdate, Reach,
};
#[cfg(target_os = "macos")]
use wis_core::WisError;
use wis_core::WisResult;

#[cfg(not(target_os = "macos"))]
use super::unavailable;
#[cfg(target_os = "macos")]
use super::{host, install, linking, store};
use crate::state::AppState;

#[cfg(target_os = "macos")]
use quick_xml::events::{BytesStart, Event};
#[cfg(target_os = "macos")]
use quick_xml::Reader;

#[cfg(target_os = "macos")]
const CHECK_EVERY: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// A version the store offers, and where to get it.
#[cfg(target_os = "macos")]
#[derive(Debug, PartialEq, Eq)]
struct Offered {
    version: String,
    codebase: String,
    sha256: Option<String>,
}

#[cfg(target_os = "macos")]
fn attribute(element: &BytesStart, name: &[u8]) -> Option<String> {
    element
        .attributes()
        .flatten()
        .find(|a| a.key.as_ref() == name)?
        .unescape_value()
        .ok()
        .map(|value| value.into_owned())
}

/// What the store's update answer offers for `store_id`, if anything.
#[cfg(target_os = "macos")]
fn offered(xml: &str, store_id: &str) -> Option<Offered> {
    let mut reader = Reader::from_str(xml);
    let mut in_app = false;
    loop {
        match reader.read_event().ok()? {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"app" => {
                in_app = attribute(&e, b"appid").as_deref() == Some(store_id);
            }
            Event::End(e) if e.local_name().as_ref() == b"app" => in_app = false,
            Event::Start(e) | Event::Empty(e)
                if in_app && e.local_name().as_ref() == b"updatecheck" =>
            {
                if attribute(&e, b"status").as_deref() != Some("ok") {
                    return None;
                }
                return Some(Offered {
                    version: attribute(&e, b"version")?,
                    codebase: attribute(&e, b"codebase")?,
                    sha256: attribute(&e, b"hash_sha256"),
                });
            }
            Event::Eof => return None,
            _ => {}
        }
    }
}

/// Whether a day has passed since the last check, or the clock went back.
#[cfg(target_os = "macos")]
fn is_due(last: Option<u64>, now: u64) -> bool {
    last.is_none_or(|last| last > now || now - last >= CHECK_EVERY.as_secs())
}

/// What a check did, by extension name.
#[derive(Debug, Clone, Default, Serialize)]
pub struct UpdateSummary {
    pub updated: Vec<String>,
    pub waiting: Vec<String>,
    pub failed: Vec<String>,
}

/// Checks every Chrome Web Store extension for an update now.
#[tauri::command]
pub async fn extension_check_updates(app: AppHandle) -> WisResult<UpdateSummary> {
    #[cfg(target_os = "macos")]
    {
        super::require_support()?;
        Ok(check_all(&app).await)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Err(unavailable())
    }
}

/// Installs the update of `id` that waited for approval.
#[tauri::command]
pub async fn extension_approve_update(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> WisResult<()> {
    #[cfg(target_os = "macos")]
    {
        super::require_support()?;
        let _guard = CHECKING.lock().await;
        let ext = state
            .browser
            .lock()
            .await
            .settings
            .extensions
            .iter()
            .find(|e| e.id == id)
            .cloned()
            .ok_or_else(|| refuse("that extension is not installed"))?;
        let approved = ext
            .pending_update
            .as_ref()
            .map(|p| p.reach.clone())
            .ok_or_else(|| refuse("that extension has no update waiting"))?;
        match update(&app, &ext, Some(&approved)).await? {
            Outcome::Updated | Outcome::Current => Ok(()),
            Outcome::Waiting => Err(refuse(
                "a newer update asks for more than the one you approved; look again below",
            )),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, state, id);
        Err(unavailable())
    }
}

#[cfg(target_os = "macos")]
const FIRST_CHECK_AFTER: std::time::Duration = std::time::Duration::from_secs(90);
#[cfg(target_os = "macos")]
const WAKE_EVERY: std::time::Duration = std::time::Duration::from_secs(60 * 60);
#[cfg(target_os = "macos")]
const STAMP: &str = "last-update-check";

#[cfg(target_os = "macos")]
pub(super) static CHECKING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[cfg(target_os = "macos")]
fn refuse(message: impl Into<String>) -> WisError {
    WisError::InvalidInput {
        message: message.into(),
    }
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Outcome {
    Current,
    Updated,
    Waiting,
}

/// Checks for updates shortly after launch and then about once a day.
#[cfg(target_os = "macos")]
pub(crate) fn schedule(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK_AFTER).await;
        loop {
            if due(&app).await {
                check_all(&app).await;
            }
            tokio::time::sleep(WAKE_EVERY).await;
        }
    });
}

#[cfg(target_os = "macos")]
pub(super) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(target_os = "macos")]
async fn due(app: &AppHandle) -> bool {
    let Some(stamp) = install::extensions_path(app).map(|dir| dir.join(STAMP)) else {
        return false;
    };
    let last = tokio::fs::read_to_string(stamp)
        .await
        .ok()
        .and_then(|s| s.trim().parse().ok());
    is_due(last, now())
}

#[cfg(target_os = "macos")]
async fn check_all(app: &AppHandle) -> UpdateSummary {
    let _guard = CHECKING.lock().await;
    let mut summary = UpdateSummary::default();
    let (linked, unlinked): (Vec<InstalledExtension>, Vec<InstalledExtension>) = {
        let state = app.state::<AppState>();
        let browser = state.browser.lock().await;
        browser
            .settings
            .extensions
            .iter()
            .cloned()
            .partition(|e| e.store_id.is_some())
    };
    if linked.is_empty() && unlinked.is_empty() {
        return summary;
    }

    for ext in &linked {
        let name = ext.name.clone();
        match update(app, ext, None).await {
            Ok(Outcome::Current) => {}
            Ok(Outcome::Updated) => summary.updated.push(name),
            Ok(Outcome::Waiting) => summary.waiting.push(name),
            Err(err) => {
                log::warn!("{name} was not updated: {err:?}");
                summary.failed.push(name);
            }
        }
    }

    if linked.is_empty() || summary.failed.len() < linked.len() {
        if let Some(stamp) = install::extensions_path(app).map(|dir| dir.join(STAMP)) {
            if let Err(err) = tokio::fs::write(stamp, now().to_string()).await {
                log::warn!("the extension update check was not recorded: {err}");
            }
        }
    }
    drop(_guard);
    linking::suggest(app, &unlinked).await;

    summary
}

/// The proxy to ask through: a regular workspace the extension runs in, or
/// any regular workspace.
#[cfg(target_os = "macos")]
pub(super) async fn proxy_for(app: &AppHandle, ext: &InstalledExtension) -> WisResult<Option<u16>> {
    let state = app.state::<AppState>();
    let workspace = {
        let browser = state.browser.lock().await;
        let regular = || browser.workspaces.iter().filter(|w| !w.ephemeral);
        regular()
            .find(|w| installed::runs_in(ext, w))
            .or_else(|| regular().next())
            .map(|w| w.id)
    };
    let Some(workspace) = workspace else {
        return Ok(None);
    };
    state
        .ensure_proxy_for(workspace)
        .await
        .map(Some)
        .map_err(WisError::from)
}

#[cfg(target_os = "macos")]
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Updates one store extension when the store has a newer version. One that
/// requires more permissions than the installed version waits, unless
/// `approved` covers it.
#[cfg(target_os = "macos")]
pub(super) async fn update(
    app: &AppHandle,
    ext: &InstalledExtension,
    approved: Option<&Reach>,
) -> WisResult<Outcome> {
    let Some(store_id) = ext.store_id.clone() else {
        return Ok(Outcome::Current);
    };
    let Some(proxy_port) = proxy_for(app, ext).await? else {
        return Ok(Outcome::Current);
    };
    let client = store::client(proxy_port)?;
    let answer = store::fetch(
        &client,
        store::store_url(&[], &store_id, Some(&ext.version))?,
    )
    .await?;
    let offer = offered(&String::from_utf8_lossy(&answer), &store_id)
        .filter(|offer| is_newer(&offer.version, &ext.version));
    let Some(offer) = offer else {
        if ext.pending_update.is_some() {
            settle(app, ext, &ext.version).await?;
        }
        return Ok(Outcome::Current);
    };

    let codebase = reqwest::Url::parse(&offer.codebase)
        .ok()
        .filter(|url| url.scheme() == "https")
        .ok_or_else(|| refuse("the store offered an update from an unsafe address"))?;
    let package = store::fetch(&client, codebase).await?;
    if offer
        .sha256
        .as_deref()
        .is_some_and(|expected| !expected.eq_ignore_ascii_case(&sha256_hex(&package)))
    {
        return Err(refuse("the update did not arrive whole"));
    }

    let archive = store::verified(package, &store_id).await?;
    let (_, unpacked) = install::unpack(app, install::Source::Store(archive, store_id)).await?;

    let installed_root = install::extensions_dir(app)?.join(&ext.id);
    let facts = match (
        install::read_facts(app, unpacked.root.clone()).await,
        install::read_facts(app, installed_root).await,
    ) {
        (Ok(next), Ok(installed)) => (next, installed),
        (Err(err), _) | (_, Err(err)) => {
            install::discard_dir(unpacked.dir).await;
            return Err(err);
        }
    };
    let (next, installed) = facts;
    if !is_newer(&next.version, &installed.version) {
        install::discard_dir(unpacked.dir).await;
        if ext.version != installed.version || ext.pending_update.is_some() {
            settle(app, ext, &installed.version).await?;
        }
        return Ok(Outcome::Current);
    }

    let reach = |facts: &host::Facts| Reach {
        permissions: facts.recognised.clone(),
        patterns: facts.patterns.clone(),
    };
    let (before, after) = (reach(&installed), reach(&next));
    let covered = approved.is_some_and(|approved| !wants_more(approved, &after));
    if wants_more(&before, &after) && !covered {
        install::discard_dir(unpacked.dir).await;
        let pending = PendingUpdate {
            version: next.version.clone(),
            abilities: added_abilities(&before, &after),
            reach: after,
        };
        if ext.pending_update.as_ref() != Some(&pending) {
            let state = app.state::<AppState>();
            let id = ext.id.clone();
            install::change(app.clone(), state, move |list| {
                with_pending_update(list, &id, Some(pending))
            })
            .await?;
        }

        return Ok(Outcome::Waiting);
    }

    install::replace(app, unpacked, &ext.id, &next.name, &next.version).await?;
    Ok(Outcome::Updated)
}

/// Records `version` as installed and drops any update that waited.
#[cfg(target_os = "macos")]
async fn settle(app: &AppHandle, ext: &InstalledExtension, version: &str) -> WisResult<()> {
    let (id, name, version) = (ext.id.clone(), ext.name.clone(), version.to_string());
    install::change(app.clone(), app.state::<AppState>(), move |list| {
        wis_core::extensions::updates::with_update(list, &id, &name, &version)
    })
    .await
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    const ID: &str = "eimadpbcbfnmbkopoojfekhnkhdbieeh";

    const NEWER: &str = r#"<?xml version="1.0" encoding="UTF-8"?><gupdate xmlns="http://www.google.com/update2/response" protocol="2.0" server="prod"><daystart elapsed_days="7231" elapsed_seconds="76240"/><app appid="eimadpbcbfnmbkopoojfekhnkhdbieeh" cohort="1::" cohortname="" status="ok"><updatecheck _esbAllowlist="true" codebase="https://clients2.googleusercontent.com/crx/blobs/AZPV/EIMADPBCBFNMBKOPOOJFEKHNKHDBIEEH_4_9_133_0.crx?a=1&amp;b=2" fp="1.ee38" hash_sha256="ee38f20d1d50789f4b482c9a1a5dadab599b8682fd2a7dfb7de21fae8561b918" protected="0" size="851022" status="ok" version="4.9.133"/></app></gupdate>"#;

    const CURRENT: &str = r#"<?xml version="1.0" encoding="UTF-8"?><gupdate xmlns="http://www.google.com/update2/response" protocol="2.0" server="prod"><daystart elapsed_days="7231" elapsed_seconds="76240"/><app appid="eimadpbcbfnmbkopoojfekhnkhdbieeh" cohort="1::" cohortname="" status="ok"><updatecheck _esbAllowlist="true" status="noupdate"/></app></gupdate>"#;

    #[test]
    fn an_offered_version_is_read_with_its_package() {
        assert_eq!(
            offered(NEWER, ID),
            Some(Offered {
                version: "4.9.133".into(),
                codebase: "https://clients2.googleusercontent.com/crx/blobs/AZPV/EIMADPBCBFNMBKOPOOJFEKHNKHDBIEEH_4_9_133_0.crx?a=1&b=2".into(),
                sha256: Some("ee38f20d1d50789f4b482c9a1a5dadab599b8682fd2a7dfb7de21fae8561b918".into()),
            })
        );
    }

    #[test]
    fn no_update_and_other_extensions_offer_nothing() {
        assert_eq!(offered(CURRENT, ID), None);
        assert_eq!(offered(NEWER, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"), None);
        assert_eq!(offered("not xml <", ID), None);
        assert_eq!(offered("", ID), None);
    }

    #[test]
    fn checks_fall_due_a_day_apart() {
        let day = CHECK_EVERY.as_secs();
        assert!(is_due(None, 1_000));
        assert!(!is_due(Some(1_000), 1_000 + day - 1));
        assert!(is_due(Some(1_000), 1_000 + day));
        assert!(is_due(Some(5_000), 1_000));
    }
}
