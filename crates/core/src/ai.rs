//! Pure logic for the BYOK AI foundation: the provider model, request-body
//! shaping, and an incremental Server-Sent-Events parser that turns each
//! provider's stream into a common [`StreamEvent`] sequence. No I/O lives
//! here; the shell (`ai.rs`) owns the HTTP client, key access, and streaming.
//!
//! The API key never appears in this module: bodies carry only the model,
//! messages, and generation parameters. Authentication is a header the shell
//! adds at send time.

use serde::{Deserialize, Serialize};

/// The two supported provider wire protocols. `OpenAiCompatible` covers
/// OpenAI, Ollama, LM Studio, and OpenRouter via a configurable base URL.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiProvider {
    #[default]
    Anthropic,
    OpenAiCompatible,
}

/// A message role. `System` is a top-level field for Anthropic and a leading
/// message for OpenAI; [`chat_body`] handles the difference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    System,
    User,
    Assistant,
}

/// One turn in a conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }
}

/// What the request asks a provider to generate.
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub max_tokens: u32,
}

/// A normalized event from a provider's streaming response. Serializes with
/// an internal `type` tag (`delta` / `done` / `error`) so the frontend can
/// discriminate; never carries the API key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    /// A chunk of assistant text.
    Delta { text: String },
    /// The stream finished normally.
    Done,
    /// The provider reported an error mid-stream.
    Error { message: String },
}

fn role_wire(role: Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
    }
}

/// Build the JSON request body for `provider`. `stream` is always set; the
/// system prompt (the first `System` message, if any) is placed where each
/// provider expects it. Extra `System` messages after the first are folded
/// in as-is (OpenAI) or dropped (Anthropic, which allows only a top-level
/// system field).
pub fn chat_body(provider: AiProvider, req: &ChatRequest) -> serde_json::Value {
    let system = req
        .messages
        .iter()
        .find(|m| m.role == Role::System)
        .map(|m| m.content.clone());

    match provider {
        AiProvider::Anthropic => {
            let messages: Vec<serde_json::Value> = req
                .messages
                .iter()
                .filter(|m| m.role != Role::System)
                .map(|m| serde_json::json!({ "role": role_wire(m.role), "content": m.content }))
                .collect();
            let mut body = serde_json::json!({
                "model": req.model,
                "max_tokens": req.max_tokens,
                "stream": true,
                "messages": messages,
            });
            if let Some(system) = system {
                body["system"] = serde_json::Value::String(system);
            }

            body
        }
        AiProvider::OpenAiCompatible => {
            let messages: Vec<serde_json::Value> = req
                .messages
                .iter()
                .map(|m| serde_json::json!({ "role": role_wire(m.role), "content": m.content }))
                .collect();
            serde_json::json!({
                "model": req.model,
                "max_tokens": req.max_tokens,
                "stream": true,
                "messages": messages,
            })
        }
    }
}

/// Upper bound on a single un-terminated SSE line held in the parser's
/// buffer. Real lines are one token or one small JSON object; anything past
/// this is a broken or hostile stream and is refused rather than buffered.
const MAX_LINE_BYTES: usize = 1 << 20; // 1 MiB

/// Incremental parser turning a provider's raw SSE byte stream into
/// [`StreamEvent`]s. Feed it chunks as they arrive; it buffers partial lines
/// so a token split across a chunk boundary is still emitted whole.
pub struct SseParser {
    provider: AiProvider,
    /// Bytes received but not yet terminated by a newline.
    buf: Vec<u8>,
    /// Latched once a terminal event was seen, so trailing noise is ignored.
    finished: bool,
}

impl SseParser {
    pub fn new(provider: AiProvider) -> Self {
        Self {
            provider,
            buf: Vec::new(),
            finished: false,
        }
    }

    /// Consume `chunk`, returning every event completed by it. Only whole
    /// lines (terminated by `\n`) are parsed; a trailing partial line is
    /// retained for the next call. Bytes stop accumulating once a terminal
    /// event has been seen.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<StreamEvent> {
        let mut events = Vec::new();
        if self.finished {
            return events;
        }

