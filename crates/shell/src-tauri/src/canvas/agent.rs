//! A canvas page's AI requests over `wisCanvas`. Settings, the key and the
//! workspace stay on this side; the page sends only the conversation, and
//! its agent may offer only the canvas tools.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, OnceLock};

use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use wis_core::{StreamEvent, WisError, WisResult};

use crate::ai::chat::{self, AiMessage, ChatJob, ChatTools};
use crate::canvas::store::CanvasTab;
use crate::state::AppState;

static APP: OnceLock<AppHandle> = OnceLock::new();
static STREAMS: LazyLock<Mutex<HashMap<String, usize>>> = LazyLock::new(Default::default);
/// Cancels that arrived before their stream started.
static EARLY_CANCELS: LazyLock<Mutex<Vec<String>>> = LazyLock::new(Default::default);
const MAX_EARLY_CANCELS: usize = 16;
const MAX_REQUEST_ID: usize = 64;
const MAX_TOKENS: u32 = 8192;
const MAX_MESSAGES: usize = 200;
const MAX_STREAMS_PER_TAB: usize = 2;

static LISTINGS: LazyLock<Mutex<HashMap<String, (bool, std::time::Instant)>>> =
    LazyLock::new(Default::default);
const LIST_GAP: std::time::Duration = std::time::Duration::from_secs(2);

/// One model list at a time per tab, and not more often than [`LIST_GAP`].
struct Listing(String);

impl Listing {
    fn start(label: &str) -> Option<Self> {
        let mut listings = LISTINGS.lock().ok()?;
        let now = std::time::Instant::now();
        if let Some((running, at)) = listings.get(label) {
            if *running || now.duration_since(*at) < LIST_GAP {
                return None;
            }
        }

        listings.insert(label.to_string(), (true, now));
        Some(Self(label.to_string()))
    }
}

impl Drop for Listing {
    fn drop(&mut self) {
        if let Ok(mut listings) = LISTINGS.lock() {
            if let Some(entry) = listings.get_mut(&self.0) {
                entry.0 = false;
            }
            if listings.len() > 256 {
                listings.retain(|_, (running, _)| *running);
            }
        }
    }
}

type Counts = LazyLock<Mutex<HashMap<String, usize>>>;

static WEB_READS: Counts = LazyLock::new(Default::default);
static DOCUMENT_READS: Counts = LazyLock::new(Default::default);

/// Counts a tab's open streams or web reads; dropping it closes one.
struct Stream(&'static Counts, String);

impl Stream {
    fn open(label: &str) -> Option<Self> {
        Self::open_in(&STREAMS, label)
    }

    fn open_in(counts: &'static Counts, label: &str) -> Option<Self> {
        let mut open_now = counts.lock().ok()?;
        let open = open_now.entry(label.to_string()).or_default();
        if *open >= MAX_STREAMS_PER_TAB {
            return None;
        }

        *open += 1;
        Some(Self(counts, label.to_string()))
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        if let Ok(mut open_now) = self.0.lock() {
            if let Some(open) = open_now.get_mut(&self.1) {
                *open = open.saturating_sub(1);
                if *open == 0 {
                    open_now.remove(&self.1);
                }
            }
        }
    }
}

pub fn start(app: AppHandle) {
    let _ = APP.set(app);
}

pub(crate) fn app() -> Option<&'static AppHandle> {
    APP.get()
}

fn cancel_early(request_id: String) {
    if let Ok(mut early) = EARLY_CANCELS.lock() {
        if early.len() >= MAX_EARLY_CANCELS {
            early.remove(0);
        }
        early.push(request_id);
    }
}

fn cancelled_early(request_id: &str) -> bool {
    EARLY_CANCELS.lock().is_ok_and(|mut early| {
        let found = early.iter().position(|id| id == request_id);
        found.map(|at| early.remove(at)).is_some()
    })
}

/// The page writes `{"id":...,"op":...}` first, so an AI request is told apart
/// without parsing what may be a multi-megabyte save.
pub fn is_ai_request(text: &str) -> bool {
    op_of(text).is_some_and(|op| op.starts_with("ai"))
}

