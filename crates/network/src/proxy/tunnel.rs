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
use tokio::net::TcpStream;

use crate::resolver::Resolver;

use super::upstream::{bad_gateway, empty};

pub async fn tunnel(
    req: Request<Incoming>,
    resolver: Arc<dyn Resolver>,
) -> Response<BoxBody<Bytes, hyper::Error>> {
    let (host, port) = match parse_authority(&req) {
        Some(pair) => pair,
        None => return bad_gateway("CONNECT target missing host:port"),
    };

    // Resolve + open upstream eagerly so a meaningful 502 can still be
    // returned before sending 200.
    let ip = match resolver.resolve(&host).await {
        Ok(ips) => match ips.into_iter().next() {
            Some(ip) => ip,
            None => return bad_gateway(&format!("no addresses for {host}")),
        },
        Err(err) => return bad_gateway(&format!("dns {host}: {err}")),
    };

    let upstream = match TcpStream::connect((ip, port)).await {
        Ok(s) => s,
        Err(err) => return bad_gateway(&format!("connect {ip}:{port}: {err}")),
    };

    tokio::spawn(async move {
        match hyper::upgrade::on(req).await {
            Ok(upgraded) => {
                let mut upgraded = TokioIo::new(upgraded);
                let mut upstream = upstream;
                if let Err(err) = tokio::io::copy_bidirectional(&mut upgraded, &mut upstream).await
                {
                    tracing::debug!("tunnel closed: {err}");
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
