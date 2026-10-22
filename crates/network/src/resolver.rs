//! DNS resolution. Production code uses a DNS-over-HTTPS resolver; tests
//! use a [`StubResolver`] that returns fixed mappings without network I/O.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;

use async_trait::async_trait;
use hickory_resolver::config::ResolverConfig;
use hickory_resolver::name_server::TokioConnectionProvider;
use hickory_resolver::TokioResolver;
use url::Url;

use crate::error::{NetworkError, NetworkResult};

/// A DoH endpoint this build can actually resolve through. `hickory` ships a
/// vetted config per provider (endpoint plus the bootstrap addresses needed to
/// reach it without DNS), which is why the choice is an enum rather than a
/// free-form URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DohProvider {
    Cloudflare,
    Quad9,
    Google,
}

impl DohProvider {
    /// The endpoint stored in `UserSettings::doh_resolver` for this provider.
    /// The picker in the chrome offers exactly these strings.
    pub fn url(self) -> &'static str {
        match self {
            Self::Cloudflare => "https://cloudflare-dns.com/dns-query",
            Self::Quad9 => "https://dns.quad9.net/dns-query",
            Self::Google => "https://dns.google/dns-query",
        }
    }

    fn config(self) -> ResolverConfig {
        match self {
            Self::Cloudflare => ResolverConfig::cloudflare_https(),
            Self::Quad9 => ResolverConfig::quad9_https(),
            Self::Google => ResolverConfig::google_https(),
        }
    }
}

/// The provider a stored DoH URL resolves through. Unrecognised endpoints fall
/// back to Cloudflare.
pub fn doh_provider_for(doh_url: &str) -> DohProvider {
    let host = Url::parse(doh_url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase));
    match host.as_deref() {
        Some("dns.quad9.net" | "9.9.9.9" | "149.112.112.112") => DohProvider::Quad9,
        Some("dns.google" | "8.8.8.8" | "8.8.4.4") => DohProvider::Google,
        _ => DohProvider::Cloudflare,
    }
}

/// Pluggable DNS resolver. Implementations must be safe to share across
/// threads.
#[async_trait]
pub trait Resolver: Send + Sync {
    async fn resolve(&self, hostname: &str) -> NetworkResult<Vec<IpAddr>>;

    /// The addresses with the CNAME chain they were reached through, empty
    /// where the resolver cannot see it.
    async fn resolve_with_aliases(&self, hostname: &str) -> NetworkResult<Resolved> {
        Ok(Resolved {
            ips: self.resolve(hostname).await?,
            aliases: Vec::new(),
        })
    }
}

pub struct Resolved {
    pub ips: Vec<IpAddr>,
    pub aliases: Vec<String>,
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

    /// The resolver for a provider. Callers name a provider rather than a
    /// URL: only these endpoints ship the bootstrap addresses needed to reach
    /// them without DNS.
    pub fn for_provider(provider: DohProvider) -> Self {
        Self::with_config(provider.config())
    }
}

/// A resolver whose backing implementation can be replaced while requests are
/// in flight. Proxies take a handle to this once, so changing the DoH provider
/// in settings takes effect on the next lookup.
pub struct SwappableResolver {
    inner: std::sync::RwLock<Arc<dyn Resolver>>,
}

impl SwappableResolver {
    pub fn new(initial: Arc<dyn Resolver>) -> Self {
        Self {
            inner: std::sync::RwLock::new(initial),
        }
    }

    pub fn set(&self, next: Arc<dyn Resolver>) {
        let mut slot = self
            .inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *slot = next;
    }

    fn current(&self) -> Arc<dyn Resolver> {
        self.inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

#[async_trait]
impl Resolver for SwappableResolver {
    async fn resolve(&self, hostname: &str) -> NetworkResult<Vec<IpAddr>> {
        let current = self.current();
        current.resolve(hostname).await
    }

    async fn resolve_with_aliases(&self, hostname: &str) -> NetworkResult<Resolved> {
        let current = self.current();
        current.resolve_with_aliases(hostname).await
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

    async fn resolve_with_aliases(&self, hostname: &str) -> NetworkResult<Resolved> {
        let lookup = self
            .resolver
            .lookup_ip(hostname)
            .await
            .map_err(|e| NetworkError::Resolver(e.to_string()))?;
        Ok(Resolved {
            ips: lookup.iter().collect(),
            aliases: cname_targets(lookup.as_lookup()),
        })
    }
}

fn cname_targets(lookup: &hickory_resolver::lookup::Lookup) -> Vec<String> {
    lookup
        .record_iter()
        .filter_map(|record| match record.data() {
            hickory_resolver::proto::rr::RData::CNAME(target) => Some(
                target
                    .0
                    .to_utf8()
                    .trim_end_matches('.')
                    .to_ascii_lowercase(),
            ),
            _ => None,
        })
        .collect()
}

/// In-memory stub resolver for tests. Maps hostnames to IPs without I/O.
#[derive(Default, Clone)]
pub struct StubResolver {
    entries: HashMap<String, Vec<IpAddr>>,
    aliases: HashMap<String, Vec<String>>,
}

impl StubResolver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, hostname: impl Into<String>, ips: Vec<IpAddr>) -> Self {
        self.entries.insert(hostname.into(), ips);
        self
    }

    pub fn with_aliases(mut self, hostname: impl Into<String>, aliases: &[&str]) -> Self {
        self.aliases.insert(
            hostname.into(),
            aliases.iter().map(|alias| alias.to_string()).collect(),
        );
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

    async fn resolve_with_aliases(&self, hostname: &str) -> NetworkResult<Resolved> {
        Ok(Resolved {
            ips: self.resolve(hostname).await?,
            aliases: self.aliases.get(hostname).cloned().unwrap_or_default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;
    use std::str::FromStr;

    use hickory_resolver::lookup::Lookup;
    use hickory_resolver::proto::op::Query;
    use hickory_resolver::proto::rr::rdata::{A, CNAME};
    use hickory_resolver::proto::rr::{Name, RData, Record, RecordType};

    use super::*;

    #[test]
    fn a_lookup_s_chain_reads_as_plain_lowercase_hosts() {
        let name = |host: &str| Name::from_str(host).unwrap();
        let records: Arc<[Record]> = Arc::from(vec![
            Record::from_rdata(
                name("a.site.test."),
                60,
                RData::CNAME(CNAME(name("A-Site.Tracker.test."))),
            ),
            Record::from_rdata(
                name("a-site.tracker.test."),
                60,
                RData::A(A(Ipv4Addr::LOCALHOST)),
            ),
        ]);
        let lookup =
            Lookup::new_with_max_ttl(Query::query(name("a.site.test."), RecordType::A), records);

        assert_eq!(cname_targets(&lookup), ["a-site.tracker.test"]);
    }
}
