//! Side-widget descriptors (WhatsApp, Telegram, Discord, etc).
//!
//! These are pinned to the sidebar and have a URL, but they aren't full tabs.

use crate::ids::WidgetId;
use serde::{Deserialize, Serialize};

/// Built-in widget identities the UI knows about. `Custom` covers anything
/// configured by the user that isn't on the curated list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WidgetKind {
    WhatsApp,
    Telegram,
    Discord,
    Slack,
    Messenger,
    XTwitter,
    Instagram,
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
