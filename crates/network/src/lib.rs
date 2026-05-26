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
pub mod context;
pub mod error;
pub mod filter_lists;
pub mod policy;
pub mod proxy;
pub mod resolver;
pub mod upgrader;

pub use blocker::{RequestType, TrackerBlocker};
pub use context::{NetworkContext, RefererPolicy};
pub use error::{NetworkError, NetworkResult};
pub use policy::{Decision, OutboundRequest, RequestPolicy};
pub use proxy::{ca_dir, CertAuthority, CertIssuer, MitmConfig, ProxyServer};
pub use resolver::{DohResolver, Resolver, StubResolver};
pub use upgrader::HttpsUpgrader;