fn op_of(text: &str) -> Option<&str> {
    let head = text.get(..text.len().min(48)).unwrap_or("");
    head.split_once(r#","op":""#).map(|(_, rest)| rest)
}

pub const MAX_DOCUMENT_MESSAGE: usize =
    crate::canvas::document_text::MAX_BYTES.div_ceil(3) * 4 + 4096;

pub fn is_document_request(text: &str) -> bool {
    op_of(text).is_some_and(|op| op.starts_with(r#"aiReadDocument""#))
}

#[derive(Deserialize)]
struct Envelope {
    id: u64,
    #[serde(flatten)]
    request: Request,
}

#[derive(Deserialize)]
#[serde(tag = "op")]
enum Request {
    #[serde(rename = "aiStatus")]
    Status,
    #[serde(rename = "aiChat")]
    Chat {
        request_id: String,
        messages: Vec<AiMessage>,
        #[serde(default)]
        skip_reasoning: bool,
        #[serde(default)]
        no_tools: bool,
    },
    #[serde(rename = "aiCancel")]
    Cancel { request_id: String },
    #[serde(rename = "aiModels")]
    Models,
    #[serde(rename = "aiSetModel")]
    SetModel { model: Option<String> },
    #[serde(rename = "aiWebSearch")]
    WebSearch { query: String },
    #[serde(rename = "aiWebFetch")]
    WebFetch { url: String },
    #[serde(rename = "aiMicStart")]
    MicStart,
    #[serde(rename = "aiMicStop")]
    MicStop,
    #[serde(rename = "aiReadDocument")]
    ReadDocument {
        kind: crate::canvas::document_text::DocKind,
        max_chars: usize,
        data: String,
    },
}

const MAX_MODEL_ID: usize = 200;

/// A model id fit to store: trimmed, bounded, printable; blank clears it.
fn tidy_model(model: Option<String>) -> WisResult<Option<String>> {
    let Some(model) = model
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
    else {
        return Ok(None);
    };
    if model.len() > MAX_MODEL_ID || model.chars().any(char::is_control) {
        return Err(bad_request());
    }
    Ok(Some(model))
}

/// The model a canvas agent uses: its own choice, else the assistant's.
fn canvas_model(ai: &wis_core::AiSettings) -> String {
    ai.canvas_model.clone().unwrap_or_else(|| ai.model.clone())
}

pub fn receive(label: String, tab: CanvasTab, text: String) {
    let Some(app) = APP.get().cloned() else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        let envelope = match serde_json::from_str::<Envelope>(&text) {
            Ok(envelope) => envelope,
            Err(_) => {
                let id = crate::canvas::page::head_id(&text).map_or(Value::Null, Value::from);
                reply(&app, &label, id, Err(bad_request()));
                return;
            }
        };
        drop(text);
        let id = Value::from(envelope.id);
        let result = run(&app, &label, tab, envelope.request).await;
        reply(&app, &label, id, result);
    });
}

async fn run(app: &AppHandle, label: &str, tab: CanvasTab, request: Request) -> WisResult<Value> {
    let state = app.state::<AppState>();
    match request {
        Request::Status => {
            let browser = state.browser.lock().await;
            let settings = &browser.settings.ai;
            Ok(json!({
                "available": chat::ready(settings),
                "model": canvas_model(settings),
                "supported": chat::ai_supported(),
                "dictation": crate::canvas::dictation::SUPPORTED,
                "documents": crate::canvas::document_text::SUPPORTED,
            }))
        }
        Request::Models => {
            let _listing = Listing::start(label).ok_or_else(|| WisError::InvalidInput {
                message: "the model list was just asked for; try again in a moment".into(),
            })?;
            let models = crate::ai::models::list_models(&state, tab.workspace).await?;
            let browser = state.browser.lock().await;
            let settings = &browser.settings.ai;
            Ok(json!({
                "models": models,
                "selected": settings.canvas_model,
                "assistant": settings.model,
            }))
        }
        Request::WebSearch { query } => {
            let _read = web_read(label)?;
            let window = window_of(app, label)?;
            let results =
                crate::ai::fetch::search_in(window, state, query, Some(tab.workspace)).await?;
            Ok(json!(results))
        }
        Request::WebFetch { url } => {
            let _read = web_read(label)?;
            let window = window_of(app, label)?;
            let page =
                crate::ai::fetch::fetch_in(window, state, url, None, Some(tab.workspace)).await?;
            Ok(json!(page))
        }
        Request::MicStart => {
            crate::canvas::dictation::start(app, label).await?;
            Ok(Value::Null)
        }
        Request::MicStop => {
            crate::canvas::dictation::stop(app, label);
            Ok(Value::Null)
        }
        Request::ReadDocument {
            kind,
            max_chars,
            data,
        } => {
            let _read =
                Stream::open_in(&DOCUMENT_READS, label).ok_or_else(|| WisError::InvalidInput {
                    message: "the canvas is already reading files".into(),
                })?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data)
                .map_err(|_| bad_request())?;
            let outcome = crate::canvas::document_text::read(kind, bytes, max_chars).await?;
            Ok(json!(outcome))
        }
        Request::SetModel { model } => {
            let model = tidy_model(model)?;
            crate::commands::mutate_unit(app, &state, |s| {
                let mut next = s.clone();
                next.settings.ai.canvas_model = model;
                Ok(next)
            })
            .await?;
            Ok(Value::Null)
        }
        Request::Cancel { request_id } => {
            if !chat::cancel(&state, &request_id) {
                cancel_early(request_id);
            }
            Ok(Value::Null)
        }
        Request::Chat {
            request_id,
            messages,
            skip_reasoning,
            no_tools,
        } => {
            if request_id.is_empty()
                || request_id.len() > MAX_REQUEST_ID
                || messages.len() > MAX_MESSAGES
            {
                return Err(bad_request());
            }

            let _stream = Stream::open(label).ok_or_else(|| WisError::InvalidInput {
                message: "the canvas agent is already busy".into(),
            })?;
            if cancelled_early(&request_id) {
                return Ok(Value::Null);
            }

            let model = canvas_model(&state.browser.lock().await.settings.ai);
            let job = ChatJob {
                workspace: tab.workspace,
                request_id: request_id.clone(),
                messages,
                max_tokens: MAX_TOKENS,
                skip_reasoning,
                no_tools,
                model: Some(model),
            };
            let tools = |ai: &wis_core::AiSettings| ChatTools {
                web: ai.web_access,
                local: false,
                action: false,
                canvas: true,
            };
            let mut emit = |event: StreamEvent| push(app, label, &request_id, &event);
            chat::stream_chat(&state, job, tools, &mut emit).await?;

            Ok(Value::Null)
        }
    }
}

