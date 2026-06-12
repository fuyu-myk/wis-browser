//! Privacy decision engine for the Wis browser.
//!
//! This crate is the brain of the privacy layer: given an outbound
//! request, it decides whether to allow it, upgrade it from HTTP to HTTPS,
//! or block it as a tracker. It also exposes a DNS-over-HTTPS resolver and a
//! per-workspace [`NetworkContext`] that carries the user-agent, referer
//! policy, and resolver to use.
//!
//! The transport that actually enforces these decisions against WebView
//! traffic (a hyper proxy, request interception, etc.) lives outside this
//! crate and will be wired in at the shell layer in future.

pub mod blocker;
pub mod content_filter;
pub mod context;
pub mod error;
pub mod filter_lists;
pub mod policy;
pub mod proxy;
pub mod resolver;
#[cfg(feature = "tor")]
pub mod tor;
pub mod upgrader;

pub use blocker::{CosmeticPage, RequestType, TrackerBlocker};
pub use content_filter::content_blocking_json;
pub use context::{NetworkContext, RefererPolicy};
pub use error::{NetworkError, NetworkResult};
pub use policy::{Decision, OutboundRequest, RequestPolicy};
pub use proxy::{probe_upstream, ProxyServer};
pub use resolver::{DohResolver, Resolver, StubResolver};
#[cfg(feature = "tor")]
pub use tor::TorProxy;
pub use upgrader::HttpsUpgrader;
