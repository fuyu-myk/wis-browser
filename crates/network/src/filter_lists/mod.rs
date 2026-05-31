//! Tracker filter lists.
//!
//! - [`DEFAULT_TRACKER_LIST`] is the bundled offline fallback (hand-curated,
//!   ships in the binary).
//! - [`fetch::refresh_easyprivacy`] downloads EasyPrivacy into a per-user
//!   cache directory; [`load_cached_or_default`] picks the fresher source
//!   without touching the network.

use std::path::Path;

pub mod bundled;
pub mod fetch;

pub use bundled::DEFAULT_TRACKER_LIST;

/// Which filter list source produced the active rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterSource {
    Bundled,
    Cached,
}

/// Read the cached EasyPrivacy list if present and fresh; otherwise return
/// the bundled fallback. Stale or unreadable caches downgrade silently to the
/// fallback.
pub fn load_cached_or_default(cache_dir: &Path) -> (String, FilterSource) {
    let path = fetch::easyprivacy_path(cache_dir);
    if fetch::cache_is_stale(cache_dir) {
        tracing::info!("filter_lists: cache stale or missing, using bundled fallback");
        return (DEFAULT_TRACKER_LIST.to_string(), FilterSource::Bundled);
    }
    
    match std::fs::read_to_string(&path) {
        Ok(body) => {
            tracing::info!(
                "filter_lists: loaded cached easyprivacy ({} bytes)",
                body.len()
            );
            (body, FilterSource::Cached)
        }
        Err(err) => {
            tracing::warn!("filter_lists: cache read failed ({err}); using bundled fallback");
            (DEFAULT_TRACKER_LIST.to_string(), FilterSource::Bundled)
        }
    }
}
