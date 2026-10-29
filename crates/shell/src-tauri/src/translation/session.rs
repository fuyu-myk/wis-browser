//! One translation per tab: the start handshake with `page.js`, the check of
//! the page's language, and a worker that translates queued segments one at a
//! time. The framework serves one request at a time anyway, and the two
//! models slow each other down when asked at once.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;

use tauri::{AppHandle, Manager};
use tokio::sync::oneshot;

use super::bridge::{self, Check, Status, Strategy, TranslateError, TranslateRequest};
use super::language::page_language;
use super::message::{Block, PageMessage, Queue, Run};
use super::objects::{held, place, HELD, OBJECT};
use super::page;
use super::pieces::{anchor, lost_tags, pieces, piecewise, relabel, spaced, tidy, MAX_PIECES};
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
        /// Whether the low-latency model is installed for the pair and still
        /// answering.
        low_latency: bool,
        queue: Queue,
        working: bool,
    },
}

struct Task {
    block: Block,
    source: String,
    target: String,
    strategy: Strategy,
}

/// Failures that will not go away for this page, so the rest of it stays on
/// high fidelity.
fn lasting(err: &TranslateError) -> bool {
    matches!(
        err,
        TranslateError::NotInstalled | TranslateError::Unsupported | TranslateError::TimedOut
    )
}

