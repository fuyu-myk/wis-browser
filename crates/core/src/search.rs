//! The user's default search engine, used to turn non-URL address-bar input
//! into a search. The query-URL templates live on the frontend (where
//! `normaliseUrl` builds the navigable URL); this enum is the persisted choice.

use serde::{Deserialize, Serialize};

/// Default search engine for address-bar queries.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchEngine {
    #[default]
    DuckDuckGo,
    Google,
    Bing,
}
