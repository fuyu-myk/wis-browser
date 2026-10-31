//! The address bar's offer to translate: once a page has loaded, or a
//! single-page app has moved to another address, the language it is in and
//! whether the reader reads it, decided once per language until the macOS
//! languages change.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tauri::{AppHandle, Manager};
use tokio::sync::OnceCell;
use wis_core::TabId;

use super::bridge::{self, Check, Status, TranslateError};
use super::language::{decide, page_language};
use super::page;
use crate::commands::{is_loading, mutate_unit};
use crate::platform::content_world;
use crate::state::AppState;

/// Languages remembered at most; past it the cache starts again.
const MAX_LANGUAGES: usize = 64;

static NEXT_PROBE: AtomicU64 = AtomicU64::new(1);
/// Set once the framework is missing (before macOS 26.4).
static UNAVAILABLE: AtomicBool = AtomicBool::new(false);
/// Each tab's latest probe, until it navigates.
static PROBES: Mutex<Option<HashMap<String, u64>>> = Mutex::new(None);
static OFFERS: Mutex<Option<Offers>> = Mutex::new(None);

type Decided = Arc<OnceCell<Option<String>>>;

/// The offer for each language, decided once however many tabs ask at once.
#[derive(Default)]
struct Offers {
    /// The macOS languages they were decided for.
    languages: Vec<String>,
    decided: HashMap<String, Decided>,
}

impl Offers {
    /// Where `source`'s offer for a reader of `languages` is decided.
    fn cell(&mut self, languages: &[String], source: &str) -> Decided {
        let key = key(source);
        if self.languages != languages {
            self.languages = languages.to_vec();
            self.decided.clear();
        } else if self.decided.len() >= MAX_LANGUAGES && !self.decided.contains_key(&key) {
            self.decided.clear();
        }
        self.decided.entry(key).or_default().clone()
    }
}

fn probes() -> MutexGuard<'static, Option<HashMap<String, u64>>> {
    PROBES.lock().unwrap_or_else(PoisonError::into_inner)
}

fn offers() -> MutexGuard<'static, Option<Offers>> {
    OFFERS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The page's language, from what it declares and what its text is in.
pub async fn language_of(raw: &str, sample: &str) -> Option<String> {
    let detected = if sample.trim().is_empty() {
        None
    } else {
        bridge::detect(sample).await.ok()
    };
    decide(page_language(raw), sample, detected.as_ref())
}

/// The source language's name when the page is in a language the reader
/// doesn't read and Wis can translate it.
pub fn offer(check: &Check) -> Option<String> {
    (!check.reads_source && check.high_fidelity != Status::Unsupported)
        .then(|| check.source_name.clone())
}

pub fn available() -> bool {
    !UNAVAILABLE.load(Ordering::Relaxed)
}

/// Asks the page in `webview` what language it is in.
pub fn probe(webview: &tauri::Webview) {
    if !available() {
        return;
    }
    let probe = register(webview.label());
    if let Err(err) = content_world::eval(webview, page::probe_script(probe)) {
        log::debug!("the translation probe failed to run: {err}");
    }
}

/// Probes tab `label`'s page again, unless a load has started, which probes
/// once it ends. On the main thread, where a load's start forgets the probe.
pub fn reprobe(app: &AppHandle, label: &str) {
    if let Some(webview) = app
        .get_webview(label)
        .filter(|webview| !is_loading(webview))
    {
        probe(&webview);
    }
}

/// A new probe of tab `label`'s page.
fn register(label: &str) -> u64 {
    let probe = NEXT_PROBE.fetch_add(1, Ordering::Relaxed);
    probes()
        .get_or_insert_with(HashMap::new)
        .insert(label.to_string(), probe);
    probe
}

/// The page's answer to `probe`: decides the offer, unless the tab navigated
/// or was probed again since.
pub fn probed(app: &AppHandle, label: &str, probe: u64, lang: String, sample: String) {
    if current(label, probe) {
        let task = show(app.clone(), label.to_string(), probe, lang, sample);
        tauri::async_runtime::spawn(task);
    }
}

/// Drops the tab's probe when it navigates or closes.
pub fn forget(label: &str) {
    if let Some(map) = probes().as_mut() {
        map.remove(label);
    }
}

fn current(label: &str, probe: u64) -> bool {
    probes().as_ref().and_then(|map| map.get(label)) == Some(&probe)
}

fn tab_of(label: &str) -> Option<TabId> {
    label
        .strip_prefix("tab-")
        .and_then(|id| uuid::Uuid::parse_str(id).ok())
        .map(TabId::from_uuid)
}

/// The tag a cached offer stands for, whole. (zh-TW is Traditional Chinese,
/// zh-CN Simplified)
fn key(source: &str) -> String {
    source.replace('_', "-").to_ascii_lowercase()
}

