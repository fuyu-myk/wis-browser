//! One translation per tab: the start handshake with `page.js`, the check of
//! the page's language, and a worker that translates queued segments one at a
//! time (the framework serves one request at a time anyway).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;

use tauri::{AppHandle, Manager};
use tokio::sync::oneshot;

use super::bridge::{self, Check, Status, Strategy, TranslateError, TranslateRequest};
use super::language::page_language;
use super::message::{PageMessage, Queue};
use super::page;
use crate::platform::content_world;

/// How long the page has to report its language after the script starts.
const START_TIMEOUT: Duration = Duration::from_secs(3);

static APP: OnceLock<AppHandle> = OnceLock::new();
static NEXT_GEN: AtomicU64 = AtomicU64::new(1);
static SESSIONS: Mutex<Option<HashMap<String, Session>>> = Mutex::new(None);

struct Session {
    gen: u64,
    phase: Phase,
}

enum Phase {
    /// Waiting for the page's `start` reply.
    Starting(Option<oneshot::Sender<String>>),
    Running {
        source: String,
        target: String,
        queue: Queue,
        working: bool,
    },
}

fn sessions() -> MutexGuard<'static, Option<HashMap<String, Session>>> {
    SESSIONS.lock().unwrap_or_else(PoisonError::into_inner)
}

fn with_session<T>(label: &str, gen: u64, f: impl FnOnce(&mut Phase) -> T) -> Option<T> {
    let mut guard = sessions();
    let session = guard.as_mut()?.get_mut(label)?;
    (session.gen == gen).then(|| f(&mut session.phase))
}

pub fn set_app_handle(app: &AppHandle) {
    APP.get_or_init(|| app.clone());
}

/// Reasons for refusing to translate a page.
fn refusal(check: &Check) -> Option<String> {
    if check.same_language {
        return Some(format!("This page is already in {}", check.target_name));
    }
    match check.high_fidelity {
        Status::Installed => None,
        Status::Supported => Some(format!(
            "{} isn't downloaded for translation yet",
            check.source_name
        )),
        Status::Unsupported => Some(format!(
            "Wis can't translate {} into {}",
            check.source_name, check.target_name
        )),
    }
}

fn failure_message(err: &TranslateError) -> String {
    match err {
        TranslateError::NoLanguage => "Wis can't tell what language this page is in".into(),
        TranslateError::Unavailable => "Translation needs macOS 26.4 or later".into(),
        TranslateError::TimedOut => "Translation isn't responding, try again in a moment".into(),
        other => {
            log::warn!("translation check failed: {other}");
            "This page can't be translated".into()
        }
    }
}

fn eval(app: &AppHandle, label: &str, js: String) {
    let Some(webview) = app.get_webview(label) else {
        return;
    };
    if let Err(err) = content_world::eval(&webview, js) {
        log::warn!("translation script failed to run in {label}: {err}");
    }
}

/// Ends the tab's translation without touching the page.
pub fn stop(label: &str) {
    if let Some(map) = sessions().as_mut() {
        map.remove(label);
    }
}

/// Ends the tab's translation and restores the original page.
pub fn disable(app: &AppHandle, label: &str) {
    stop(label);
    eval(app, label, page::REVERT_SCRIPT.to_string());
}

/// Whether translation `gen` is still the tab's, not replaced by a newer
/// press or ended by a navigation.
pub fn is_current(label: &str, gen: u64) -> bool {
    with_session(label, gen, |_| ()).is_some()
}

/// Removes translation `gen`, unless something newer already replaced it.
/// Returns whether it was current.
fn remove_if_current(label: &str, gen: u64) -> bool {
    let mut guard = sessions();
    let Some(map) = guard.as_mut() else {
        return false;
    };
    let current = map.get(label).is_some_and(|session| session.gen == gen);
    if current {
        map.remove(label);
    }
    current
}

/// Ends translation `gen` and restores the page, unless a newer translation
/// or a navigation has already replaced it. Returns whether it was current.
fn end(app: &AppHandle, label: &str, gen: u64) -> bool {
    let current = remove_if_current(label, gen);
    if current {
        eval(app, label, page::REVERT_SCRIPT.to_string());
    }
    current
}

