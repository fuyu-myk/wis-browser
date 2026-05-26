//! Tracker blocking using Brave's `adblock` engine.
//!
//! A [`TrackerBlocker`] is built once from a set of Adblock-Plus-syntax
//! filter lists (the bundled default list, optionally supplemented at
//! runtime).

use adblock::lists::{FilterSet, ParseOptions};
use adblock::request::Request;
use adblock::Engine;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::{NetworkError, NetworkResult};
use crate::filter_lists::DEFAULT_TRACKER_LIST;

/// The kind of subresource being requested. Lifted from the standard
/// Adblock Plus `$type` options.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestType {
    /// Top-level navigation (a tab's main URL).
    Document,
    /// Iframe or other embedded document.
    Subdocument,
    Stylesheet,
    Script,
    Image,
    Font,
    Media,
    Xhr,
    WebSocket,
    Other,
}

impl RequestType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::Subdocument => "subdocument",
            Self::Stylesheet => "stylesheet",
            Self::Script => "script",
            Self::Image => "image",
            Self::Font => "font",
            Self::Media => "media",
            Self::Xhr => "xhr",
            Self::WebSocket => "websocket",
            Self::Other => "other",
        }
    }
}

/// Result of a single blocker decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockerVerdict {
    /// Request did not match any blocking rule.
    Allow,
    /// Request matched a blocking rule. `reason` is the filter rule text.
    Block { reason: String },
}

pub struct TrackerBlocker {
    engine: Engine,
}

impl TrackerBlocker {
    /// Build a blocker preloaded with the bundled default tracker list.
    pub fn with_default_list() -> NetworkResult<Self> {
        Self::from_filter_texts(&[DEFAULT_TRACKER_LIST])
    }

    /// Build from one or more Adblock-Plus-syntax filter list texts.
    pub fn from_filter_texts(texts: &[&str]) -> NetworkResult<Self> {
        let mut set = FilterSet::new(false);
        for text in texts {
            set.add_filter_list(text, ParseOptions::default());
        }

        let engine = Engine::from_filter_set(set, true);
        Ok(Self { engine })
    }

    /// An empty blocker. Used when `PrivacyMode::Permissive` disables
    /// tracker blocking entirely, but the policy code still wants something
    /// to call.
    pub fn empty() -> Self {
        let engine = Engine::from_filter_set(FilterSet::new(false), true);
        Self { engine }
    }

    /// Check a request. `source_url` is the URL of the document making the
    /// request (used to determine first-party vs third-party).
    pub fn check_request(
        &self,
        url: &Url,
        source_url: &Url,
        request_type: RequestType,
    ) -> NetworkResult<BlockerVerdict> {
        let req = Request::new(url.as_str(), source_url.as_str(), request_type.as_str())
            .map_err(|e| NetworkError::FilterList(format!("invalid request: {e:?}")))?;
        let result = self.engine.check_network_request(&req);

        if result.matched {
            Ok(BlockerVerdict::Block {
                reason: result.filter.unwrap_or_else(|| "tracker".into()),
            })
        } else {
            Ok(BlockerVerdict::Allow)
        }
    }
}
