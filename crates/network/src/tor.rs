//! Built-in Tor client (Arti), exposed as a loopback SOCKS5 proxy.
//!
//! Only compiled with the `tor` cargo feature. [`TorProxy::start`] bootstraps an
//! in-process Tor client and runs a tiny SOCKS5 server on `127.0.0.1:<port>`;
//! the shell points a workspace's upstream at that port, so the rest of the
//! transport (see [`crate::proxy`]) treats built-in Tor exactly like any other
//! SOCKS5 upstream. Connections are dialed through Tor via Arti's
//! `TorClient::connect`, which resolves DNS over the Tor network.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use arti_client::{config::TorClientConfigBuilder, TorClient};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::compat::FuturesAsyncReadCompatExt;
use tor_rtcompat::Runtime;

/// A running loopback SOCKS5 proxy backed by an in-process Tor client.
pub struct TorProxy {
    local_addr: SocketAddr,
}

impl TorProxy {
    /// Bootstrap Tor and start the SOCKS5 listener. Tor's directory cache and
    /// persistent state live under `data_dir/{cache,state}`. Awaits a full
    /// bootstrap (can take several seconds on first launch).
    pub async fn start(data_dir: PathBuf) -> Result<Self, String> {
        let state_dir = data_dir.join("state");
        let cache_dir = data_dir.join("cache");
        std::fs::create_dir_all(&state_dir).map_err(|e| format!("tor state dir: {e}"))?;
        std::fs::create_dir_all(&cache_dir).map_err(|e| format!("tor cache dir: {e}"))?;

        let config = TorClientConfigBuilder::from_directories(&state_dir, &cache_dir)
            .build()
            .map_err(|e| format!("tor config: {e}"))?;
        let client = TorClient::create_bootstrapped(config)
            .await
            .map_err(|e| format!("tor bootstrap failed: {e}"))?;
        let client = Arc::new(client);

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("tor socks bind: {e}"))?;
        let local_addr = listener
            .local_addr()
            .map_err(|e| format!("tor socks addr: {e}"))?;

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((sock, _)) => {
                        let client = client.clone();
                        tokio::spawn(async move {
                            if let Err(err) = serve(sock, client).await {
                                tracing::trace!("tor socks conn: {err}");
                            }
                        });
                    }
                    Err(err) => {
                        tracing::debug!("tor socks accept: {err}");
                        break;
                    }
                }
            }
        });

        tracing::info!("built-in Tor SOCKS5 listening at {local_addr}");
        Ok(Self { local_addr })
    }

    pub fn port(&self) -> u16 {
        self.local_addr.port()
    }
}

/// Handle one inbound SOCKS5 connection: negotiate no-auth, read the CONNECT
/// target, dial it through Tor, then splice the two streams.
async fn serve<R: Runtime>(
    mut inbound: TcpStream,
    client: Arc<TorClient<R>>,
) -> Result<(), String> {
    // Greeting: VER, NMETHODS, METHODS...
    let mut head = [0u8; 2];
    inbound.read_exact(&mut head).await.map_err(io)?;
    if head[0] != 0x05 {
        return Err("not a SOCKS5 client".to_string());
    }

    let mut methods = vec![0u8; head[1] as usize];
    inbound.read_exact(&mut methods).await.map_err(io)?;
    inbound.write_all(&[0x05, 0x00]).await.map_err(io)?;

    // Request: VER, CMD, RSV, ATYP, ADDR, PORT.
    let mut req = [0u8; 4];
    inbound.read_exact(&mut req).await.map_err(io)?;
    if req[1] != 0x01 {
        // Only CONNECT is supported.
        let _ = inbound
            .write_all(&[0x05, 0x07, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
            .await;
        return Err(format!("unsupported SOCKS command {}", req[1]));
    }
    let host = match req[3] {
        0x01 => {
            let mut a = [0u8; 4];
            inbound.read_exact(&mut a).await.map_err(io)?;
            std::net::Ipv4Addr::from(a).to_string()
        }
        0x04 => {
            let mut a = [0u8; 16];
            inbound.read_exact(&mut a).await.map_err(io)?;
            std::net::Ipv6Addr::from(a).to_string()
        }
        0x03 => {
            let mut len = [0u8; 1];
            inbound.read_exact(&mut len).await.map_err(io)?;
            let mut domain = vec![0u8; len[0] as usize];
            inbound.read_exact(&mut domain).await.map_err(io)?;
            String::from_utf8_lossy(&domain).into_owned()
        }
        other => return Err(format!("bad SOCKS atyp {other}")),
    };
    let mut port_bytes = [0u8; 2];
    inbound.read_exact(&mut port_bytes).await.map_err(io)?;
    let port = u16::from_be_bytes(port_bytes);

    // Dial through Tor (remote DNS).
    let outbound = match client.connect((host.as_str(), port)).await {
        Ok(stream) => stream,
        Err(err) => {
            let _ = inbound
                .write_all(&[0x05, 0x01, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                .await;
            return Err(format!("tor connect {host}:{port}: {err}"));
        }
    };

    // Reply: success (bound address left zeroed).
    inbound
        .write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
        .await
        .map_err(io)?;

    let mut outbound = outbound.compat();
    tokio::io::copy_bidirectional(&mut inbound, &mut outbound)
        .await
        .map_err(io)?;

    Ok(())
}

fn io(err: std::io::Error) -> String {
    err.to_string()
}
