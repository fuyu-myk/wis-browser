//! Per-workspace upstream proxy configuration.
//!
//! When set on a [`crate::Workspace`], the workspace's loopback proxy dials
//! every destination through this upstream instead of connecting directly.
//! SOCKS5 is the path for Tor: point it at a locally-running Tor daemon
//! (`127.0.0.1:9050`) and the proxy resolves + routes through Tor.
//! `None` means a direct connection (the default).

use serde::{Deserialize, Serialize};

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
}
