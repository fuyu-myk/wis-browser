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
    /// OpenAI-compatible wire only.
    Tool,
}

/// One tool invocation the model asked for, or is being answered about.
/// `arguments` stays a raw JSON string end to end, exactly as the wire
/// carries it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// One turn in a conversation. `tool_calls` rides an assistant turn that
/// requested tools.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }
}

/// How a request treats the model's reasoning, Anthropic only.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ThinkingMode {
    /// Whatever the model does by default.
    #[default]
    ProviderDefault,
    /// Ask for adaptive thinking with a summarized display.
    Summarized,
    /// Explicitly forbid thinking.
    Disabled,
}

/// What the request asks a provider to generate.
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    /// Ceiling on generated tokens. Required by Anthropic.
    pub max_tokens: u32,
    /// Offer the web tools (OpenAI-compatible arm only).
    pub web_tools: bool,
    pub thinking: ThinkingMode,
}

/// A normalized event from a provider's streaming response. Serializes with
/// an internal `type` tag (`delta` / `reasoning` / `done` / `error`) so the
/// frontend can discriminate; never carries the API key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    /// A chunk of assistant text.
    Delta { text: String },
    /// A chunk of the model's reasoning. Kept separate from
    /// [`StreamEvent::Delta`].
    Reasoning { text: String },
    /// The model asked for tools instead of (or before) answering. Emitted
    /// once per request round, after every fragment has been reassembled.
    ToolCalls { calls: Vec<ToolCall> },
    /// An incomplete answer.
    Truncated,
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
        Role::Tool => "tool",
    }
}

/// The tools the assistant may offer a model. Nothing here can send data
/// anywhere, except for the search engine and the fetched page itself.
fn web_tool_definitions() -> serde_json::Value {
    serde_json::json!([
        {
            "type": "function",
            "function": {
                "name": "web_search",
                "description": "Search the web. Use for current events, recent facts, or anything after your training data. Returns titles, URLs and snippets.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "The search query." }
                    },
                    "required": ["query"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "web_fetch",
                "description": "Fetch a web page and return its readable text. Only URLs from web_search results or from the user's message are allowed.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "url": { "type": "string", "description": "The http(s) URL to fetch." }
                    },
                    "required": ["url"]
                }
            }
        }
    ])
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
                .filter(|m| {
                    m.role != Role::System
                        && m.role != Role::Tool
                        && !(m.role == Role::Assistant
                            && m.content.is_empty()
                            && !m.tool_calls.is_empty())
                })
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
            match req.thinking {
                ThinkingMode::ProviderDefault => {}
                ThinkingMode::Summarized => {
                    body["thinking"] = serde_json::json!({
                        "type": "adaptive",
                        "display": "summarized",
                    });
                }
                ThinkingMode::Disabled => {
                    body["thinking"] = serde_json::json!({ "type": "disabled" });
                }
            }

            body
        }
        AiProvider::OpenAiCompatible => {
            // Best-effort no thinking switch.
            let last_user = req
                .messages
                .iter()
                .rposition(|m| m.role == Role::User)
                .filter(|_| req.thinking == ThinkingMode::Disabled);
            let messages: Vec<serde_json::Value> = req
                .messages
                .iter()
                .enumerate()
                .map(|(i, m)| {
                    let content = if last_user == Some(i) {
                        format!("{}\n\n/no_think", m.content)
                    } else {
                        m.content.clone()
                    };
                    let mut msg =
                        serde_json::json!({ "role": role_wire(m.role), "content": content });

                    if !m.tool_calls.is_empty() {
                        msg["tool_calls"] = m
                            .tool_calls
                            .iter()
                            .map(|c| {
                                serde_json::json!({
                                    "id": c.id,
                                    "type": "function",
                                    "function": { "name": c.name, "arguments": c.arguments },
                                })
                            })
                            .collect();
                    }
                    if let Some(id) = &m.tool_call_id {
                        msg["tool_call_id"] = serde_json::Value::String(id.clone());
                    }

                    msg
                })
                .collect();
            let mut body = serde_json::json!({
                "model": req.model,
                "stream": true,
                "messages": messages,
            });

            // Tools are never offered on an answer-now retry.
            if req.web_tools && req.thinking != ThinkingMode::Disabled {
                body["tools"] = web_tool_definitions();
            }

            body
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
    /// Tool-call fragments accumulated by wire index; the wire streams a
    /// call's id, name and argument text across many chunks.
    pending_tools: Vec<ToolCall>,
    /// Set once the accumulated calls were emitted, so EOF cannot re-emit.
    tools_flushed: bool,
}

