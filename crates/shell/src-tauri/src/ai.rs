//! BYOK AI foundation, imperative shell: API-key storage commands and the
//! streaming chat command. Pure request-shaping and SSE parsing live in
//! [`wis_core::ai`].
//!
//! Key hygiene: the API key is loaded from the OS keystore per request,
//! kept in `Zeroizing` bytes, and used only as an auth header. It is never
//! stringified into logs, events, or error messages. Transport errors are
//! reduced to a coarse category so nothing incidental leaks. Requests route
//! through the workspace loopback proxy with upstream TLS verification ON.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use serde::Deserialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, State};
use wis_core::ai::chat_body;
use wis_core::{
    AiProvider, ChatMessage, ChatRequest, Role, SseParser, StreamEvent, WisError, WisResult,
    WorkspaceId,
};
use zeroize::Zeroizing;

use crate::state::AppState;

/// Bounds only the reach for the provider. Generation itself is deliberately
/// unbounded. Stopping is handled via `ai_cancel`.
const AI_CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// How often a waiting request notices that it was cancelled.
const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(250);
const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const OPENAI_DEFAULT_BASE: &str = "https://api.openai.com/v1";

/// Tauri event carrying the new key-present state after a set or clear.
const AI_KEY_EVENT: &str = "wis-ai-key-changed";

/// Platforms with an OS key store to seal the provider key in: macOS
/// (Keychain) and Windows (DPAPI). Elsewhere the assistant stays off rather
/// than holding the key somewhere weaker.
fn ai_supported() -> bool {
    cfg!(any(target_os = "macos", windows))
}

fn unsupported() -> WisError {
    WisError::InvalidInput {
        message: "the assistant is not available on this platform".into(),
    }
}

fn emit_key_present(app: &AppHandle, present: bool) {
    if let Err(err) = app.emit(AI_KEY_EVENT, present) {
        log::warn!("emit {AI_KEY_EVENT} failed: {err}");
    }
}

// ---------------------------------------------------------------------------
// Key storage commands
// ---------------------------------------------------------------------------

/// Store the provider API key in the OS keystore (non-biometric). Trims
/// surrounding whitespace and rejects an empty key. Never echoes it back.
#[tauri::command]
pub async fn ai_set_key(app: AppHandle, key: String) -> WisResult<()> {
    if !ai_supported() {
        return Err(unsupported());
    }

    let key = Zeroizing::new(key);
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return Err(WisError::InvalidInput {
            message: "API key is empty".into(),
        });
    }

    wis_secrets::ai_key_store(trimmed.as_bytes()).map_err(|e| WisError::InvalidInput {
        message: format!("could not store key: {e}"),
    })?;
    emit_key_present(&app, true);

    Ok(())
}

/// Whether a provider API key is currently stored. This is the assistant's
/// activation switch. A platform without a key store answers `false`.
#[tauri::command]
pub async fn ai_key_present() -> WisResult<bool> {
    if !ai_supported() {
        return Ok(false);
    }

    wis_secrets::ai_key_load()
        .map(|k| k.is_some())
        .map_err(|e| WisError::InvalidInput {
            message: format!("could not check key: {e}"),
        })
}

/// Remove the stored provider API key (idempotent).
#[tauri::command]
pub async fn ai_clear_key(app: AppHandle) -> WisResult<()> {
    if !ai_supported() {
        return Ok(());
    }

    wis_secrets::ai_key_delete().map_err(|e| WisError::InvalidInput {
        message: format!("could not clear key: {e}"),
    })?;
    emit_key_present(&app, false);

    Ok(())
}

// ---------------------------------------------------------------------------
// Streaming chat
// ---------------------------------------------------------------------------

/// One message from the caller. Mirrors [`wis_core::ChatMessage`] with a serde
/// derive for the IPC boundary.
#[derive(Deserialize)]
pub struct AiMessage {
    pub role: Role,
    pub content: String,
}

/// Arguments for [`ai_chat`]. The model and provider come from settings; the
/// key comes from the keystore.
#[derive(Deserialize)]
pub struct AiChatArgs {
    pub workspace_id: WorkspaceId,
    pub messages: Vec<AiMessage>,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    /// Caller-minted id [`ai_cancel`] uses to stop this completion.
    pub request_id: String,
}