/// Hands one stream event to the page; false once the tab is gone or has left
/// the canvas.
fn push(app: &AppHandle, label: &str, request_id: &str, event: &StreamEvent) -> bool {
    let Some(webview) = app.get_webview(label) else {
        return false;
    };
    if !webview
        .url()
        .is_ok_and(|url| crate::canvas::page::is_canvas_url(&url))
    {
        return false;
    }
    let (Ok(id), Ok(event)) = (
        serde_json::to_string(request_id),
        serde_json::to_string(event),
    ) else {
        return false;
    };
    webview
        .eval(format!(
            "window.__wisCanvas&&window.__wisCanvas.ai({id},{event})"
        ))
        .is_ok()
}

fn reply(app: &AppHandle, label: &str, id: Value, result: WisResult<Value>) {
    let reply = match result {
        Ok(value) => json!({ "id": id, "ok": true, "value": value }),
        Err(WisError::InvalidInput { message }) => {
            json!({ "id": id, "ok": false, "error": message })
        }
        Err(err) => {
            log::warn!("canvas AI request: {err}");
            json!({ "id": id, "ok": false, "error": "The AI request failed." })
        }
    };
    crate::canvas::page::deliver(app, label, &reply.to_string());
}

fn web_read(label: &str) -> WisResult<Stream> {
    Stream::open_in(&WEB_READS, label).ok_or_else(|| WisError::InvalidInput {
        message: "the canvas agent is already reading the web".into(),
    })
}

/// The window a canvas tab sits in, which hosts the hidden page a web read uses.
fn window_of(app: &AppHandle, label: &str) -> WisResult<tauri::Window> {
    app.get_webview(label)
        .map(|webview| webview.window())
        .ok_or_else(|| WisError::InvalidInput {
            message: "this canvas tab is gone".into(),
        })
}

