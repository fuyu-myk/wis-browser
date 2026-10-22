//! The dial's resolver on a direct route: one lookup gives the addresses and
//! the CNAME chain, and a host hiding a listed tracker fails like a lookup,
//! inside the dial's own redialling on network changes.

use std::net::IpAddr;
use std::sync::Arc;

use async_trait::async_trait;

use crate::error::{NetworkError, NetworkResult};
use crate::{RequestPolicy, Resolver};

pub(super) struct Uncloaking {
    inner: Arc<dyn Resolver>,
    policy: Arc<RequestPolicy>,
}

impl Uncloaking {
    pub(super) fn new(inner: Arc<dyn Resolver>, policy: Arc<RequestPolicy>) -> Self {
        Self { inner, policy }
    }
}

#[async_trait]
impl Resolver for Uncloaking {
    async fn resolve(&self, host: &str) -> NetworkResult<Vec<IpAddr>> {
        let resolved = self.inner.resolve_with_aliases(host).await?;
        let host = host.to_ascii_lowercase();
        let host = host.as_str();
        if let Some(alias) = self.policy.cloaking_alias(host, &resolved.aliases) {
            tracing::info!(target: "wis_network::proxy", host, alias, "uncloaked CONNECT");
            self.policy.record_uncloaked(host);
            return Err(NetworkError::Resolver(format!(
                "blocked: {host} is an alias of {alias}"
            )));
        }
        Ok(resolved.ips)
    }
}