        self.buf.extend_from_slice(chunk);

        while let Some(nl) = self.buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=nl).collect();
            // Trim the trailing \n and any \r (CRLF servers).
            let line = String::from_utf8_lossy(&line);
            let line = line.trim_end_matches(['\n', '\r']);
            if let Some(event) = self.parse_line(line) {
                let done = matches!(event, StreamEvent::Done | StreamEvent::Error { .. });
                events.push(event);
                if done {
                    self.finished = true;
                    return events;
                }
            }
        }

        // Guard against a runaway un-terminated line.
        if self.buf.len() > MAX_LINE_BYTES {
            self.finished = true;
            self.buf = Vec::new();
            events.push(StreamEvent::Error {
                message: "malformed stream: line too long".into(),
            });
        }

        events
    }

    fn parse_line(&self, line: &str) -> Option<StreamEvent> {
        if self.finished {
            return None;
        }

        let data = line.strip_prefix("data:")?.trim_start();
        if data.is_empty() {
            return None;
        }

        match self.provider {
            AiProvider::OpenAiCompatible if data == "[DONE]" => Some(StreamEvent::Done),
            _ => {
                let json: serde_json::Value = serde_json::from_str(data).ok()?;
                match self.provider {
                    AiProvider::Anthropic => parse_anthropic(&json),
                    AiProvider::OpenAiCompatible => parse_openai(&json),
                }
            }
        }
    }
}

fn parse_anthropic(json: &serde_json::Value) -> Option<StreamEvent> {
    match json.get("type").and_then(|t| t.as_str()) {
        Some("content_block_delta") => {
            let delta = json.get("delta")?;
            if delta.get("type").and_then(|t| t.as_str()) == Some("text_delta") {
                let text = delta.get("text").and_then(|t| t.as_str())?;
                return Some(StreamEvent::Delta {
                    text: text.to_string(),
                });
            }

            None
        }
        Some("message_stop") => Some(StreamEvent::Done),
        Some("error") => {
            let message = json
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("provider error");
            Some(StreamEvent::Error {
                message: message.to_string(),
            })
        }
        // message_start, content_block_start/stop, message_delta, ping: no text.
        _ => None,
    }
}