fn bad_request() -> WisError {
    WisError::InvalidInput {
        message: "the canvas page sent an AI request Wis could not read".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_requests_are_told_apart_by_their_opening() {
        assert!(is_ai_request(r#"{"id":12,"op":"aiChat","request_id":"r"}"#));
        assert!(is_ai_request(r#"{"id":9007199254740991,"op":"aiStatus"}"#));
        assert!(!is_ai_request(
            r#"{"id":3,"op":"save","canvas":"x","doc":"{\"op\":\"ai\"}"}"#
        ));
        assert!(!is_ai_request(r#"{"id":3,"op":"list"}"#));
        assert!(!is_ai_request("é"));
    }

    #[test]
    fn document_reads_are_told_apart_and_parse() {
        let text =
            r#"{"id":7,"op":"aiReadDocument","kind":"pdf","max_chars":1200,"data":"JVBERi0="}"#;
        assert!(is_ai_request(text) && is_document_request(text));
        assert!(!is_document_request(r#"{"id":7,"op":"aiReadDocumentX"}"#));
        assert!(!is_document_request(
            r#"{"id":7,"op":"save","doc":"aiReadDocument"}"#
        ));
        let envelope: Envelope = serde_json::from_str(text).unwrap();
        assert!(matches!(
            envelope.request,
            Request::ReadDocument { kind: crate::canvas::document_text::DocKind::Pdf, max_chars: 1200, ref data } if data == "JVBERi0="
        ));
        assert!(serde_json::from_str::<Envelope>(
            r#"{"id":7,"op":"aiReadDocument","kind":"exe","max_chars":1,"data":""}"#
        )
        .is_err());
    }

    #[test]
    fn a_cancel_before_its_stream_is_kept_for_it() {
        cancel_early("r-early".into());
        assert!(cancelled_early("r-early"));
        assert!(!cancelled_early("r-early"), "used once");
        for i in 0..40 {
            cancel_early(format!("r{i}"));
        }
        assert!(EARLY_CANCELS.lock().unwrap().len() <= MAX_EARLY_CANCELS);
    }

    #[test]
    fn model_lists_are_one_at_a_time_and_spaced() {
        let label = "tab-listing-test";
        let first = Listing::start(label).expect("first");
        assert!(Listing::start(label).is_none(), "one at a time");
        drop(first);
        assert!(Listing::start(label).is_none(), "spaced out");
        assert!(Listing::start("tab-other").is_some());
    }

    #[test]
    fn a_chosen_model_is_tidied_or_refused() {
        assert_eq!(
            tidy_model(Some("  qwen3:8b ".into())).unwrap().as_deref(),
            Some("qwen3:8b")
        );
        assert_eq!(tidy_model(Some("   ".into())).unwrap(), None);
        assert_eq!(tidy_model(None).unwrap(), None);
        assert!(tidy_model(Some("bad\nid".into())).is_err());
        assert!(tidy_model(Some("x".repeat(MAX_MODEL_ID + 1))).is_err());
    }

    #[test]
    fn a_canvas_follows_the_assistant_until_it_picks() {
        let mut ai = wis_core::AiSettings {
            model: "claude-sonnet-5-5".into(),
            ..Default::default()
        };
        assert_eq!(canvas_model(&ai), "claude-sonnet-5-5");
        ai.canvas_model = Some("claude-haiku-5-5".into());
        assert_eq!(canvas_model(&ai), "claude-haiku-5-5");
    }

    #[test]
    fn requests_parse_with_their_fields() {
        let chat: Envelope = serde_json::from_str(
            r#"{"id":4,"op":"aiChat","request_id":"r1","messages":[{"role":"user","content":"hi"}]}"#,
        )
        .unwrap();
        assert!(
            matches!(chat.request, Request::Chat { ref request_id, ref messages, .. } if request_id == "r1" && messages.len() == 1)
        );
        let cancel: Envelope =
            serde_json::from_str(r#"{"id":5,"op":"aiCancel","request_id":"r1"}"#).unwrap();
        assert!(matches!(cancel.request, Request::Cancel { .. }));
    }
}