impl SseParser {
    pub fn new(provider: AiProvider) -> Self {
        Self {
            provider,
            buf: Vec::new(),
            finished: false,
            pending_tools: Vec::new(),
            tools_flushed: false,
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
            for event in self.parse_line(line) {
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

    /// Events completed by one SSE line. A chunk can carry both reasoning and
    /// answer text, so this yields a list rather than at most one event.
    fn parse_line(&mut self, line: &str) -> Vec<StreamEvent> {
        if self.finished {
            return Vec::new();
        }

        let Some(data) = line.strip_prefix("data:").map(str::trim_start) else {
            return Vec::new();
        };
        if data.is_empty() {
            return Vec::new();
        }

        if self.provider == AiProvider::OpenAiCompatible && data == "[DONE]" {
            let mut events = self.flush_tools();
            events.push(StreamEvent::Done);
            return events;
        }

        let Ok(json) = serde_json::from_str::<serde_json::Value>(data) else {
            return Vec::new();
        };
        match self.provider {
            AiProvider::Anthropic => parse_anthropic(&json),
            AiProvider::OpenAiCompatible => self.parse_openai(&json),
        }
    }

    /// Drain what a closed stream left behind. A server that reaches EOF
    /// without `[DONE]` or a `tool_calls` finish marker may still have handed
    /// over a complete round.
    pub fn finish(&mut self) -> Vec<StreamEvent> {
        self.flush_tools()
    }

    /// Emit the reassembled tool calls once, dropping fragments that never
    /// received a name.
    fn flush_tools(&mut self) -> Vec<StreamEvent> {
        if self.tools_flushed || self.pending_tools.is_empty() {
            return Vec::new();
        }

        self.tools_flushed = true;
        let calls: Vec<ToolCall> = std::mem::take(&mut self.pending_tools)
            .into_iter()
            .filter(|c| !c.name.is_empty())
            .enumerate()
            .map(|(i, mut c)| {
                if c.id.is_empty() {
                    c.id = format!("call_{i}");
                }

                c
            })
            .collect();
        if calls.is_empty() {
            return Vec::new();
        }

        vec![StreamEvent::ToolCalls { calls }]
    }

    fn parse_openai(&mut self, json: &serde_json::Value) -> Vec<StreamEvent> {
        if let Some(err) = json.get("error").filter(|e| !e.is_null()) {
            let message = err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("provider error");
            return vec![StreamEvent::Error {
                message: message.to_string(),
            }];
        }

        let Some(choice) = json
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|c| c.first())
        else {
            return Vec::new();
        };
        let finish = choice.get("finish_reason").and_then(|f| f.as_str());

        let mut events = Vec::new();
        if let Some(delta) = choice.get("delta") {
            if let Some(fragments) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                for fragment in fragments {
                    self.accumulate_tool_fragment(fragment);
                }
            }

            let reasoning = delta_event(delta, "reasoning", |text| StreamEvent::Reasoning { text })
                .or_else(|| {
                    delta_event(delta, "reasoning_content", |text| StreamEvent::Reasoning {
                        text,
                    })
                });
            events.extend(reasoning);
            events.extend(delta_event(delta, "content", |text| StreamEvent::Delta {
                text,
            }));
        }

        match finish {
            Some("tool_calls") => events.extend(self.flush_tools()),
            Some("length") => {
                self.pending_tools.clear();
                events.push(StreamEvent::Truncated);
            }
            _ => {}
        }

        events
    }

    /// Fold one wire fragment into the pending call at its index. The id and
    /// name usually arrive on the first fragment, the argument text split
    /// across the rest.
    fn accumulate_tool_fragment(&mut self, fragment: &serde_json::Value) {
        let index = fragment
            .get("index")
            .and_then(|i| i.as_u64())
            .unwrap_or(self.pending_tools.len().saturating_sub(1) as u64)
            as usize;
        if index >= 32 {
            return;
        }

        while self.pending_tools.len() <= index {
            self.pending_tools.push(ToolCall {
                id: String::new(),
                name: String::new(),
                arguments: String::new(),
            });
        }

        let call = &mut self.pending_tools[index];
        if let Some(id) = fragment.get("id").and_then(|i| i.as_str()) {
            if !id.is_empty() {
                call.id = id.to_string();
            }
        }
        if let Some(function) = fragment.get("function") {
            if let Some(name) = function.get("name").and_then(|n| n.as_str()) {
                if !name.is_empty() {
                    call.name = name.to_string();
                }
            }
            if let Some(args) = function.get("arguments").and_then(|a| a.as_str()) {
                call.arguments.push_str(args);
            }
        }
    }
}

/// A non-empty string field of `delta`, as `make` wraps it.
fn delta_event(
    delta: &serde_json::Value,
    field: &str,
    make: fn(String) -> StreamEvent,
) -> Option<StreamEvent> {
    let text = delta.get(field).and_then(|v| v.as_str())?;
    if text.is_empty() {
        return None;
    }

    Some(make(text.to_string()))
}

fn parse_anthropic(json: &serde_json::Value) -> Vec<StreamEvent> {
    match json.get("type").and_then(|t| t.as_str()) {
        Some("content_block_delta") => {
            let Some(delta) = json.get("delta") else {
                return Vec::new();
            };

            match delta.get("type").and_then(|t| t.as_str()) {
                Some("text_delta") => {
                    delta_event(delta, "text", |text| StreamEvent::Delta { text })
                        .into_iter()
                        .collect()
                }
                Some("thinking_delta") => {
                    delta_event(delta, "thinking", |text| StreamEvent::Reasoning { text })
                        .into_iter()
                        .collect()
                }
                _ => Vec::new(),
            }
        }
        Some("message_delta") => {
            let capped = json
                .get("delta")
                .and_then(|d| d.get("stop_reason"))
                .and_then(|s| s.as_str())
                == Some("max_tokens");
            if capped {
                vec![StreamEvent::Truncated]
            } else {
                Vec::new()
            }
        }
        Some("message_stop") => vec![StreamEvent::Done],
        Some("error") => {
            let message = json
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("provider error");
            vec![StreamEvent::Error {
                message: message.to_string(),
            }]
        }
        // message_start, content_block_start/stop, message_delta, ping: no text.
        _ => Vec::new(),
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
    fn openai_reasoning_deltas_are_kept_apart_from_the_answer() {
        let stream = b"data: {\"choices\":[{\"delta\":{\"reasoning\":\"weigh\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"reasoning\":\" it\"}}]}\n\n\
data: {\"choices\":[{\"delta\":{\"content\":\"Yes\"}}]}\n\n\
data: [DONE]\n\n";
        assert_eq!(
            feed_all(AiProvider::OpenAiCompatible, &[stream]),
            vec![
                StreamEvent::Reasoning {
                    text: "weigh".into()
                },
                StreamEvent::Reasoning { text: " it".into() },
                StreamEvent::Delta { text: "Yes".into() },
                StreamEvent::Done
            ]
        );
    }

    #[test]
    fn openai_reasoning_content_is_the_same_event() {
        let stream = b"data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"hmm\"}}]}\n\n";
        assert_eq!(
            feed_all(AiProvider::OpenAiCompatible, &[stream]),
            vec![StreamEvent::Reasoning { text: "hmm".into() }]
        );
    }

    #[test]
    fn a_chunk_carrying_both_reasoning_and_content_yields_both() {
        let stream =
            b"data: {\"choices\":[{\"delta\":{\"reasoning\":\"so\",\"content\":\"Hi\"}}]}\n\n";
        assert_eq!(
            feed_all(AiProvider::OpenAiCompatible, &[stream]),
            vec![
                StreamEvent::Reasoning { text: "so".into() },
                StreamEvent::Delta { text: "Hi".into() }
            ]
        );
    }

    #[test]
    fn anthropic_thinking_deltas_parse_as_reasoning() {
        let stream = b"data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"step\"}}\n\n\
data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"signature_delta\",\"signature\":\"abc\"}}\n\n\
data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"Done\"}}\n\n";
        assert_eq!(
            feed_all(AiProvider::Anthropic, &[stream]),
            vec![
                StreamEvent::Reasoning {
                    text: "step".into()
                },
                StreamEvent::Delta {
                    text: "Done".into()
                }
            ]
        );
    }

    #[test]
    fn openai_tool_call_fragments_reassemble_across_chunks() {
        let chunks: &[&[u8]] = &[
            b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"web_search\",\"arguments\":\"{\\\"que\"}}]}}]}\n\n",
            b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"ry\\\": \\\"rust\\\"}\"}}]}}]}\n\n",
            b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            b"data: [DONE]\n\n",
        ];
        assert_eq!(
            feed_all(AiProvider::OpenAiCompatible, chunks),
            vec![
                StreamEvent::ToolCalls {
                    calls: vec![ToolCall {
                        id: "call_1".into(),
                        name: "web_search".into(),
                        arguments: "{\"query\": \"rust\"}".into(),
                    }]
                },
                StreamEvent::Done
            ]
        );
    }

    #[test]
    fn tool_calls_flush_on_eof_when_no_finish_marker_arrives() {
        let chunks: &[&[u8]] = &[
            b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c\",\"function\":{\"name\":\"web_fetch\",\"arguments\":\"{}\"}}]}}]}\n\n",
            b"data: [DONE]\n\n",
        ];
        let events = feed_all(AiProvider::OpenAiCompatible, chunks);
        assert!(matches!(events[0], StreamEvent::ToolCalls { .. }));
        assert_eq!(events[1], StreamEvent::Done);
    }

    #[test]
    fn a_nameless_tool_fragment_never_becomes_a_round() {
        let chunks: &[&[u8]] = &[
            b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"junk\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
            b"data: [DONE]\n\n",
        ];
        assert_eq!(
            feed_all(AiProvider::OpenAiCompatible, chunks),
            vec![StreamEvent::Done]
        );
    }

    #[test]
    fn openai_body_declares_tools_only_when_asked_and_not_forced() {
        let mut req = ChatRequest {
            model: "qwen3.5:9b".into(),
            max_tokens: 4096,
            web_tools: true,
            thinking: ThinkingMode::ProviderDefault,
            messages: vec![ChatMessage::user("hi")],
        };
        let body = chat_body(AiProvider::OpenAiCompatible, &req);
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0]["function"]["name"], "web_search");
        assert_eq!(tools[1]["function"]["name"], "web_fetch");

        // The answer-now retry must not be able to start another round.
        req.thinking = ThinkingMode::Disabled;
        let body = chat_body(AiProvider::OpenAiCompatible, &req);
        assert!(body.get("tools").is_none());

        req.thinking = ThinkingMode::ProviderDefault;
        req.web_tools = false;
        let body = chat_body(AiProvider::OpenAiCompatible, &req);
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn tool_turns_serialize_for_openai_and_are_dropped_for_anthropic() {
        let messages = vec![
            ChatMessage::user("what is new in rust"),
            ChatMessage {
                role: Role::Assistant,
                content: String::new(),
                tool_calls: vec![ToolCall {
                    id: "call_1".into(),
                    name: "web_search".into(),
                    arguments: "{\"query\":\"rust news\"}".into(),
                }],
                tool_call_id: None,
            },
            ChatMessage {
                role: Role::Tool,
                content: "results here".into(),
                tool_calls: Vec::new(),
                tool_call_id: Some("call_1".into()),
            },
        ];
        let req = ChatRequest {
            model: "qwen3.5:9b".into(),
            max_tokens: 4096,
            web_tools: true,
            thinking: ThinkingMode::ProviderDefault,
            messages,
        };

        let body = chat_body(AiProvider::OpenAiCompatible, &req);
        let wire = body["messages"].as_array().unwrap();
        assert_eq!(wire.len(), 3);
        assert_eq!(wire[1]["tool_calls"][0]["function"]["name"], "web_search");
        assert_eq!(wire[1]["tool_calls"][0]["type"], "function");
        assert_eq!(wire[2]["role"], "tool");
        assert_eq!(wire[2]["tool_call_id"], "call_1");

        let body = chat_body(AiProvider::Anthropic, &req);
        let wire = body["messages"].as_array().unwrap();
        assert_eq!(wire.len(), 1, "a tool round is not Anthropic-expressible");
        assert_eq!(wire[0]["role"], "user");
    }

    #[test]
    fn an_id_less_tool_call_gets_a_synthesized_id() {
        let chunks: &[&[u8]] = &[
            b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"name\":\"web_search\",\"arguments\":\"{}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
        ];
        let events = feed_all(AiProvider::OpenAiCompatible, chunks);
        let StreamEvent::ToolCalls { calls } = &events[0] else {
            panic!("expected tool calls, got {events:?}");
        };
        assert_eq!(calls[0].id, "call_0");
    }

    #[test]
    fn an_index_less_continuation_extends_the_call_underway() {
        let chunks: &[&[u8]] = &[
            b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"web_search\",\"arguments\":\"{\\\"query\\\":\"}}]}}]}\n\n",
            b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"function\":{\"arguments\":\"\\\"rust\\\"}\"}}]}}]}\n\n",
            b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
        ];
        let events = feed_all(AiProvider::OpenAiCompatible, chunks);
        let StreamEvent::ToolCalls { calls } = &events[0] else {
            panic!("expected tool calls, got {events:?}");
        };
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].arguments, "{\"query\":\"rust\"}");
    }

    #[test]
    fn length_truncation_discards_half_built_tool_calls() {
        let chunks: &[&[u8]] = &[
            b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c\",\"function\":{\"name\":\"web_fetch\",\"arguments\":\"{\\\"ur\"}}]},\"finish_reason\":\"length\"}]}\n\n",
            b"data: [DONE]\n\n",
        ];
        assert_eq!(
            feed_all(AiProvider::OpenAiCompatible, chunks),
            vec![StreamEvent::Truncated, StreamEvent::Done]
        );
    }

    #[test]
    fn finish_flushes_a_round_the_stream_never_terminated() {
        let mut parser = SseParser::new(AiProvider::OpenAiCompatible);
        let _ = parser.feed(
            b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c\",\"function\":{\"name\":\"web_search\",\"arguments\":\"{}\"}}]}}]}\n\n",
        );
        let events = parser.finish();
        assert!(matches!(events[0], StreamEvent::ToolCalls { .. }));
        assert!(parser.finish().is_empty(), "finish flushes only once");
    }

    #[test]
    fn openai_length_finish_surfaces_truncation() {
        let stream = b"data: {\"choices\":[{\"delta\":{\"content\":\"partial\"},\"finish_reason\":\"length\"}]}\n\ndata: [DONE]\n\n";
        assert_eq!(
            feed_all(AiProvider::OpenAiCompatible, &[stream]),
            vec![
                StreamEvent::Delta {
                    text: "partial".into()
                },
                StreamEvent::Truncated,
                StreamEvent::Done
            ]
        );
    }

    #[test]
    fn openai_stop_finish_is_not_truncation() {
        let stream =
            b"data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
        assert_eq!(
            feed_all(AiProvider::OpenAiCompatible, &[stream]),
            vec![StreamEvent::Done]
        );
    }

    #[test]
    fn anthropic_max_tokens_stop_reason_surfaces_truncation() {
        let stream = b"data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"max_tokens\"},\"usage\":{\"output_tokens\":4096}}\n\ndata: {\"type\":\"message_stop\"}\n\n";
        assert_eq!(
            feed_all(AiProvider::Anthropic, &[stream]),
            vec![StreamEvent::Truncated, StreamEvent::Done]
        );
    }

    #[test]
    fn anthropic_end_turn_stop_reason_is_not_truncation() {
        let stream = b"data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\ndata: {\"type\":\"message_stop\"}\n\n";
        assert_eq!(
            feed_all(AiProvider::Anthropic, &[stream]),
            vec![StreamEvent::Done]
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
            web_tools: false,
            thinking: ThinkingMode::ProviderDefault,
            messages: vec![
                ChatMessage {
                    role: Role::System,
                    content: "be brief".into(),
                    tool_calls: Vec::new(),
                    tool_call_id: None,
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
    fn openai_body_sends_no_token_ceiling() {
        let req = ChatRequest {
            model: "qwen3".into(),
            max_tokens: 1024,
            web_tools: false,
            thinking: ThinkingMode::ProviderDefault,
            messages: vec![ChatMessage::user("hi")],
        };
        let body = chat_body(AiProvider::OpenAiCompatible, &req);
        assert!(
            body.get("max_tokens").is_none(),
            "capping this cuts a reasoning model off mid-thought"
        );
        assert_eq!(body["stream"], true);
    }

    #[test]
    fn anthropic_body_still_sends_the_required_token_ceiling() {
        let req = ChatRequest {
            model: "claude-x".into(),
            max_tokens: 2048,
            web_tools: false,
            thinking: ThinkingMode::ProviderDefault,
            messages: vec![ChatMessage::user("hi")],
        };
        let body = chat_body(AiProvider::Anthropic, &req);
        assert_eq!(body["max_tokens"], 2048, "the Messages API requires it");
    }

    #[test]
    fn anthropic_body_carries_summarized_thinking_only_when_asked() {
        let mut req = ChatRequest {
            model: "claude-opus-5".into(),
            max_tokens: 16000,
            web_tools: false,
            thinking: ThinkingMode::Summarized,
            messages: vec![ChatMessage::user("hi")],
        };
        let body = chat_body(AiProvider::Anthropic, &req);
        assert_eq!(body["thinking"]["type"], "adaptive");
        assert_eq!(body["thinking"]["display"], "summarized");

        req.thinking = ThinkingMode::ProviderDefault;
        let body = chat_body(AiProvider::Anthropic, &req);
        assert!(body.get("thinking").is_none());
    }

    #[test]
    fn anthropic_body_can_forbid_thinking_outright() {
        let req = ChatRequest {
            model: "claude-opus-5".into(),
            max_tokens: 4096,
            web_tools: false,
            thinking: ThinkingMode::Disabled,
            messages: vec![ChatMessage::user("hi")],
        };
        let body = chat_body(AiProvider::Anthropic, &req);
        assert_eq!(body["thinking"]["type"], "disabled");
        assert_eq!(body["messages"][0]["content"], "hi");
    }

    #[test]
    fn openai_disabled_thinking_appends_the_qwen_soft_switch() {
        let req = ChatRequest {
            model: "qwen3.5:9b".into(),
            max_tokens: 4096,
            web_tools: false,
            thinking: ThinkingMode::Disabled,
            messages: vec![
                ChatMessage::user("first"),
                ChatMessage {
                    role: Role::Assistant,
                    content: "answer".into(),
                    tool_calls: Vec::new(),
                    tool_call_id: None,
                },
                ChatMessage::user("second"),
            ],
        };
        let body = chat_body(AiProvider::OpenAiCompatible, &req);
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages[0]["content"], "first", "only the final user turn");
        assert_eq!(messages[2]["content"], "second\n\n/no_think");
        assert!(body.get("thinking").is_none());
    }

    #[test]
    fn openai_body_never_carries_a_thinking_config() {
        let req = ChatRequest {
            model: "qwen3".into(),
            max_tokens: 16000,
            web_tools: false,
            thinking: ThinkingMode::Summarized,
            messages: vec![ChatMessage::user("hi")],
        };
        let body = chat_body(AiProvider::OpenAiCompatible, &req);
        assert!(
            body.get("thinking").is_none(),
            "the OpenAI-compatible wire has no such field; reasoning models stream regardless"
        );
    }

    #[test]
    fn openai_body_keeps_system_as_leading_message() {
        let req = ChatRequest {
            model: "gpt-x".into(),
            max_tokens: 1024,
            web_tools: false,
            thinking: ThinkingMode::ProviderDefault,
            messages: vec![
                ChatMessage {
                    role: Role::System,
                    content: "be brief".into(),
                    tool_calls: Vec::new(),
                    tool_call_id: None,
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