/// The offer for `source` (`Some(None)` when there is none), or `None` when
/// the macOS languages could not be read or the check timed out.
async fn offer_for(source: &str) -> Option<Option<String>> {
    let languages = match bridge::languages().await {
        Ok(languages) => languages,
        Err(err) => {
            log::debug!("the macOS languages could not be read: {err}");
            return None;
        }
    };
    let cell = offers()
        .get_or_insert_with(Offers::default)
        .cell(&languages, source);
    let decided = cell
        .get_or_try_init(|| async {
            match bridge::check(source).await {
                Ok(check) => Ok(offer(&check)),
                Err(TranslateError::TimedOut) => Err(()),
                Err(err) => {
                    if err == TranslateError::Unavailable {
                        UNAVAILABLE.store(true, Ordering::Relaxed);
                    }
                    log::debug!("no translation offer for {source}: {err}");
                    Ok(None)
                }
            }
        })
        .await;
    decided.ok().cloned()
}

/// The offer an answer leaves on the button, or `None` to leave the button
/// as it is: when nothing changed, or the page's language or the check could
/// not be told.
fn change(shown: Option<&str>, decided: Option<Option<String>>) -> Option<Option<String>> {
    decided.filter(|offer| offer.as_deref() != shown)
}

async fn show(app: AppHandle, label: String, probe: u64, lang: String, sample: String) {
    let decided = match language_of(&lang, &sample).await {
        Some(source) => offer_for(&source).await,
        None => None,
    };
    let Some(tab_id) = tab_of(&label) else {
        return;
    };
    if !current(&label, probe) {
        return;
    }
    let state = app.state::<AppState>();
    let offer = {
        let browser = state.browser.lock().await;
        let Ok(tab) = browser.tab(tab_id) else {
            return;
        };
        change(tab.translatable_from.as_deref(), decided)
    };
    let Some(offer) = offer else {
        return;
    };
    let result = mutate_unit(&app, &state, |s| {
        if current(&label, probe) {
            s.with_tab_translatable_from(tab_id, offer)
        } else {
            Ok(s.clone())
        }
    })
    .await;
    if let Err(err) = result {
        log::debug!("the translation offer for {label} was not shown: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(reads_source: bool, high_fidelity: Status) -> Check {
        Check {
            target: "en".into(),
            source_name: "Japanese".into(),
            target_name: "English".into(),
            same_language: false,
            reads_source,
            high_fidelity,
            low_latency: Status::Unsupported,
        }
    }

    #[test]
    fn only_a_language_the_reader_does_not_read_and_wis_can_translate_is_offered() {
        assert_eq!(
            offer(&check(false, Status::Installed)).as_deref(),
            Some("Japanese")
        );
        assert_eq!(
            offer(&check(false, Status::Supported)).as_deref(),
            Some("Japanese")
        );
        assert_eq!(offer(&check(false, Status::Unsupported)), None);
        assert_eq!(offer(&check(true, Status::Installed)), None);
    }

    #[test]
    fn an_answer_changes_the_offer_only_when_it_could_tell_and_it_differs() {
        let japanese = Some("Japanese".to_string());
        assert_eq!(change(None, Some(japanese.clone())), Some(japanese.clone()));
        assert_eq!(
            change(Some("Japanese"), Some(None)),
            Some(None),
            "taken back"
        );
        assert_eq!(change(Some("Japanese"), None), None, "could not tell");
        assert_eq!(change(Some("Japanese"), Some(japanese)), None, "unchanged");
        assert_eq!(change(None, Some(None)), None, "unchanged");
    }

    #[test]
    fn a_navigation_or_a_newer_probe_makes_an_answer_late() {
        let first = register("tab-test");
        let second = register("tab-test");
        assert!(current("tab-test", second));
        assert!(!current("tab-test", first));
        forget("tab-test");
        assert!(!current("tab-test", second));
    }

    #[test]
    fn other_macos_languages_decide_every_offer_again() {
        let mut offers = Offers::default();
        let english = ["en-US".to_string()];
        let first = offers.cell(&english, "ja");
        assert!(Arc::ptr_eq(&first, &offers.cell(&english, "JA")));
        let french = ["fr-FR".to_string(), "en-US".to_string()];
        assert!(!Arc::ptr_eq(&first, &offers.cell(&french, "ja")));
        assert!(!Arc::ptr_eq(&first, &offers.cell(&english, "ja")));
    }

    #[test]
    fn past_the_cap_the_offers_start_again() {
        let mut offers = Offers::default();
        let english = ["en-US".to_string()];
        let first = offers.cell(&english, "lang-0");
        for n in 1..MAX_LANGUAGES {
            offers.cell(&english, &format!("lang-{n}"));
        }
        assert!(Arc::ptr_eq(&first, &offers.cell(&english, "lang-0")));
        offers.cell(&english, "one-more");
        assert!(!Arc::ptr_eq(&first, &offers.cell(&english, "lang-0")));
    }

    #[test]
    fn a_tag_keeps_its_own_offer_as_a_region_can_decide_the_script() {
        assert_eq!(key("JA_jp"), "ja-jp");
        assert_eq!(key("zh-Hant-TW"), "zh-hant-tw");
        assert_ne!(key("zh-TW"), key("zh-CN"));
        assert_ne!(key("zh-Hans"), key("zh-Hant"));
    }
}
