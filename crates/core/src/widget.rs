//! Side-widget descriptors (social apps and AI providers).
//!
//! Quick-launch shortcuts pinned to the sidebar: a favicon that opens its site
//! in a split pane. They carry a URL but aren't full tabs in the tab list.

use crate::ids::WidgetId;
use serde::{Deserialize, Serialize};

/// Built-in widget identities the UI knows about. `Custom` covers anything
/// configured by the user that isn't on the curated list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WidgetKind {
    // Social / messaging.
    WhatsApp,
    Telegram,
    Discord,
    Slack,
    Messenger,
    XTwitter,
    Instagram,
    Reddit,
    Line,
    // AI providers.
    ChatGpt,
    Claude,
    Gemini,
    Grok,
    DeepSeek,
    Qwen,
    Perplexity,
    // User-defined.
    Custom,
}

/// A pinned side widget. URL is stored as a string here (parsed on use) so the
/// type stays cheaply clonable and JSON-roundtrips identically.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Widget {
    pub id: WidgetId,
    pub kind: WidgetKind,
    pub label: String,
    pub url: String,
    pub icon: Option<String>,
    pub enabled: bool,
}
