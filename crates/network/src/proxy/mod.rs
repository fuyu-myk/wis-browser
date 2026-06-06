//! Loopback HTTP proxy that consults [`crate::RequestPolicy`] for every
//! request the WebView issues.
//!
//! See the crate root for the bigger picture. The proxy is HTTP-only:
//! WebViews connect via plain HTTP on `127.0.0.1:PORT`, then either
//! forward (HTTP), tunnel (HTTPS `CONNECT`), 301-redirect (HTTPS-First),
//! or refuse (tracker blocking).

mod connect;
mod handler;
mod request_type;
mod tunnel;
mod upstream;

pub use connect::probe_upstream;

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::sync::watch;

use crate::error::{NetworkError, NetworkResult};
use crate::policy::RequestPolicy;
use crate::resolver::Resolver;

/// A running per-workspace proxy server. Dropping the value sends a
/// shutdown signal to the accept loop and the spawned connection
/// handlers: open connections finish on their own.
pub struct ProxyServer {
    local_addr: SocketAddr,
    shutdown_tx: watch::Sender<bool>,
    /// Hot-swappable request policy.
    policy_tx: watch::Sender<Arc<RequestPolicy>>,
}

impl ProxyServer {
    /// Bind and start the proxy. Returns once the listener is bound;
    /// the accept loop runs in a detached `tokio::task`.
    ///
    /// `bind_addr` is typically `127.0.0.1:0`: pass port `0` and read
    /// the assigned port back via [`port`](Self::port).
    ///
    /// HTTPS CONNECTs are tunnelled raw, end-to-end: the proxy never
    /// terminates TLS.
    pub async fn start(
        policy: Arc<RequestPolicy>,
        resolver: Arc<dyn Resolver>,
        bind_addr: SocketAddr,
    ) -> NetworkResult<Self> {
        let listener = TcpListener::bind(bind_addr).await?;
        let local_addr = listener.local_addr()?;
        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
        let (policy_tx, policy_rx) = watch::channel(policy);

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    changed = shutdown_rx.changed() => {
                        match changed {
                            Ok(()) if *shutdown_rx.borrow() => break,
                            Ok(()) => continue,
                            Err(_) => break,
                        }
                    }
                    accepted = listener.accept() => {
                        let (stream, _peer) = match accepted {
                            Ok(pair) => pair,
                            Err(err) => {
                                tracing::debug!("proxy accept: {err}");
                                continue;
                            }
                        };
                        let policy_rx = policy_rx.clone();
                        let resolver = resolver.clone();
                        tokio::spawn(serve_connection(stream, policy_rx, resolver));
                    }
                }
            }
            tracing::debug!("proxy shutting down at {local_addr}");
        });

        tracing::info!("proxy listening at {local_addr}");
        Ok(Self {
            local_addr,
            shutdown_tx,
            policy_tx,
        })
    }

    /// Swap in a new request policy; takes effect on the next request.
    pub fn update_policy(&self, policy: Arc<RequestPolicy>) {
        let _ = self.policy_tx.send(policy);
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn port(&self) -> u16 {
        self.local_addr.port()
    }
}

impl Drop for ProxyServer {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(true);
    }
}

async fn serve_connection(
    stream: tokio::net::TcpStream,
    policy_rx: watch::Receiver<Arc<RequestPolicy>>,
    resolver: Arc<dyn Resolver>,
) {
    let io = TokioIo::new(stream);
    let service = service_fn(move |req: Request<Incoming>| {
        let policy = policy_rx.borrow().clone();
        let resolver = resolver.clone();
        async move {
            let resp: Response<_> = handler::handle(req, policy, resolver).await;
            Ok::<_, Infallible>(resp)
        }
    });

    if let Err(err) = hyper::server::conn::http1::Builder::new()
        .preserve_header_case(true)
        .title_case_headers(true)
        .serve_connection(io, service)
        .with_upgrades()
        .await
    {
        tracing::debug!("proxy connection: {err}");
    }
}

// Bridge std::io::Error → NetworkError ergonomically.
impl From<NetworkError> for std::io::Error {
    fn from(err: NetworkError) -> Self {
        std::io::Error::other(err.to_string())
    }
}
