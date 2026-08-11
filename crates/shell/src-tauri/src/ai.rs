//! BYOK AI foundation, imperative shell: API-key storage commands and the
//! streaming chat command. Pure request-shaping and SSE parsing live in
//! [`wis_core::ai`].
//!
//! Key hygiene: the API key is loaded from the OS keystore per request,
//! kept in `Zeroizing` bytes, and used only as an auth header. It is never
//! stringified into logs, events, or error messages. Transport errors are
//! reduced to a coarse category so nothing incidental leaks. Requests route
//! through the workspace loopback proxy with upstream TLS verification ON.

use futures_util::StreamExt;
use serde::Deserialize;
use tauri::ipc::Channel;
use tauri::State;
use wis_core::ai::chat_body;
use wis_core::{
    AiProvider, ChatMessage, ChatRequest, Role, SseParser, StreamEvent, WisError, WisResult,
    WorkspaceId,
};
use zeroize::Zeroizing;

use crate::state::AppState;

const AI_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
/// Per-read inactivity bound. A live stream sends tokens continuously; a gap
/// this long means the provider stalled. Bounds a hung stream without capping
/// a legitimately long completion.
const AI_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const OPENAI_DEFAULT_BASE: &str = "https://api.openai.com/v1";

// ---------------------------------------------------------------------------
// Key storage commands
// ---------------------------------------------------------------------------

/// Store the provider API key in the OS keystore (non-biometric). Trims
/// surrounding whitespace and rejects an empty key. Never echoes it back.
#[tauri::command]
pub async fn ai_set_key(key: String) -> WisResult<()> {
    let key = Zeroizing::new(key);
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return Err(WisError::InvalidInput {
            message: "API key is empty".into(),
        });
    }

    wis_secrets::ai_key_store(trimmed.as_bytes()).map_err(|e| WisError::InvalidInput {
        message: format!("could not store key: {e}"),
    })
}

/// Whether a provider API key is currently stored.
#[tauri::command]
pub async fn ai_key_present() -> WisResult<bool> {
    wis_secrets::ai_key_load()
        .map(|k| k.is_some())
        .map_err(|e| WisError::InvalidInput {
            message: format!("could not check key: {e}"),
        })
}

/// Remove the stored provider API key (idempotent).
#[tauri::command]
pub async fn ai_clear_key() -> WisResult<()> {
    wis_secrets::ai_key_delete().map_err(|e| WisError::InvalidInput {
        message: format!("could not clear key: {e}"),
    })
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
}

fn default_max_tokens() -> u32 {
    1024
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
    let key = wis_secrets::ai_key_load()
        .map_err(|e| WisError::InvalidInput {
            message: format!("could not read key: {e}"),
        })?
        .ok_or_else(|| WisError::InvalidInput {
            message: "no AI key configured".into(),
        })?;

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
    let client = reqwest::Client::builder()
        .connect_timeout(AI_CONNECT_TIMEOUT)
        .read_timeout(AI_READ_TIMEOUT)
        .proxy(proxy)
        .build()
        .map_err(|e| WisError::InvalidInput {
            message: format!("http client: {e}"),
        })?;

    let (url, headers) = build_request_headers(provider, base_url.as_deref(), &key)?;

    let resp = client
        .post(url)
        .headers(headers)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(reqwest::header::ACCEPT, "text/event-stream")
        .json(&body)
        .send()
        .await
        .map_err(|e| WisError::InvalidInput {
            message: format!("request failed: {}", transport_error(&e)),
        })?;

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

    'outer: while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => {
                // A read stall (read_timeout) or truncated body arrives here.
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

/// Build the endpoint URL and auth headers for `provider`. The key becomes a
/// header value marked sensitive and is never turned into a `String`.
fn build_request_headers(
    provider: AiProvider,
    base_url: Option<&str>,
    key: &[u8],
) -> WisResult<(String, reqwest::header::HeaderMap)> {
    use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};

    let invalid_key = || WisError::InvalidInput {
        message: "API key contains invalid characters".into(),
    };
    let mut headers = HeaderMap::new();

    let url = match provider {
        AiProvider::Anthropic => {
            let mut v = HeaderValue::from_bytes(key).map_err(|_| invalid_key())?;
            v.set_sensitive(true);
            headers.insert("x-api-key", v);
            headers.insert(
                "anthropic-version",
                HeaderValue::from_static(ANTHROPIC_VERSION),
            );

            ANTHROPIC_URL.to_string()
        }
        AiProvider::OpenAiCompatible => {
            let mut bearer = Zeroizing::new(Vec::with_capacity(7 + key.len()));
            bearer.extend_from_slice(b"Bearer ");
            bearer.extend_from_slice(key);

            let mut v = HeaderValue::from_bytes(&bearer).map_err(|_| invalid_key())?;
            v.set_sensitive(true);
            headers.insert(AUTHORIZATION, v);

            let base = base_url
                .map(str::trim)
                .filter(|b| !b.is_empty())
                .unwrap_or(OPENAI_DEFAULT_BASE);
            format!("{}/chat/completions", base.trim_end_matches('/'))
        }
    };

    Ok((url, headers))
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
