//! Side-widget descriptors (social apps and AI providers).
//!
//! Quick-launch shortcuts pinned to the sidebar: a favicon that opens its site
//! in a split pane. They carry a URL but aren't full tabs in the tab list.

use crate::error::{WisError, WisResult};
use crate::ids::WidgetId;
use serde::{Deserialize, Serialize};

/// Cap on a widget's display label, enforced at the IPC boundary.
pub const MAX_WIDGET_LABEL_CHARS: usize = 48;
/// Cap on a widget's stored URL, enforced at the IPC boundary.
pub const MAX_WIDGET_URL_BYTES: usize = 2048;

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
    // Productivity.
    Gmail,
    GoogleCalendar,
    GoogleKeep,
    Notion,
    Todoist,
    Outlook,
    ProtonMail,
    ProtonCalendar,
    // User-defined.
    Custom,
}

/// Dock row a widget renders under. Curated widgets always render under
/// their kind's own row; a custom widget may be placed in any row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WidgetGroup {
    Social,
    Ai,
    Productivity,
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
    /// Dock-row override for a custom widget; `None` renders under the
    /// kind's own row.
    #[serde(default)]
    pub group: Option<WidgetGroup>,
}

impl Widget {
    /// Boundary validation: every stored widget, curated or custom, must
    /// carry a launchable http(s) URL and a displayable label. Anything
    /// that is not plainly a web address (javascript:, file:, data:,
    /// or a bare word) is refused here.
    pub fn validate(&self) -> WisResult<()> {
        let label = self.label.trim();
        if label.is_empty() || label.chars().count() > MAX_WIDGET_LABEL_CHARS {
            return Err(WisError::InvalidInput {
                message: format!("widget label must be 1 to {MAX_WIDGET_LABEL_CHARS} characters"),
            });
        }

        let web_url_needed = || WisError::InvalidInput {
            message: "widget URL must be a full http(s) URL".to_string(),
        };
        if self.url.len() > MAX_WIDGET_URL_BYTES {
            return Err(web_url_needed());
        }

        let parsed = url::Url::parse(&self.url).map_err(|_| web_url_needed())?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
            return Err(web_url_needed());
        }

        if self.kind != WidgetKind::Custom && self.group.is_some() {
            return Err(WisError::InvalidInput {
                message: "only custom widgets choose their group".to_string(),
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widget(label: &str, url: &str) -> Widget {
        Widget {
            id: WidgetId::new(),
            kind: WidgetKind::Custom,
            label: label.into(),
            url: url.into(),
            icon: None,
            enabled: true,
            group: None,
        }
    }

    #[test]
    fn a_widget_with_a_web_url_is_valid() {
        for url in [
            "https://notion.so",
            "http://example.com/path?q=1",
            "https://192.168.1.5:8080/dash",
        ] {
            assert!(widget("Notes", url).validate().is_ok(), "{url}");
        }
    }

    #[test]
    fn non_web_and_malformed_urls_are_refused() {
        for url in [
            "",
            "notion.so",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "ftp://example.com/",
            "data:text/html,hi",
            "https://",
        ] {
            let err = widget("Notes", url).validate();
            assert!(
                matches!(err, Err(WisError::InvalidInput { .. })),
                "{url:?} must be refused"
            );
        }
        let long = format!("https://example.com/{}", "a".repeat(MAX_WIDGET_URL_BYTES));
        assert!(widget("Notes", &long).validate().is_err());
    }

    #[test]
    fn only_customs_may_choose_their_group() {
        let mut curated = widget("Chat", "https://example.com");
        curated.kind = WidgetKind::Slack;
        curated.group = Some(WidgetGroup::Productivity);
        assert!(curated.validate().is_err());

        let mut custom = widget("Wiki", "https://example.com");
        custom.group = Some(WidgetGroup::Social);
        assert!(custom.validate().is_ok());
    }

    #[test]
    fn labels_are_bounded_and_non_blank() {
        assert!(widget("", "https://example.com").validate().is_err());
        assert!(widget("   ", "https://example.com").validate().is_err());
        let long = "x".repeat(MAX_WIDGET_LABEL_CHARS + 1);
        assert!(widget(&long, "https://example.com").validate().is_err());
        let max = "x".repeat(MAX_WIDGET_LABEL_CHARS);
        assert!(widget(&max, "https://example.com").validate().is_ok());
    }
}
