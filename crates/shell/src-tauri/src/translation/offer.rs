//! The address bar's offer to translate: once a page has loaded, the
//! language it is in and whether the reader reads it, decided once per
//! language for the session.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tauri::{AppHandle, Manager};
use tokio::sync::OnceCell;
use wis_core::TabId;

use super::bridge::{self, Check, Status, TranslateError};
use super::language::{decide, page_language};
use crate::commands::mutate_unit;
use crate::state::AppState;

/// Languages remembered at most; past it the cache starts again.
const MAX_LANGUAGES: usize = 64;

static NEXT_PROBE: AtomicU64 = AtomicU64::new(1);
/// Set once the framework is missing (before macOS 26.4).
static UNAVAILABLE: AtomicBool = AtomicBool::new(false);
/// Each tab's latest probe, until it navigates.
static PROBES: Mutex<Option<HashMap<String, u64>>> = Mutex::new(None);
/// The offer for each language, decided once however many tabs ask at once.
static OFFERS: Mutex<Option<HashMap<String, Decided>>> = Mutex::new(None);

type Decided = Arc<OnceCell<Option<String>>>;
type Map<T> = MutexGuard<'static, Option<HashMap<String, T>>>;

fn probes() -> Map<u64> {
    PROBES.lock().unwrap_or_else(PoisonError::into_inner)
}

fn offers() -> Map<Decided> {
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

/// A new probe of tab `label`'s page.
pub fn register(label: &str) -> u64 {
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

/// The tag a cached offer stands for, whole. (zh-TW is Traditional Chinese,
/// zh-CN Simplified)
fn key(source: &str) -> String {
    source.replace('_', "-").to_ascii_lowercase()
}

async fn offer_for(source: &str) -> Option<String> {
    let cell = {
        let mut guard = offers();
        let map = guard.get_or_insert_with(HashMap::new);
        if map.len() >= MAX_LANGUAGES && !map.contains_key(&key(source)) {
            map.clear();
        }
        map.entry(key(source)).or_default().clone()
    };
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
    decided.ok().cloned().flatten()
}

async fn show(app: AppHandle, label: String, probe: u64, lang: String, sample: String) {
    let Some(source) = language_of(&lang, &sample).await else {
        return;
    };
    let Some(offer) = offer_for(&source).await else {
        return;
    };
    let Some(tab_id) = label
        .strip_prefix("tab-")
        .and_then(|id| uuid::Uuid::parse_str(id).ok())
        .map(TabId::from_uuid)
    else {
        return;
    };
    if !current(&label, probe) {
        return;
    }
    let state = app.state::<AppState>();
    let result = mutate_unit(&app, &state, |s| {
        if current(&label, probe) {
            s.with_tab_translatable_from(tab_id, Some(offer))
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
    fn a_navigation_or_a_newer_probe_makes_an_answer_late() {
        let first = register("tab-test");
        let second = register("tab-test");
        assert!(current("tab-test", second));
        assert!(!current("tab-test", first));
        forget("tab-test");
        assert!(!current("tab-test", second));
    }

    #[test]
    fn a_tag_keeps_its_own_offer_as_a_region_can_decide_the_script() {
        assert_eq!(key("JA_jp"), "ja-jp");
        assert_eq!(key("zh-Hant-TW"), "zh-hant-tw");
        assert_ne!(key("zh-TW"), key("zh-CN"));
        assert_ne!(key("zh-Hans"), key("zh-Hant"));
    }
}
