//! BYOK AI foundation, imperative shell: API-key storage commands and the
//! streaming chat command. Pure request-shaping and SSE parsing live in
//! [`wis_core::ai::chat`].
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
use wis_core::ai::chat::chat_body;
use wis_core::{
    AiProvider, AiSettings, ChatMessage, ChatRequest, Role, SseParser, StreamEvent, ThinkingMode,
    WisError, WisResult, WorkspaceId,
};
use zeroize::Zeroizing;

use crate::state::AppState;

/// Bounds only the reach for the provider. Generation itself is deliberately
/// unbounded. Stopping is handled via `ai_cancel`.
const AI_CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// How often a waiting request notices that it was cancelled.
const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(250);
const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_MODELS_URL: &str = "https://api.anthropic.com/v1/models?limit=1000";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const OPENAI_DEFAULT_BASE: &str = "https://api.openai.com/v1";

/// Tauri event carrying the new key-present state after a set or clear.
const AI_KEY_EVENT: &str = "wis-ai-key-changed";

/// Platforms with an OS key store to seal the provider key in: macOS
/// (Keychain) and Windows (DPAPI). Elsewhere the assistant stays off rather
/// than holding the key somewhere weaker.
pub(crate) fn ai_supported() -> bool {
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
    #[serde(default)]
    pub tool_calls: Vec<wis_core::ai::chat::ToolCall>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
    /// An Anthropic tool-use turn as streamed, echoed back verbatim.
    #[serde(default)]
    pub content_blocks: Vec<serde_json::Value>,
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
    /// Forbid thinking for this request regardless of the show-reasoning setting.
    #[serde(default)]
    pub skip_reasoning: bool,
    /// Withhold web tools for this request.
    #[serde(default)]
    pub no_tools: bool,
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

/// Assess a link the way navguard's heuristics would, with the anchor text
/// included.
#[tauri::command]
pub async fn ai_assess_link(
    url: String,
    anchor_text: Option<String>,
) -> wis_core::navigation::link_safety::LinkAssessment {
    wis_core::navigation::link_safety::assess_link(&url, anchor_text.as_deref())
}

/// Stop an in-flight completion, closing the upstream connection rather than
/// only muting the reply. A no-op for an id that already finished.
#[tauri::command]
pub async fn ai_cancel(state: State<'_, AppState>, request_id: String) -> WisResult<()> {
    cancel(&state, &request_id);
    Ok(())
}

/// Whether a request by that id was running to stop.
pub(crate) fn cancel(state: &AppState, request_id: &str) -> bool {
    let Ok(map) = state.ai_cancels.lock() else {
        return false;
    };
    let Some(flag) = map.get(request_id) else {
        return false;
    };
    flag.store(true, Ordering::Relaxed);
    true
}

/// Whether a request could run: a supported platform, a model, and a stored key
/// or an OpenAI-compatible endpoint (local servers need no key).
pub(crate) fn ready(ai: &AiSettings) -> bool {
    if !ai_supported() || ai.model.trim().is_empty() {
        return false;
    }

    let endpoint = ai.provider == AiProvider::OpenAiCompatible
        && ai
            .base_url
            .as_deref()
            .is_some_and(|base| !base.trim().is_empty());
    endpoint || wis_secrets::ai_key_load().is_ok_and(|key| key.is_some())
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
    let job = ChatJob {
        workspace: args.workspace_id,
        request_id: args.request_id,
        messages: args.messages,
        max_tokens: args.max_tokens,
        skip_reasoning: args.skip_reasoning,
        no_tools: args.no_tools,
        model: None,
    };
    let tools = |ai: &AiSettings| ChatTools {
        web: ai.web_access,
        local: ai.local_data,
        action: ai.local_data && ai.tab_actions,
        canvas: false,
    };
    stream_chat(&state, job, tools, &mut |ev| on_event.send(ev).is_ok()).await
}

/// Which tools a request may offer, decided by its caller from the settings.
pub(crate) struct ChatTools {
    pub web: bool,
    pub local: bool,
    pub action: bool,
    pub canvas: bool,
}

/// One completion: the conversation and the per-request switches.
pub(crate) struct ChatJob {
    pub workspace: WorkspaceId,
    pub request_id: String,
    pub messages: Vec<AiMessage>,
    pub max_tokens: u32,
    pub skip_reasoning: bool,
    pub no_tools: bool,
    /// A model other than the settings' own.
    pub model: Option<String>,
}

/// Streams one completion to `emit`, which returns false once its consumer is
/// gone. Shared by the assistant panel and canvas agents.
pub(crate) async fn stream_chat(
    state: &AppState,
    job: ChatJob,
    tools: impl FnOnce(&AiSettings) -> ChatTools,
    emit: &mut (dyn FnMut(StreamEvent) -> bool + Send),
) -> WisResult<()> {
    if !ai_supported() {
        return Err(unsupported());
    }

    let running = state
        .ai_cancels
        .lock()
        .is_ok_and(|map| map.contains_key(&job.request_id));
    if running {
        return Err(WisError::InvalidInput {
            message: "that AI request is already running".into(),
        });
    }
    let cancelled = CancelGuard::register(state, &job.request_id);
    let _cancel_guard = CancelGuard {
        state,
        request_id: job.request_id.clone(),
    };

    let (provider, model, base_url, show_reasoning, tools) = {
        let browser = state.browser.lock().await;
        let ai = &browser.settings.ai;
        let model = job
            .model
            .clone()
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| ai.model.clone());
        (
            ai.provider,
            model,
            ai.base_url.clone(),
            ai.show_reasoning,
            tools(ai),
        )
    };
    if model.trim().is_empty() {
        return Err(WisError::InvalidInput {
            message: "no AI model configured".into(),
        });
    }

    let local_server =
        provider == AiProvider::OpenAiCompatible && local_openai_base(base_url.as_deref());

    let thinking = thinking_for(provider, show_reasoning, job.skip_reasoning);
    let max_tokens = if thinking == ThinkingMode::Summarized {
        job.max_tokens.max(16000)
    } else {
        job.max_tokens
    };
    let req = ChatRequest {
        model,
        max_tokens,
        web_tools: tools.web,
        local_tools: tools.local,
        action_tools: tools.action,
        canvas_tools: tools.canvas,
        tools_closed: job.no_tools || job.skip_reasoning,
        local_server,
        thinking,
        messages: job
            .messages
            .into_iter()
            .map(|m| ChatMessage {
                role: m.role,
                content: m.content,
                tool_calls: m.tool_calls,
                tool_call_id: m.tool_call_id,
                content_blocks: m.content_blocks,
            })
            .collect(),
    };
    let body = chat_body(provider, &req);

    let Endpoint {
        url,
        headers,
        host: endpoint_host,
        client,
    } = endpoint(
        state,
        job.workspace,
        provider,
        base_url.as_deref(),
        Route::Chat,
    )
    .await?;

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
                    message: format!(
                        "request failed: {} ({endpoint_host})",
                        transport_error(&e)
                    ),
                })
            }
        },
    };

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let detail = resp.text().await.unwrap_or_default();
        let _ = emit(StreamEvent::Error {
            message: format!(
                "provider error {status} from {endpoint_host}: {}",
                first_line(&detail, 300)
            ),
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
                let _ = emit(StreamEvent::Error {
                    message: format!("stream error: {} ({endpoint_host})", transport_error(&e)),
                });
                emitted_terminal = true;
                break;
            }
        };

        for ev in parser.feed(&chunk) {
            let terminal = matches!(ev, StreamEvent::Done | StreamEvent::Error { .. });
            if !emit(ev) {
                // The consumer is gone; stop draining.
                return Ok(());
            }

            if terminal {
                emitted_terminal = true;
                break 'outer;
            }
        }
    }

    // A clean EOF with no terminal event: drain any tool round the stream
    // handed over without a finish marker, then close.
    if !emitted_terminal {
        for ev in parser.finish() {
            if !emit(ev) {
                return Ok(());
            }
        }

        let _ = emit(StreamEvent::Done);
    }

    Ok(())
}

