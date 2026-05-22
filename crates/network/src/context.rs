//! Per-workspace networking configuration. Holds the resolver, user-agent,
//! and referer policy in a single value so the transport layer can
//! switch between workspaces by swapping in a different [`NetworkContext`].

use std::sync::Arc;

use crate::resolver::Resolver;

const DEFAULT_USER_AGENT: &str = concat!(
    "Mozilla/5.0 (Wis/",
    env!("CARGO_PKG_VERSION"),
    "; KHTML, like Gecko) Wis Browser"
);

/// What to send in the `Referer` header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RefererPolicy {
    /// Never send a referer.
    NoReferer,
    /// Send a referer for same-origin requests only.
    #[default]
    SameOrigin,
    /// Send the origin (scheme + host + port) but not the path/query.
    StrictOrigin,
    /// Send the full URL on same-origin, only the origin cross-origin.
    /// Most browsers' default.
    StrictOriginWhenCrossOrigin,
}

#[derive(Clone)]
pub struct NetworkContext {
    pub user_agent: String,
    pub referer_policy: RefererPolicy,
    pub resolver: Arc<dyn Resolver>,
}

impl NetworkContext {
    pub fn new(resolver: Arc<dyn Resolver>) -> Self {
        Self {
            user_agent: DEFAULT_USER_AGENT.to_string(),
            referer_policy: RefererPolicy::default(),
            resolver,
        }
    }

    pub fn with_user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = ua.into();
        self
    }

    pub fn with_referer_policy(mut self, policy: RefererPolicy) -> Self {
        self.referer_policy = policy;
        self
    }
}
