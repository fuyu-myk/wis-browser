//! DNS resolution. Production code uses a DNS-over-HTTPS resolver; tests
//! use a [`StubResolver`] that returns fixed mappings without network I/O.

use std::collections::HashMap;
use std::net::IpAddr;

use async_trait::async_trait;
use hickory_resolver::config::ResolverConfig;
use hickory_resolver::name_server::TokioConnectionProvider;
use hickory_resolver::TokioResolver;
use url::Url;

use crate::error::{NetworkError, NetworkResult};

/// Pluggable DNS resolver. Implementations must be safe to share across
/// threads.
#[async_trait]
pub trait Resolver: Send + Sync {
    async fn resolve(&self, hostname: &str) -> NetworkResult<Vec<IpAddr>>;
}

/// DNS-over-HTTPS resolver. Wraps `hickory-resolver`.
///
/// `DohResolver::cloudflare()` and `quad9()` are convenience constructors
/// using built-in `ResolverConfig` presets; `with_config` lets callers point
/// at any DoH endpoint, including the one stored in
/// `UserSettings::doh_resolver`.
pub struct DohResolver {
    resolver: TokioResolver,
}

impl DohResolver {
    /// Use Cloudflare's `1.1.1.1` DoH endpoint.
    pub fn cloudflare() -> Self {
        Self::with_config(ResolverConfig::cloudflare_https())
    }

    /// Use Quad9's `9.9.9.9` DoH endpoint.
    pub fn quad9() -> Self {
        Self::with_config(ResolverConfig::quad9_https())
    }

    /// Build from any `ResolverConfig`. Callers can hand-craft one to point at
    /// e.g. Mozilla's resolver.
    pub fn with_config(config: ResolverConfig) -> Self {
        let resolver =
            TokioResolver::builder_with_config(config, TokioConnectionProvider::default()).build();
        Self { resolver }
    }

    /// Best-effort: parse a DoH URL string and pick a known provider.
    /// Unknown URLs fall back to Cloudflare with a warning.
    pub fn from_settings_url(doh_url: &str) -> NetworkResult<Self> {
        let parsed = Url::parse(doh_url)?;
        let host = parsed
            .host_str()
            .ok_or_else(|| NetworkError::Config(format!("DoH URL has no host: {doh_url}")))?;
        match host {
            "cloudflare-dns.com" | "1.1.1.1" | "1.0.0.1" => Ok(Self::cloudflare()),
            "dns.quad9.net" | "9.9.9.9" => Ok(Self::quad9()),
            "mozilla.cloudflare-dns.com" => Ok(Self::cloudflare()),
            other => {
                tracing::warn!(
                    "DoH host {other} not in built-in provider list; falling back to Cloudflare"
                );
                Ok(Self::cloudflare())
            }
        }
    }
}

#[async_trait]
impl Resolver for DohResolver {
    async fn resolve(&self, hostname: &str) -> NetworkResult<Vec<IpAddr>> {
        let lookup = self
            .resolver
            .lookup_ip(hostname)
            .await
            .map_err(|e| NetworkError::Resolver(e.to_string()))?;
        Ok(lookup.iter().collect())
    }
}

/// In-memory stub resolver for tests. Maps hostnames to IPs without I/O.
#[derive(Default, Clone)]
pub struct StubResolver {
    entries: HashMap<String, Vec<IpAddr>>,
}

impl StubResolver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, hostname: impl Into<String>, ips: Vec<IpAddr>) -> Self {
        self.entries.insert(hostname.into(), ips);
        self
    }
}

#[async_trait]
impl Resolver for StubResolver {
    async fn resolve(&self, hostname: &str) -> NetworkResult<Vec<IpAddr>> {
        self.entries
            .get(hostname)
            .cloned()
            .ok_or_else(|| NetworkError::Resolver(format!("no stub entry for {hostname}")))
    }
}