/// Starts translating the page in tab `label`. `Ok(Some(gen))` once the
/// first segments are on their way (the rest follow as they scroll into
/// view), `Ok(None)` when a newer press or a navigation took over meanwhile,
/// and `Err` with the reason when Wis won't translate the page.
pub async fn enable(app: &AppHandle, label: &str) -> Result<Option<u64>, String> {
    let gen = NEXT_GEN.fetch_add(1, Ordering::Relaxed);
    let (sender, receiver) = oneshot::channel();
    sessions().get_or_insert_with(HashMap::new).insert(
        label.to_string(),
        Session {
            gen,
            phase: Phase::Starting(Some(sender)),
        },
    );
    eval(app, label, page::start_script(gen));

    let refuse = |message: String| {
        if end(app, label, gen) {
            Err(message)
        } else {
            Ok(None)
        }
    };
    let Ok(Ok(raw)) = tokio::time::timeout(START_TIMEOUT, receiver).await else {
        return refuse("This page can't be translated".into());
    };
    let Some(source) = page_language(&raw) else {
        return refuse(failure_message(&TranslateError::NoLanguage));
    };
    let check = match bridge::check(&source).await {
        Ok(check) => check,
        Err(err) => return refuse(failure_message(&err)),
    };
    if let Some(message) = refusal(&check) {
        return refuse(message);
    }

    let running = with_session(label, gen, |phase| {
        *phase = Phase::Running {
            source,
            target: check.target,
            queue: Queue::default(),
            working: false,
        };
    });
    if running.is_none() {
        return Ok(None);
    }

    eval(app, label, page::go_script(gen));
    Ok(Some(gen))
}

/// A message from the page's script, on the main thread.
pub fn on_message(label: &str, message: PageMessage) {
    match message {
        PageMessage::Start { gen, lang } => {
            let sender = with_session(label, gen, |phase| match phase {
                Phase::Starting(sender) => sender.take(),
                Phase::Running { .. } => None,
            });
            if let Some(sender) = sender.flatten() {
                if sender.send(lang).is_err() {
                    log::debug!("translation start reply arrived after the wait ended");
                }
            }
        }
        PageMessage::Blocks { gen, blocks } => {
            let start_worker = with_session(label, gen, |phase| match phase {
                Phase::Running { queue, working, .. } => {
                    queue.push_batch(blocks);
                    !std::mem::replace(working, true)
                }
                Phase::Starting(_) => false,
            });
            if start_worker == Some(true) {
                if let Some(app) = APP.get() {
                    tauri::async_runtime::spawn(work(app.clone(), label.to_string(), gen));
                }
            }
        }
    }
}

/// The next segment to translate with its language pair, or `None` when the
/// queue is empty (the worker then stops) or the translation has ended.
fn next_segment(label: &str, gen: u64) -> Option<(super::message::Block, String, String)> {
    with_session(label, gen, |phase| match phase {
        Phase::Running {
            queue,
            working,
            source,
            target,
        } => match queue.next() {
            Some(block) => Some((block, source.clone(), target.clone())),
            None => {
                *working = false;
                None
            }
        },
        Phase::Starting(_) => None,
    })
    .flatten()
}

async fn work(app: AppHandle, label: String, gen: u64) {
    while let Some((block, source, target)) = next_segment(&label, gen) {
        let request = TranslateRequest {
            source: &source,
            target: &target,
            strategy: Strategy::HighFidelity,
            runs: &block.runs,
        };
        match bridge::translate(&request).await {
            Ok(runs) => {
                if is_current(&label, gen) {
                    eval(&app, &label, page::apply_script(gen, block.id, &runs));
                }
            }
            Err(TranslateError::NothingToTranslate) => {}
            Err(err @ (TranslateError::NotInstalled | TranslateError::Unsupported)) => {
                log::warn!("translation stopped in {label}: {err}");
                remove_if_current(&label, gen);
                return;
            }
            Err(err) => log::debug!("a segment in {label} was not translated: {err}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translation::{Check, Status};

    fn check(same: bool, high_fidelity: Status) -> Check {
        Check {
            target: "en".into(),
            source_name: "Japanese".into(),
            target_name: "English".into(),
            same_language: same,
            high_fidelity,
            low_latency: Status::Unsupported,
        }
    }

    #[test]
    fn an_installed_foreign_language_is_translated() {
        assert_eq!(refusal(&check(false, Status::Installed)), None);
    }

    #[test]
    fn a_page_in_the_reader_s_language_or_without_models_is_refused_plainly() {
        assert_eq!(
            refusal(&check(true, Status::Installed)).as_deref(),
            Some("This page is already in English")
        );
        assert_eq!(
            refusal(&check(false, Status::Supported)).as_deref(),
            Some("Japanese isn't downloaded for translation yet")
        );
        assert_eq!(
            refusal(&check(false, Status::Unsupported)).as_deref(),
            Some("Wis can't translate Japanese into English")
        );
    }

    #[test]
    fn a_failed_check_says_what_went_wrong() {
        assert_eq!(
            failure_message(&TranslateError::NoLanguage),
            "Wis can't tell what language this page is in"
        );
        assert_eq!(
            failure_message(&TranslateError::Unavailable),
            "Translation needs macOS 26.4 or later"
        );
        assert_eq!(
            failure_message(&TranslateError::TimedOut),
            "Translation isn't responding, try again in a moment"
        );
    }
}