/// The low-latency model drops inline elements, so it only takes plain text.
fn strategy_for(block: &Block, low_latency: bool) -> Strategy {
    if low_latency && block.plain() {
        Strategy::LowLatency
    } else {
        Strategy::HighFidelity
    }
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

    let go = page::go_script(gen, &source, &check.target);
    let running = with_session(label, gen, |phase| {
        *phase = Phase::Running {
            source,
            target: check.target,
            low_latency: check.low_latency == Status::Installed,
            queue: Queue::default(),
            working: false,
        };
    });
    if running.is_none() {
        return Ok(None);
    }

    eval(app, label, go);
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
        PageMessage::Blocks { gen, wave, blocks } => {
            let start_worker = with_session(label, gen, |phase| match phase {
                Phase::Running { queue, working, .. } => {
                    queue.push_batch(wave, blocks);
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

/// The worker's next segment, or `None` when the queue is empty (the worker
/// then stops) or the translation has ended.
fn next_task(label: &str, gen: u64) -> Option<Task> {
    with_session(label, gen, |phase| match phase {
        Phase::Running {
            queue,
            working,
            source,
            target,
            low_latency,
        } => {
            let low_latency = *low_latency;
            let Some(block) = queue.next(|block| low_latency && block.plain()) else {
                *working = false;
                return None;
            };
            Some(Task {
                strategy: strategy_for(&block, low_latency),
                block,
                source: source.clone(),
                target: target.clone(),
            })
        }
        Phase::Starting(_) => None,
    })
    .flatten()
}

fn stop_low_latency(label: &str, gen: u64) {
    with_session(label, gen, |phase| {
        if let Phase::Running { low_latency, .. } = phase {
            *low_latency = false;
        }
    });
}

fn apply(app: &AppHandle, label: &str, gen: u64, id: u32, runs: &[Run]) {
    if is_current(label, gen) {
        eval(app, label, page::apply_script(gen, id, runs));
    }
}

/// What stands for an object in a request with `strategy`.
fn marker(strategy: Strategy) -> &'static str {
    match strategy {
        Strategy::HighFidelity => HELD,
        Strategy::LowLatency => OBJECT,
    }
}

/// The segment translated with `strategy`; high fidelity gets its objects
/// as held bullets.
async fn ask(task: &Task, strategy: Strategy) -> Result<Vec<Run>, TranslateError> {
    let objects = &task.block.objects;
    let holding = (strategy == Strategy::HighFidelity && !objects.is_empty())
        .then(|| held(&task.block.runs, objects));
    let request = TranslateRequest {
        source: &task.source,
        target: &task.target,
        strategy,
        runs: holding.as_deref().unwrap_or(&task.block.runs),
        skip: if holding.is_some() { objects } else { &[] },
    };
    bridge::translate(&request).await
}

/// Translates the task's segment, with the strategy that answered. One the
/// low-latency model fails on is retried in high fidelity, and a lasting
/// failure keeps the page there.
async fn translate_whole(
    label: &str,
    gen: u64,
    task: &Task,
) -> Result<(Vec<Run>, Strategy), TranslateError> {
    match ask(task, task.strategy).await {
        Err(err)
            if task.strategy == Strategy::LowLatency
                && err != TranslateError::NothingToTranslate =>
        {
            if !is_current(label, gen) {
                return Err(err);
            }
            if lasting(&err) {
                log::debug!("low latency failed in {label}, using high fidelity from now: {err}");
                stop_low_latency(label, gen);
            }
            let runs = ask(task, Strategy::HighFidelity).await?;
            Ok((runs, Strategy::HighFidelity))
        }
        result => result.map(|runs| (runs, task.strategy)),
    }
}

async fn translate_piece(label: &str, gen: u64, task: &Task, text: String) -> Option<String> {
    if !is_current(label, gen) {
        return None;
    }
    let runs = [Run { text, tag: None }];
    let request = TranslateRequest {
        source: &task.source,
        target: &task.target,
        strategy: Strategy::HighFidelity,
        runs: &runs,
        skip: &[],
    };
    let pieces = bridge::translate(&request).await.ok()?;
    let text: String = pieces.into_iter().map(|piece| piece.text).collect();
    (!text.trim().is_empty()).then_some(text)
}

/// A lost inline element goes back on its own words in the translation,
/// and failing that, the segment is redone piece by piece; an object stays
/// as it was sent.
async fn translate(label: &str, gen: u64, task: &Task) -> Result<Vec<Run>, TranslateError> {
    let (whole, strategy) = translate_whole(label, gen, task).await?;
    let runs = place(
        whole,
        &task.block.runs,
        &task.block.objects,
        marker(strategy),
    );
    let sent = &task.block.runs;
    let lost = lost_tags(sent, &runs, &task.block.needed);
    if lost.is_empty() || pieces(sent) > MAX_PIECES || !is_current(label, gen) {
        return Ok(runs);
    }

    let unplaced = || TranslateError::Failed("an inline element could not be placed".into());
    let mut found = Vec::with_capacity(lost.len());
    for &(tag, text) in &lost {
        let piece = if task.block.objects.contains(&tag) {
            Some(text.to_string())
        } else {
            translate_piece(label, gen, task, text.to_string()).await
        };
        found.push((tag, piece.ok_or_else(unplaced)?));
    }

    let spaced = spaced(&task.target);
    if let Some(anchored) = anchor(runs, &found, spaced) {
        return Ok(anchored);
    }

    log::debug!("a segment in {label} lost an inline element, translating it piece by piece");
    let known = |text: &str| {
        lost.iter()
            .zip(&found)
            .find_map(|(&(_, sent), (_, piece))| (sent == text).then(|| piece.clone()))
    };
    let known = &known;
    piecewise(sent, spaced, |text| async move {
        match known(&text) {
            Some(piece) => Some(piece),
            None if text == OBJECT => Some(text),
            None => translate_piece(label, gen, task, text).await,
        }
    })
    .await
    .ok_or_else(unplaced)
}

async fn work(app: AppHandle, label: String, gen: u64) {
    while let Some(task) = next_task(&label, gen) {
        match translate(&label, gen, &task).await {
            Ok(runs) => {
                let runs = tidy(relabel(&task.block.runs, runs, &task.target));
                apply(&app, &label, gen, task.block.id, &runs);
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

    fn run(text: &str, tag: Option<u32>) -> Run {
        Run {
            text: text.into(),
            tag,
        }
    }

    #[test]
    fn plain_segments_take_low_latency_once_it_is_installed() {
        let plain = Block {
            id: 0,
            runs: vec![run("概要", None), run("の説明", None)],
            main: true,
            needed: vec![],
            objects: vec![],
        };
        let linked = Block {
            id: 1,
            runs: vec![run("詳しくは", None), run("こちら", Some(0))],
            main: true,
            needed: vec![0],
            objects: vec![],
        };
        let pictured = Block {
            id: 2,
            runs: vec![run("右上の", None), run("\u{FFFC}", Some(0))],
            main: true,
            needed: vec![0],
            objects: vec![0],
        };
        assert_eq!(strategy_for(&plain, true), Strategy::LowLatency);
        let two = Block {
            runs: vec![run("\u{FFFC}", Some(0)), run("\u{FFFC}", Some(1))],
            needed: vec![0, 1],
            objects: vec![0, 1],
            ..pictured.clone()
        };
        assert_eq!(strategy_for(&pictured, true), Strategy::LowLatency);
        assert_eq!(strategy_for(&two, true), Strategy::HighFidelity);
        assert_eq!(strategy_for(&linked, true), Strategy::HighFidelity);
        assert_eq!(strategy_for(&plain, false), Strategy::HighFidelity);
    }

    #[test]
    fn only_a_lasting_failure_turns_low_latency_off() {
        assert!(lasting(&TranslateError::NotInstalled));
        assert!(lasting(&TranslateError::Unsupported));
        assert!(lasting(&TranslateError::TimedOut));
        assert!(!lasting(&TranslateError::Failed("one segment".into())));
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
