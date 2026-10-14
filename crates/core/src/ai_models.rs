//! Reading a provider's model list: Anthropic's `GET /v1/models` and the
//! OpenAI-style `GET {base}/models` that Ollama, LM Studio and OpenRouter share.

use serde::Serialize;

use crate::ai::AiProvider;

const MAX_MODELS: usize = 1000;
const MAX_ID: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
}

/// The models in a list response, newest first for Anthropic and by id for
/// the rest. Malformed or oversized entries are dropped.
pub fn parse_models(provider: AiProvider, body: &serde_json::Value) -> Vec<ModelInfo> {
    let Some(entries) = body.get("data").and_then(|d| d.as_array()) else {
        return Vec::new();
    };
    let mut seen = std::collections::HashSet::new();
    let mut models: Vec<ModelInfo> = entries
        .iter()
        .filter_map(|entry| {
            let id = entry.get("id")?.as_str()?.trim();
            if id.is_empty() || id.len() > MAX_ID || id.chars().any(char::is_control) {
                return None;
            }

            let name = entry
                .get("display_name")
                .and_then(|n| n.as_str())
                .map(str::trim)
                .filter(|n| !n.is_empty() && n.len() <= MAX_ID)
                .unwrap_or(id);
            Some(ModelInfo {
                id: id.to_string(),
                name: name.to_string(),
            })
        })
        .filter(|m| seen.insert(m.id.clone()))
        .take(MAX_MODELS)
        .collect();
    if provider == AiProvider::OpenAiCompatible {
        models.sort_by(|a, b| a.id.cmp(&b.id));
    }

    models
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn anthropic_lists_keep_their_order_and_names() {
        let body = json!({ "data": [
            { "id": "claude-opus-5-5", "display_name": "Claude Opus 5.5", "type": "model" },
            { "id": "claude-haiku-5-5", "display_name": "Claude Haiku 5.5", "type": "model" }
        ], "has_more": false });
        let models = parse_models(AiProvider::Anthropic, &body);
        assert_eq!(
            models
                .iter()
                .map(|m| (m.id.as_str(), m.name.as_str()))
                .collect::<Vec<_>>(),
            [
                ("claude-opus-5-5", "Claude Opus 5.5"),
                ("claude-haiku-5-5", "Claude Haiku 5.5")
            ]
        );
    }

    #[test]
    fn openai_style_lists_sort_by_id_and_name_by_id() {
        let body = json!({ "object": "list", "data": [
            { "id": "qwen3:8b", "object": "model" },
            { "id": "llama3.2", "object": "model" },
            { "id": "qwen3:8b", "object": "model" }
        ] });
        let models = parse_models(AiProvider::OpenAiCompatible, &body);
        assert_eq!(
            models
                .iter()
                .map(|m| (m.id.as_str(), m.name.as_str()))
                .collect::<Vec<_>>(),
            [("llama3.2", "llama3.2"), ("qwen3:8b", "qwen3:8b")]
        );
    }

    #[test]
    fn junk_is_dropped() {
        let long = "x".repeat(MAX_ID + 1);
        let body = json!({ "data": [
            { "id": "" }, { "id": 5 }, { "name": "no id" }, { "id": "bad\nid" }, { "id": long }, { "id": "ok" }
        ] });
        assert_eq!(parse_models(AiProvider::OpenAiCompatible, &body).len(), 1);
        assert!(parse_models(AiProvider::Anthropic, &json!({ "error": "nope" })).is_empty());
    }
}
