//! Per-workspace upstream proxy configuration.
//!
//! When set on a [`crate::Workspace`], the workspace's loopback proxy dials
//! every destination through this upstream instead of connecting directly.
//! SOCKS5 is the path for Tor: point it at a locally-running Tor daemon
//! (`127.0.0.1:9050`) and the proxy resolves + routes through Tor.
//! `None` means a direct connection (the default).

use serde::{Deserialize, Serialize};

use crate::error::{WisError, WisResult};

/// Longest hostname a DNS name can carry, and the cap on a typed proxy host.
pub const MAX_PROXY_HOST_BYTES: usize = 253;

/// An upstream proxy to route a workspace's traffic through.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UpstreamProxy {
    /// SOCKS5 proxy. The proxy performs DNS resolution. This is also the
    /// transport built-in Tor resolves to (a loopback Arti SOCKS5).
    Socks5 { host: String, port: u16 },
    /// The browser's built-in Tor client (Arti). The shell resolves this to a
    /// loopback SOCKS5 address at connect time; only available when the `tor`
    /// cargo feature is compiled in.
    Tor,
}

impl UpstreamProxy {
    /// The conventional local Tor SOCKS5 endpoint (`127.0.0.1:9050`).
    pub fn tor_default() -> Self {
        UpstreamProxy::Socks5 {
            host: "127.0.0.1".to_string(),
            port: 9050,
        }
    }

    /// The same upstream with a typed host tidied up. Paired with
    /// [`Self::validate`]: normalize first, then judge what will be stored.
    pub fn normalized(self) -> Self {
        match self {
            Self::Socks5 { host, port } => {
                let host = host.trim();
                let host = host
                    .strip_prefix('[')
                    .and_then(|inner| inner.strip_suffix(']'))
                    .filter(|inner| inner.parse::<std::net::Ipv6Addr>().is_ok())
                    .unwrap_or(host);
                Self::Socks5 {
                    host: host.to_string(),
                    port,
                }
            }
            Self::Tor => Self::Tor,
        }
    }

    /// Boundary validation for a user-entered upstream. Every request in the
    /// workspace is dialled through this, so it must be a bare host and port.
    pub fn validate(&self) -> WisResult<()> {
        let (host, port) = match self {
            Self::Tor => return Ok(()),
            Self::Socks5 { host, port } => (host.as_str(), *port),
        };
        let invalid = |message: &str| WisError::InvalidInput {
            message: message.to_string(),
        };

        if host.is_empty() || host.len() > MAX_PROXY_HOST_BYTES {
            return Err(invalid(&format!(
                "proxy host must be 1 to {MAX_PROXY_HOST_BYTES} characters"
            )));
        }
        if port == 0 {
            return Err(invalid("proxy port must be between 1 and 65535"));
        }
        if host.contains("://")
            || host.contains('@')
            || host.contains('/')
            || host.chars().any(char::is_whitespace)
        {
            return Err(invalid(
                "enter a host and port only, with no scheme, credentials or path",
            ));
        }
        // A user types an IPv6 literal bare; `url::Host` only takes it bracketed.
        if host.parse::<std::net::IpAddr>().is_ok() {
            return Ok(());
        }
        if host.contains('[') || host.contains(']') {
            return Err(invalid("enter an IPv6 address without brackets"));
        }

        url::Host::parse(host)
            .map(|_| ())
            .map_err(|_| invalid("proxy host is not a valid hostname or IP address"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn socks5(host: &str, port: u16) -> UpstreamProxy {
        UpstreamProxy::Socks5 {
            host: host.into(),
            port,
        }
    }

    #[test]
    fn hosts_and_ports_a_proxy_can_be_dialled_at_are_accepted() {
        for (host, port) in [
            ("127.0.0.1", 9050u16),
            ("localhost", 1080),
            ("proxy.example.com", 3128),
            ("::1", 9050),
            ("10.0.0.7", 65535),
        ] {
            assert!(socks5(host, port).validate().is_ok(), "{host}:{port}");
        }
        assert!(UpstreamProxy::Tor.validate().is_ok());
    }

    #[test]
    fn a_url_shaped_or_empty_host_is_refused() {
        let cases = [
            socks5("", 9050),
            socks5("socks5://127.0.0.1", 9050),
            socks5("user:pass@127.0.0.1", 9050),
            socks5("127.0.0.1/path", 9050),
            socks5("127.0.0.1 ", 9050),
            socks5("bad host", 9050),
            socks5("127.0.0.1", 0),
            socks5(&"a".repeat(MAX_PROXY_HOST_BYTES + 1), 9050),
        ];
        for case in cases {
            assert!(
                matches!(case.validate(), Err(WisError::InvalidInput { .. })),
                "{case:?} must be refused"
            );
        }
    }

    #[test]
    fn normalizing_trims_a_pasted_host_so_it_can_then_validate() {
        let tidied = socks5("  127.0.0.1  ", 9050).normalized();
        assert_eq!(tidied, socks5("127.0.0.1", 9050));
        assert!(tidied.validate().is_ok());
    }

    /// A URL writes an IPv6 literal bracketed, which is the form that gets
    /// pasted, but no dial site resolves it that way.
    #[test]
    fn a_bracketed_ipv6_host_is_unwrapped_and_otherwise_refused() {
        let tidied = socks5("[::1]", 9050).normalized();
        assert_eq!(tidied, socks5("::1", 9050));
        assert!(tidied.validate().is_ok());

        assert!(socks5("[::1]", 9050).validate().is_err());
        assert_eq!(socks5("[nope]", 9050).normalized(), socks5("[nope]", 9050));
        assert!(socks5("[nope]", 9050).validate().is_err());
    }

    #[test]
    fn the_length_and_port_bounds_are_inclusive() {
        assert!(socks5(&"a".repeat(MAX_PROXY_HOST_BYTES), 9050)
            .validate()
            .is_ok());
        assert!(socks5("127.0.0.1", 1).validate().is_ok());
        assert!(socks5("127.0.0.1", 65535).validate().is_ok());
        assert!(socks5("   ", 9050).normalized().validate().is_err());
    }
}
