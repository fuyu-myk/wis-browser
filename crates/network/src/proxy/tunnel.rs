//! `CONNECT host:port` handling: raw TCP tunnel.
//!
//! Resolution is done via DoH, opening an upstream socket, returning
//! `200 Connection Established`, and `tokio::io::copy_bidirectional` until
//! either side closes. The WebView talks TLS directly to the upstream,
//! end-to-end: the proxy never terminates TLS.

use std::sync::Arc;

use http_body_util::combinators::BoxBody;
use hyper::body::{Bytes, Incoming};
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use wis_core::UpstreamProxy;

use crate::resolver::Resolver;

use super::connect::connect_target;
use super::upstream::{bad_gateway, empty};

pub async fn tunnel(
    req: Request<Incoming>,
    resolver: Arc<dyn Resolver>,
    upstream: Option<UpstreamProxy>,
    mut route_rx: tokio::sync::watch::Receiver<u64>,
) -> Response<BoxBody<Bytes, hyper::Error>> {
    let (host, port) = match parse_authority(&req) {
        Some(pair) => pair,
        None => return bad_gateway("CONNECT target missing host:port"),
    };

    // Open the upstream eagerly (directly or via the proxy) so a meaningful
    // 502 can still be returned before sending 200.
    let upstream = match connect_target(&host, port, upstream.as_ref(), &resolver).await {
        Ok(s) => s,
        Err(err) => return bad_gateway(&err),
    };

    tokio::spawn(async move {
        match hyper::upgrade::on(req).await {
            Ok(upgraded) => {
                let mut upgraded = TokioIo::new(upgraded);
                let mut upstream = upstream;
                // A route change drops this tunnel.
                let rerouted = async {
                    if route_rx.changed().await.is_ok() {
                        return;
                    }

                    std::future::pending::<()>().await
                };

                tokio::select! {
                    copied = tokio::io::copy_bidirectional(&mut upgraded, &mut upstream) => {
                        if let Err(err) = copied {
                            tracing::debug!("tunnel closed: {err}");
                        }
                    }
                    _ = rerouted => {
                        tracing::debug!("tunnel cut: the workspace route changed");
                    }
                }
            }
            Err(err) => tracing::warn!("CONNECT upgrade failed: {err}"),
        }
    });

    Response::builder()
        .status(200)
        .body(empty())
        .expect("CONNECT 200 builder valid")
}

/// Extract `host` and `port` from a `CONNECT host:port` request URI.
fn parse_authority(req: &Request<Incoming>) -> Option<(String, u16)> {
    let authority = req.uri().authority()?;
    let host = authority.host().to_owned();
    let port = authority.port_u16().unwrap_or(443);

    Some((host, port))
}