fn default_max_tokens() -> u32 {
    4096
}

/// Registers a request's cancel flag on construction and removes it on drop.
struct CancelGuard<'a> {
    state: &'a AppState,
    request_id: String,
}

impl<'a> CancelGuard<'a> {
    fn register(state: &'a AppState, request_id: &str) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        if let Ok(mut map) = state.ai_cancels.lock() {
            map.insert(request_id.to_string(), Arc::clone(&flag));
        }

        flag
    }
}

impl Drop for CancelGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut map) = self.state.ai_cancels.lock() {
            map.remove(&self.request_id);
        }
    }
}

/// Stop an in-flight completion, closing the upstream connection rather than
/// only muting the reply. A no-op for an id that already finished.
#[tauri::command]
pub async fn ai_cancel(state: State<'_, AppState>, request_id: String) -> WisResult<()> {
    if let Ok(map) = state.ai_cancels.lock() {
        if let Some(flag) = map.get(&request_id) {
            flag.store(true, Ordering::Relaxed);
        }
    }

    Ok(())
}

/// Stream a chat completion to `on_event` as [`StreamEvent`]s (`delta` / `done`
/// / `error`). Fails fast if no key or model is configured.
///
/// A provider-side failure (non-2xx or an in-stream error) is delivered as a
/// terminal `error` event, handled uniformly by the frontend.
#[tauri::command]
pub async fn ai_chat(
    state: State<'_, AppState>,
    args: AiChatArgs,
    on_event: Channel<StreamEvent>,
) -> WisResult<()> {
    if !ai_supported() {
        return Err(unsupported());
    }

    let app_state: &AppState = &state;
    let cancelled = CancelGuard::register(app_state, &args.request_id);
    let _cancel_guard = CancelGuard {
        state: app_state,
        request_id: args.request_id.clone(),
    };

    let (provider, model, base_url) = {
        let browser = state.browser.lock().await;
        let ai = &browser.settings.ai;
        (ai.provider, ai.model.clone(), ai.base_url.clone())
    };
    if model.trim().is_empty() {
        return Err(WisError::InvalidInput {
            message: "no AI model configured".into(),
        });
    }

    let key = wis_secrets::ai_key_load().map_err(|e| WisError::InvalidInput {
        message: format!("could not read key: {e}"),
    })?;

    let req = ChatRequest {
        model,
        max_tokens: args.max_tokens,
        messages: args
            .messages
            .into_iter()
            .map(|m| ChatMessage {
                role: m.role,
                content: m.content,
            })
            .collect(),
    };
    let body = chat_body(provider, &req);

    let key = key.as_ref().map(|k| k.as_slice());
    let (url, headers) = match provider {
        AiProvider::Anthropic => {
            let key = key.ok_or_else(|| WisError::InvalidInput {
                message: "no AI key configured".into(),
            })?;
            anthropic_request(key)?
        }
        AiProvider::OpenAiCompatible => openai_request(base_url.as_deref(), key)?,
    };

    let local_endpoint = url::Url::parse(&url)
        .map(|parsed| wis_network::is_loopback(&parsed))
        .unwrap_or(false);

    let mut builder = reqwest::Client::builder().connect_timeout(AI_CONNECT_TIMEOUT);
    if !local_endpoint {
        let proxy_port = state
            .ensure_proxy_for(args.workspace_id)
            .await
            .map_err(|e| WisError::InvalidInput {
                message: format!("proxy unavailable: {e:?}"),
            })?;
        let proxy = reqwest::Proxy::all(format!("http://127.0.0.1:{proxy_port}")).map_err(|e| {
            WisError::InvalidInput {
                message: format!("proxy: {e}"),
            }
        })?;

        builder = builder.proxy(proxy);
    }

    let client = builder.build().map_err(|e| WisError::InvalidInput {
        message: format!("http client: {e}"),
    })?;

    let request = client
        .post(url)
        .headers(headers)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(reqwest::header::ACCEPT, "text/event-stream")
        .json(&body)
        .send();

    let resp = tokio::select! {
        biased;
        _ = wait_cancelled(&cancelled) => return Ok(()),
        sent = request => match sent {
            Ok(resp) => resp,
            Err(e) => {
                return Err(WisError::InvalidInput {
                    message: format!("request failed: {}", transport_error(&e)),
                })
            }
        },
    };

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let detail = resp.text().await.unwrap_or_default();
        let _ = on_event.send(StreamEvent::Error {
            message: format!("provider error {status}: {}", first_line(&detail, 300)),
        });

        return Ok(());
    }

    let mut stream = resp.bytes_stream();
    let mut parser = SseParser::new(provider);
    let mut emitted_terminal = false;

    'outer: loop {
        // Returning drops the response, which closes the upstream connection.
        let next = tokio::select! {
            biased;
            _ = wait_cancelled(&cancelled) => return Ok(()),
            chunk = stream.next() => chunk,
        };

        let Some(chunk) = next else { break };
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => {
                // A truncated or reset body arrives here.
                let _ = on_event.send(StreamEvent::Error {
                    message: format!("stream error: {}", transport_error(&e)),
                });
                emitted_terminal = true;
                break;
            }
        };

        for ev in parser.feed(&chunk) {
            let terminal = matches!(ev, StreamEvent::Done | StreamEvent::Error { .. });
            if on_event.send(ev).is_err() {
                // The consumer (webview channel) is gone; stop draining.
                return Ok(());
            }

            if terminal {
                emitted_terminal = true;
                break 'outer;
            }
        }
    }

    // A clean EOF with no terminal event.
    if !emitted_terminal {
        let _ = on_event.send(StreamEvent::Done);
    }

    Ok(())
}

