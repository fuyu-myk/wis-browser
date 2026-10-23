//! The single entry point that callers use to find out what to do with an
//! outbound request. Composes the tracker blocker, the HTTPS upgrader, and
//! any per-site overrides into one decision.

use std::collections::HashSet;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use url::Url;
use wis_core::{PerSitePolicy, PrivacyMode, UpstreamProxy};

use crate::blocker::{BlockerVerdict, RequestType, TrackerBlocker};
use crate::cloaking::Cloaking;
use crate::error::NetworkResult;
use crate::upgrader::{HttpsUpgrader, UpgradeAction};

/// Everything the policy engine needs to know about a single outbound
/// request. Built by the transport layer just before sending.
#[derive(Debug, Clone)]
pub struct OutboundRequest<'a> {
    pub url: &'a Url,
    pub source_url: &'a Url,
    pub request_type: RequestType,
}

/// The result of running a request through the [`RequestPolicy`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Decision {
    /// Send the request as-is.
    Allow,
    /// Rewrite the request URL to the supplied (upgraded) URL, then send.
    Rewrite { url: Url },
    /// Drop the request. `reason` is the rule text or policy name.
    Block { reason: String },
}

pub struct RequestPolicy {
    privacy_mode: PrivacyMode,
    blocker: Arc<TrackerBlocker>,
    upgrader: HttpsUpgrader,
    per_site: Vec<PerSitePolicy>,
    upstream: Option<UpstreamProxy>,
    global_privacy_control: bool,
    refuse_all: bool,
    cloaking: Option<Arc<Cloaking>>,
    /// Registrable domains whose blocking is off in the site panel.
    unblocked_sites: Arc<HashSet<String>>,
}

impl RequestPolicy {
    pub fn new(privacy_mode: PrivacyMode, blocker: Arc<TrackerBlocker>) -> Self {
        Self {
            upgrader: HttpsUpgrader::new(privacy_mode),
            blocker,
            privacy_mode,
            per_site: Vec::new(),
            upstream: None,
            global_privacy_control: false,
            refuse_all: false,
            cloaking: None,
            unblocked_sites: Arc::default(),
        }
    }

    /// Refuse every request, for a workspace whose route cannot be reached.
    pub fn refusing_all(self) -> Self {
        Self {
            refuse_all: true,
            ..self
        }
    }

    pub fn refuses_all(&self) -> bool {
        self.refuse_all
    }

    /// Send `Sec-GPC: 1` on every request the proxy forwards itself.
    pub fn with_global_privacy_control(self, enabled: bool) -> Self {
        Self {
            global_privacy_control: enabled,
            ..self
        }
    }

    /// Attach per-host overrides. Later entries override earlier ones for
    /// the same host.
    pub fn with_per_site(self, per_site: Vec<PerSitePolicy>) -> Self {
        Self { per_site, ..self }
    }

    /// Route all connections through an upstream proxy (e.g. Tor via SOCKS5).
    pub fn with_upstream(self, upstream: UpstreamProxy) -> Self {
        Self {
            upstream: Some(upstream),
            ..self
        }
    }

    pub fn upstream(&self) -> Option<&UpstreamProxy> {
        self.upstream.as_ref()
    }

    pub fn privacy_mode(&self) -> PrivacyMode {
        self.privacy_mode
    }

    pub fn global_privacy_control(&self) -> bool {
        self.global_privacy_control
    }

    /// Run the request through the full policy. The order is significant:
    /// 1. Apply HTTPS upgrade.
    /// 2. Consult the tracker blocker on the (possibly rewritten) URL.
    /// 3. Honour per-site overrides last so they have the final say.
    pub fn decide(&self, request: &OutboundRequest<'_>) -> NetworkResult<Decision> {
        if self.refuse_all {
            return Ok(Decision::Block {
                reason: "the workspace's route is unavailable".into(),
            });
        }

        // HTTPS upgrade
        let effective_url = match self.upgrader.decide(request.url) {
            UpgradeAction::Keep => request.url.clone(),
            UpgradeAction::UpgradeToHttps(u) => u,
            UpgradeAction::Block => {
                return Ok(Decision::Block {
                    reason: "https-only policy blocked plaintext request".into(),
                });
            }
        };

        // Per-site overrides
        if let Some(host) = effective_url.host_str() {
            if let Some(override_policy) = self.lookup_per_site(host) {
                if !override_policy.allow_third_party
                    && is_third_party(&effective_url, request.source_url)
                {
                    return Ok(Decision::Block {
                        reason: format!("per-site policy for {host} blocks third-party"),
                    });
                }
            }
        }

        // Tracker blocking
        let verdict =
            self.blocker
                .check_request(&effective_url, request.source_url, request.request_type)?;
        match verdict {
            BlockerVerdict::Allow => {
                if effective_url == *request.url {
                    Ok(Decision::Allow)
                } else {
                    Ok(Decision::Rewrite { url: effective_url })
                }
            }
            BlockerVerdict::Block { reason } => Ok(Decision::Block { reason }),
        }
    }

    pub fn with_cloaking(
        mut self,
        cloaking: Arc<Cloaking>,
        unblocked_sites: Arc<HashSet<String>>,
    ) -> Self {
        self.cloaking = Some(cloaking);
        self.unblocked_sites = unblocked_sites;
        self
    }

    pub fn uncloaks(&self) -> bool {
        self.cloaking.is_some()
            && self.privacy_mode != PrivacyMode::Permissive
            && self.upstream.is_none()
    }

    /// The alias that hides a listed tracker behind `host`. Every alias in the
    /// chain counts. Like uBO, an alias on the host's own site, a page being
    /// opened and a host a list excepts are left alone.
    pub fn cloaking_alias<'a>(&self, host: &str, aliases: &'a [String]) -> Option<&'a str> {
        let cloaking = self.cloaking.as_ref()?;
        let site = wis_core::sites::domain::registrable_domain(host);
        let alias = aliases.iter().map(String::as_str).find(|alias| {
            wis_core::sites::domain::registrable_domain(alias) != site
                && self.blocker.blocks_alias(alias, host)
        })?;
        let spared = cloaking.is_page(host)
            || self.unblocked_sites.contains(&site)
            || self.blocker.excepts_tunnel(host);
        (!spared).then_some(alias)
    }

    pub fn record_uncloaked(&self, host: &str) {
        if let Some(cloaking) = &self.cloaking {
            cloaking.record_uncloaked(host);
        }
    }

    fn lookup_per_site(&self, host: &str) -> Option<&PerSitePolicy> {
        self.per_site.iter().rev().find(|p| p.host == host)
    }
}

/// In future, the public-suffix list will be used to compare
/// effective top-level domains (so `mail.example.com` and `www.example.com`
/// are first-party).
fn is_third_party(url: &Url, source_url: &Url) -> bool {
    match (url.host_str(), source_url.host_str()) {
        (Some(a), Some(b)) => a != b,
        _ => false,
    }
}
