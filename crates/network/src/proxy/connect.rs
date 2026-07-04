//! Establishing the outbound TCP connection to a destination, either directly
//! (DoH-resolve + connect) or through a configured upstream proxy.
//!
//! For SOCKS5 upstreams, the proxy is asked to reach `host:port` by name,
//! so the upstream performs DNS resolution, which is what keeps Tor from
//! leaking lookups.

use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;
use wis_core::UpstreamProxy;

use crate::resolver::Resolver;

/// How long a reachability probe waits before giving up.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Backstop for DNS resolution.
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(12);

/// Per-address connect timeout.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);

/// Check that a SOCKS5 upstream is reachable and speaks the protocol, without
/// opening any onward connection.
pub async fn probe_upstream(upstream: &UpstreamProxy) -> Result<(), String> {
    let (host, port) = match upstream {
        UpstreamProxy::Socks5 { host, port } => (host.clone(), *port),
        UpstreamProxy::Tor => {
            return Err("built-in Tor is probed via the client, not a port".to_string())
        }
    };

    let mut s = match timeout(PROBE_TIMEOUT, TcpStream::connect((host.as_str(), port))).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return Err(format!("can't reach proxy at {host}:{port}: {e}")),
        Err(_) => return Err(format!("timed out reaching {host}:{port}")),
    };

    // Confirm that the no-auth method negotiation is really a SOCKS5
    // endpoint.
    s.write_all(&[0x05, 0x01, 0x00])
        .await
        .map_err(|e| format!("socks5 handshake write: {e}"))?;
    let mut method = [0u8; 2];
    match timeout(PROBE_TIMEOUT, s.read_exact(&mut method)).await {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => return Err(format!("socks5 handshake read: {e}")),
        Err(_) => return Err("socks5 handshake timed out".to_string()),
    }
    if method[0] != 0x05 {
        return Err(format!("{host}:{port} is not a SOCKS5 proxy"));
    }

    Ok(())
}

/// Open a TCP stream to `host:port`, routing through `upstream` when set.
pub async fn connect_target(
    host: &str,
    port: u16,
    upstream: Option<&UpstreamProxy>,
    resolver: &Arc<dyn Resolver>,
) -> Result<TcpStream, String> {
    match upstream {
        None => direct(host, port, resolver).await,
        Some(UpstreamProxy::Socks5 {
            host: proxy_host,
            port: proxy_port,
        }) => socks5_connect(proxy_host, *proxy_port, host, port).await,
        // The shell resolves Tor to a concrete loopback SOCKS5 address before
        // building the policy, so it should never reach the transport layer.
        Some(UpstreamProxy::Tor) => {
            Err("built-in Tor was not resolved to a SOCKS5 address".to_string())
        }
    }
}

/// Direct connection: DoH-resolve the host, then dial each address in turn
/// until one connects.
async fn direct(host: &str, port: u16, resolver: &Arc<dyn Resolver>) -> Result<TcpStream, String> {
    let addrs = match timeout(RESOLVE_TIMEOUT, resolver.resolve(host)).await {
        Ok(Ok(addrs)) => addrs,
        Ok(Err(e)) => return Err(format!("dns {host}: {e}")),
        Err(_) => {
            return Err(format!(
                "dns {host}: timed out after {}s",
                RESOLVE_TIMEOUT.as_secs()
            ))
        }
    };

    if addrs.is_empty() {
        return Err(format!("no addresses for {host}"));
    }

    let mut last_err = format!("no addresses reachable for {host}");
    for ip in addrs {
        match timeout(CONNECT_TIMEOUT, TcpStream::connect((ip, port))).await {
            Ok(Ok(stream)) => return Ok(stream),
            Ok(Err(e)) => last_err = format!("connect {ip}:{port}: {e}"),
            Err(_) => {
                last_err = format!(
                    "connect {ip}:{port}: timed out after {}s",
                    CONNECT_TIMEOUT.as_secs()
                )
            }
        }
    }

    Err(last_err)
}