/// Which provider endpoint a request is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Route {
    Chat,
    Models,
}

/// Where a provider request goes, how it authenticates, and the client that
/// sends it.
pub(crate) struct Endpoint {
    pub url: String,
    pub headers: reqwest::header::HeaderMap,
    pub host: String,
    pub client: reqwest::Client,
}

/// A local server never sees the key and is dialled direct; everything else
/// carries the key as a sensitive header through the workspace's proxy.
pub(crate) async fn endpoint(
    state: &AppState,
    workspace: WorkspaceId,
    provider: AiProvider,
    base_url: Option<&str>,
    route: Route,
) -> WisResult<Endpoint> {
    let local_server = provider == AiProvider::OpenAiCompatible && local_openai_base(base_url);
    let key = if local_server {
        None
    } else {
        wis_secrets::ai_key_load().map_err(|e| WisError::InvalidInput {
            message: format!("could not read key: {e}"),
        })?
    };
    let key = key.as_ref().map(|k| k.as_slice());
    let (url, mut headers) = match provider {
        AiProvider::Anthropic => {
            let key = key.ok_or_else(|| WisError::InvalidInput {
                message: "no AI key configured".into(),
            })?;
            anthropic_request(key)?
        }
        AiProvider::OpenAiCompatible => openai_request(base_url, key)?,
    };
    let url = match (route, provider) {
        (Route::Chat, _) => url,
        (Route::Models, AiProvider::Anthropic) => ANTHROPIC_MODELS_URL.to_string(),
        (Route::Models, AiProvider::OpenAiCompatible) => {
            format!("{}/models", url.trim_end_matches("/chat/completions"))
        }
    };

    let parsed_url = url::Url::parse(&url).ok();
    let local_endpoint = parsed_url
        .as_ref()
        .map(wis_network::is_loopback)
        .unwrap_or(false);
    if local_endpoint {
        headers.remove(reqwest::header::AUTHORIZATION);
    }
    let host = parsed_url
        .as_ref()
        .and_then(|u| u.host_str())
        .unwrap_or("the provider")
        .to_string();

    let mut builder = reqwest::Client::builder().connect_timeout(AI_CONNECT_TIMEOUT);
    if !local_endpoint {
        let proxy_port =
            state
                .ensure_proxy_for(workspace)
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

    Ok(Endpoint {
        url,
        headers,
        host,
        client,
    })
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

/// How a request treats reasoning. Answer now skips it on OpenAI-compatible
/// models; Anthropic keeps the turn's thinking and ends the turn with closed
/// tools and the answer-now directive.
fn thinking_for(provider: AiProvider, show_reasoning: bool, skip_reasoning: bool) -> ThinkingMode {
    match provider {
        AiProvider::OpenAiCompatible if skip_reasoning => ThinkingMode::Disabled,
        AiProvider::OpenAiCompatible => ThinkingMode::ProviderDefault,
        AiProvider::Anthropic if show_reasoning => ThinkingMode::Summarized,
        AiProvider::Anthropic => ThinkingMode::ProviderDefault,
    }
}

/// Whether an OpenAI-compatible base URL points at this machine.
fn local_openai_base(base_url: Option<&str>) -> bool {
    base_url
        .and_then(|base| url::Url::parse(base.trim()).ok())
        .is_some_and(|url| wis_network::is_loopback(&url))
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
pub(crate) fn transport_error(e: &reqwest::Error) -> &'static str {
    if e.is_timeout() {
        "timed out"
    } else if e.is_connect() {
        "could not connect"
    } else {
        "network error"
    }
}

/// The first line of `s`, char-safely truncated to `max` chars.
pub(crate) fn first_line(s: &str, max: usize) -> String {
    let line = s.lines().next().unwrap_or("").trim();
    if line.chars().count() > max {
        let truncated: String = line.chars().take(max).collect();
        format!("{truncated}…")
    } else {
        line.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answer_now_skips_reasoning_only_where_the_provider_allows_it() {
        use AiProvider::{Anthropic, OpenAiCompatible};
        assert_eq!(
            thinking_for(OpenAiCompatible, false, true),
            ThinkingMode::Disabled
        );
        assert_eq!(
            thinking_for(OpenAiCompatible, true, false),
            ThinkingMode::ProviderDefault
        );
        assert_eq!(
            thinking_for(Anthropic, true, false),
            ThinkingMode::Summarized
        );
        assert_eq!(
            thinking_for(Anthropic, true, true),
            ThinkingMode::Summarized
        );
        assert_eq!(
            thinking_for(Anthropic, false, true),
            ThinkingMode::ProviderDefault
        );
    }

    #[test]
    fn only_a_loopback_openai_base_skips_the_key() {
        assert!(local_openai_base(Some("http://127.0.0.1:11434/v1")));
        assert!(local_openai_base(Some(" http://localhost:1234/v1/ ")));
        assert!(!local_openai_base(Some("https://openrouter.ai/api/v1")));
        assert!(!local_openai_base(Some("http://192.168.1.20:11434/v1")));
        assert!(!local_openai_base(Some("localhost:11434")));
        assert!(!local_openai_base(None));
    }
}