fn parse_openai(json: &serde_json::Value) -> Option<StreamEvent> {
    if let Some(err) = json.get("error").filter(|e| !e.is_null()) {
        let message = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("provider error");
        return Some(StreamEvent::Error {
            message: message.to_string(),
        });
    }

    let choice = json.get("choices")?.as_array()?.first()?;
    let content = choice
        .get("delta")
        .and_then(|d| d.get("content"))
        .and_then(|c| c.as_str());

    match content {
        Some(text) if !text.is_empty() => Some(StreamEvent::Delta {
            text: text.to_string(),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_all(provider: AiProvider, chunks: &[&[u8]]) -> Vec<StreamEvent> {
        let mut p = SseParser::new(provider);
        let mut out = Vec::new();
        for c in chunks {
            out.extend(p.feed(c));
        }
        out
    }

    #[test]
    fn anthropic_stream_yields_deltas_then_done() {
        let stream = b"event: message_start\ndata: {\"type\":\"message_start\"}\n\n\
event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"Hel\"}}\n\n\
data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"lo\"}}\n\n\
data: {\"type\":\"ping\"}\n\n\
data: {\"type\":\"message_stop\"}\n\n";
        assert_eq!(
            feed_all(AiProvider::Anthropic, &[stream]),
            vec![
                StreamEvent::Delta { text: "Hel".into() },
                StreamEvent::Delta { text: "lo".into() },
                StreamEvent::Done
            ]
        );
    }

    #[test]
    fn openai_stream_yields_deltas_then_done() {
        let stream =
            b"data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"content\":\" there\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n\
data: [DONE]\n\n";
        assert_eq!(
            feed_all(AiProvider::OpenAiCompatible, &[stream]),
            vec![
                StreamEvent::Delta { text: "Hi".into() },
                StreamEvent::Delta {
                    text: " there".into()
                },
                StreamEvent::Done
            ]
        );
    }

    #[test]
    fn delta_split_across_chunk_boundaries_is_reassembled() {
        // The `data:` line arrives in three pieces, one splitting the JSON.
        let chunks: &[&[u8]] = &[
            b"data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_",
            b"delta\",\"text\":\"world\"}}\n",
            b"data: {\"type\":\"message_stop\"}\n",
        ];
        assert_eq!(
            feed_all(AiProvider::Anthropic, chunks),
            vec![
                StreamEvent::Delta {
                    text: "world".into()
                },
                StreamEvent::Done
            ]
        );
    }

    #[test]
    fn in_stream_error_surfaces_and_latches() {
        let anthropic = b"data: {\"type\":\"error\",\"error\":{\"message\":\"overloaded\"}}\n\n\
data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"ignored\"}}\n\n";
        assert_eq!(
            feed_all(AiProvider::Anthropic, &[anthropic]),
            vec![StreamEvent::Error {
                message: "overloaded".into()
            }]
        );

        let openai = b"data: {\"error\":{\"message\":\"bad key\"}}\n\n";
        assert_eq!(
            feed_all(AiProvider::OpenAiCompatible, &[openai]),
            vec![StreamEvent::Error {
                message: "bad key".into()
            }]
        );
    }

    #[test]
    fn crlf_line_endings_parse() {
        let stream = b"data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\r\ndata: [DONE]\r\n";
        assert_eq!(
            feed_all(AiProvider::OpenAiCompatible, &[stream]),
            vec![StreamEvent::Delta { text: "x".into() }, StreamEvent::Done]
        );
    }

    #[test]
    fn openai_error_null_is_not_treated_as_an_error() {
        // A success chunk that carries `"error": null` alongside `choices`
        // must yield the delta, not a spurious terminal error.
        let stream =
            b"data: {\"error\":null,\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}\n\ndata: [DONE]\n\n";
        assert_eq!(
            feed_all(AiProvider::OpenAiCompatible, &[stream]),
            vec![StreamEvent::Delta { text: "Hi".into() }, StreamEvent::Done]
        );
    }

    #[test]
    fn a_runaway_unterminated_line_is_refused_not_buffered() {
        let mut p = SseParser::new(AiProvider::OpenAiCompatible);
        // Feed more than MAX_LINE_BYTES with no newline.
        let mut events = Vec::new();
        for _ in 0..20 {
            events.extend(p.feed(&vec![b'x'; 100 * 1024]));
        }
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], StreamEvent::Error { .. }));
        // Latched: further feeds produce nothing and do not grow.
        assert!(p.feed(b"data: [DONE]\n\n").is_empty());
    }

    #[test]
    fn anthropic_body_lifts_system_out_of_messages() {
        let req = ChatRequest {
            model: "claude-x".into(),
            max_tokens: 1024,
            messages: vec![
                ChatMessage {
                    role: Role::System,
                    content: "be brief".into(),
                },
                ChatMessage::user("hi"),
            ],
        };
        let body = chat_body(AiProvider::Anthropic, &req);
        assert_eq!(body["system"], "be brief");
        assert_eq!(body["stream"], true);
        assert_eq!(body["max_tokens"], 1024);
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1, "system is not a message for anthropic");
        assert_eq!(messages[0]["role"], "user");
    }

    #[test]
    fn openai_body_keeps_system_as_leading_message() {
        let req = ChatRequest {
            model: "gpt-x".into(),
            max_tokens: 1024,
            messages: vec![
                ChatMessage {
                    role: Role::System,
                    content: "be brief".into(),
                },
                ChatMessage::user("hi"),
            ],
        };
        let body = chat_body(AiProvider::OpenAiCompatible, &req);
        assert!(body.get("system").is_none());
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[1]["role"], "user");
    }
}