fn invalid_key() -> WisError {
    WisError::InvalidInput {
        message: "API key contains invalid characters".into(),
    }
}

/// Anthropic's Messages endpoint and its auth headers. The key becomes a
/// header value marked sensitive and is never turned into a `String`.
fn anthropic_request(key: &[u8]) -> WisResult<(String, reqwest::header::HeaderMap)> {
    use reqwest::header::{HeaderMap, HeaderValue};

    let mut headers = HeaderMap::new();
    let mut value = HeaderValue::from_bytes(key).map_err(|_| invalid_key())?;
    value.set_sensitive(true);
    headers.insert("x-api-key", value);
    headers.insert(
        "anthropic-version",
        HeaderValue::from_static(ANTHROPIC_VERSION),
    );

    Ok((ANTHROPIC_URL.to_string(), headers))
}

/// An OpenAI-compatible endpoint and its auth headers. The bearer header is
/// omitted entirely when no key is stored (for local servers like Ollama or
/// LM Studio).
fn openai_request(
    base_url: Option<&str>,
    key: Option<&[u8]>,
) -> WisResult<(String, reqwest::header::HeaderMap)> {
    use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};

    let mut headers = HeaderMap::new();
    if let Some(key) = key {
        let mut bearer = Zeroizing::new(Vec::with_capacity(7 + key.len()));
        bearer.extend_from_slice(b"Bearer ");
        bearer.extend_from_slice(key);

        let mut value = HeaderValue::from_bytes(&bearer).map_err(|_| invalid_key())?;
        value.set_sensitive(true);
        headers.insert(AUTHORIZATION, value);
    }

    let base = base_url
        .map(str::trim)
        .filter(|b| !b.is_empty())
        .unwrap_or(OPENAI_DEFAULT_BASE);

    Ok((
        format!("{}/chat/completions", base.trim_end_matches('/')),
        headers,
    ))
}

/// Resolve once `flag` is set. Polled rather than notified so the cancel path
/// stays a plain atomic any thread can set, and so a request waiting on a slow
/// first token still notices a stop.
async fn wait_cancelled(flag: &AtomicBool) {
    while !flag.load(Ordering::Relaxed) {
        tokio::time::sleep(CANCEL_POLL_INTERVAL).await;
    }
}

/// A coarse transport-error category. Deliberately drops the full error chain
/// to prevent incidental leaks into an event or log.
fn transport_error(e: &reqwest::Error) -> &'static str {
    if e.is_timeout() {
        "timed out"
    } else if e.is_connect() {
        "could not connect"
    } else {
        "network error"
    }
}

/// The first line of `s`, char-safely truncated to `max` chars.
fn first_line(s: &str, max: usize) -> String {
    let line = s.lines().next().unwrap_or("").trim();
    if line.chars().count() > max {
        let truncated: String = line.chars().take(max).collect();
        format!("{truncated}…")
    } else {
        line.to_string()
    }
}
