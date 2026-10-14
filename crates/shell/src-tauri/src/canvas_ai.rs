//! A canvas page's AI requests over `wisCanvas`. Settings, the key and the
//! workspace stay on this side; the page sends only the conversation, and
//! its agent may offer only the canvas tools.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, OnceLock};

use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use wis_core::{StreamEvent, WisError, WisResult};

use crate::ai::{self, AiMessage, ChatJob, ChatTools};
use crate::canvas_store::CanvasTab;
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

/// Counts a tab's open streams; dropping it closes one.
struct Stream(String);

impl Stream {
    fn open(label: &str) -> Option<Self> {
        let mut streams = STREAMS.lock().ok()?;
        let open = streams.entry(label.to_string()).or_default();
        if *open >= MAX_STREAMS_PER_TAB {
            return None;
        }

        *open += 1;
        Some(Self(label.to_string()))
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        if let Ok(mut streams) = STREAMS.lock() {
            if let Some(open) = streams.get_mut(&self.0) {
                *open = open.saturating_sub(1);
                if *open == 0 {
                    streams.remove(&self.0);
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
    let head = text.get(..text.len().min(48)).unwrap_or("");
    head.split_once(r#","op":""#)
        .is_some_and(|(_, rest)| rest.starts_with("ai"))
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
}

pub fn receive(label: String, tab: CanvasTab, text: String) {
    let Some(app) = APP.get().cloned() else {
        return;
    };
    let envelope = match serde_json::from_str::<Envelope>(&text) {
        Ok(envelope) => envelope,
        Err(_) => {
            let id = crate::canvas::head_id(&text).map_or(Value::Null, Value::from);
            reply(&app, &label, id, Err(bad_request()));
            return;
        }
    };
    let id = Value::from(envelope.id);
    tauri::async_runtime::spawn(async move {
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
                "available": ai::ready(settings),
                "model": settings.model,
            }))
        }
        Request::Cancel { request_id } => {
            if !ai::cancel(&state, &request_id) {
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
            if request_id.is_empty() || request_id.len() > MAX_REQUEST_ID || messages.len() > MAX_MESSAGES {
                return Err(bad_request());
            }

            let _stream = Stream::open(label).ok_or_else(|| WisError::InvalidInput {
                message: "the canvas agent is already busy".into(),
            })?;
            if cancelled_early(&request_id) {
                return Ok(Value::Null);
            }

            let job = ChatJob {
                workspace: tab.workspace,
                request_id: request_id.clone(),
                messages,
                max_tokens: MAX_TOKENS,
                skip_reasoning,
                no_tools,
            };
            let tools = |_: &wis_core::AiSettings| ChatTools {
                web: false,
                local: false,
                action: false,
                canvas: true,
            };
            let mut emit = |event: StreamEvent| push(app, label, &request_id, &event);
            ai::stream_chat(&state, job, tools, &mut emit).await?;

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
    if !webview.url().is_ok_and(|url| crate::canvas::is_canvas_url(&url)) {
        return false;
    }
    let (Ok(id), Ok(event)) = (serde_json::to_string(request_id), serde_json::to_string(event)) else {
        return false;
    };
    webview
        .eval(format!("window.__wisCanvas&&window.__wisCanvas.ai({id},{event})"))
        .is_ok()
}

fn reply(app: &AppHandle, label: &str, id: Value, result: WisResult<Value>) {
    let reply = match result {
        Ok(value) => json!({ "id": id, "ok": true, "value": value }),
        Err(WisError::InvalidInput { message }) => json!({ "id": id, "ok": false, "error": message }),
        Err(err) => {
            log::warn!("canvas AI request: {err}");
            json!({ "id": id, "ok": false, "error": "The AI request failed." })
        }
    };
    crate::canvas::deliver(app, label, &reply.to_string());
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
        assert!(!is_ai_request(r#"{"id":3,"op":"save","canvas":"x","doc":"{\"op\":\"ai\"}"}"#));
        assert!(!is_ai_request(r#"{"id":3,"op":"list"}"#));
        assert!(!is_ai_request("é"));
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
    fn requests_parse_with_their_fields() {
        let chat: Envelope = serde_json::from_str(
            r#"{"id":4,"op":"aiChat","request_id":"r1","messages":[{"role":"user","content":"hi"}]}"#,
        )
        .unwrap();
        assert!(matches!(chat.request, Request::Chat { ref request_id, ref messages, .. } if request_id == "r1" && messages.len() == 1));
        let cancel: Envelope =
            serde_json::from_str(r#"{"id":5,"op":"aiCancel","request_id":"r1"}"#).unwrap();
        assert!(matches!(cancel.request, Request::Cancel { .. }));
    }
}