/// SOCKS5 no-auth CONNECT, asking the proxy to reach `host:port` by name.
async fn socks5_connect(
    proxy_host: &str,
    proxy_port: u16,
    host: &str,
    port: u16,
) -> Result<TcpStream, String> {
    let host_bytes = host.as_bytes();
    if host_bytes.len() > 255 {
        return Err(format!("socks5: host {host} too long"));
    }

    let mut s = TcpStream::connect((proxy_host, proxy_port))
        .await
        .map_err(|e| format!("socks5 connect {proxy_host}:{proxy_port}: {e}"))?;

    // Greeting: VER=5, NMETHODS=1, METHODS=[0x00 no-auth].
    s.write_all(&[0x05, 0x01, 0x00])
        .await
        .map_err(|e| format!("socks5 write greeting: {e}"))?;
    let mut method = [0u8; 2];
    s.read_exact(&mut method)
        .await
        .map_err(|e| format!("socks5 read method: {e}"))?;
    if method[0] != 0x05 || method[1] != 0x00 {
        return Err(format!(
            "socks5: proxy rejected no-auth (ver={}, method={})",
            method[0], method[1]
        ));
    }

    // CONNECT: VER=5, CMD=1, RSV=0, ATYP=3 (domain), LEN, domain, port (BE).
    let mut req = Vec::with_capacity(7 + host_bytes.len());
    req.extend_from_slice(&[0x05, 0x01, 0x00, 0x03, host_bytes.len() as u8]);
    req.extend_from_slice(host_bytes);
    req.extend_from_slice(&port.to_be_bytes());
    s.write_all(&req)
        .await
        .map_err(|e| format!("socks5 write connect: {e}"))?;

    // Reply: VER, REP, RSV, ATYP, BND.ADDR, BND.PORT.
    let mut head = [0u8; 4];
    s.read_exact(&mut head)
        .await
        .map_err(|e| format!("socks5 read reply: {e}"))?;
    if head[1] != 0x00 {
        return Err(format!("socks5 connect failed (rep={})", head[1]));
    }
    let addr_len = match head[3] {
        0x01 => 4,
        0x04 => 16,
        0x03 => {
            let mut len = [0u8; 1];
            s.read_exact(&mut len)
                .await
                .map_err(|e| format!("socks5 read domain len: {e}"))?;
            len[0] as usize
        }
        other => return Err(format!("socks5: unexpected bound atyp {other}")),
    };
    let mut rest = vec![0u8; addr_len + 2]; // bound address + port
    s.read_exact(&mut rest)
        .await
        .map_err(|e| format!("socks5 read bound addr: {e}"))?;

    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolver::StubResolver;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn stub() -> Arc<dyn Resolver> {
        Arc::new(StubResolver::new())
    }

    /// Mock SOCKS5 proxy: completes a no-auth CONNECT for a domain target, then
    /// echoes back the requested hostname (proving remote DNS / atyp=domain).
    #[tokio::test]
    async fn socks5_connect_uses_remote_domain() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut greet = [0u8; 3];
            s.read_exact(&mut greet).await.unwrap();
            assert_eq!(greet, [0x05, 0x01, 0x00], "no-auth greeting");
            s.write_all(&[0x05, 0x00]).await.unwrap();

            let mut head = [0u8; 5];
            s.read_exact(&mut head).await.unwrap();
            assert_eq!(&head[..4], &[0x05, 0x01, 0x00, 0x03], "CONNECT domain");
            let len = head[4] as usize;
            let mut rest = vec![0u8; len + 2];
            s.read_exact(&mut rest).await.unwrap();
            let host = String::from_utf8_lossy(&rest[..len]).to_string();
            assert_eq!(host, "example.com");

            // Success reply with a zero IPv4 bound address.
            s.write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                .await
                .unwrap();
            // Then act as the target.
            let mut ping = [0u8; 4];
            s.read_exact(&mut ping).await.unwrap();
            s.write_all(host.as_bytes()).await.unwrap();
        });

        let upstream = UpstreamProxy::Socks5 {
            host: addr.ip().to_string(),
            port: addr.port(),
        };
        let mut stream = connect_target("example.com", 443, Some(&upstream), &stub())
            .await
            .expect("socks5 connect");
        stream.write_all(b"ping").await.unwrap();
        let mut echoed = vec![0u8; "example.com".len()];
        stream.read_exact(&mut echoed).await.unwrap();
        assert_eq!(&echoed, b"example.com");
        server.await.unwrap();
    }
}
