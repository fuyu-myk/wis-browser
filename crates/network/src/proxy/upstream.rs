//! Outbound HTTP forwarding. The proxy receives a non-CONNECT request,
//! [`crate::RequestPolicy`] has just said `Allow`, and now that request
//! needs to be delivered to the real origin server and the response piped
//! back.
//!
//! Deliberately speak HTTP only here: HTTPS upstreams arrive as
//! `CONNECT` and are tunneled raw (see [`super::tunnel`]).

use std::sync::Arc;

use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};
use hyper::body::{Bytes, Incoming};
use hyper::{HeaderMap, Request, Response, Uri};
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;
use url::Url;

use crate::Resolver;

/// Forward an HTTP request through to its upstream and return the
/// response (with the body streamed through unchanged). Hop-by-hop
/// headers are stripped on both legs.
pub async fn forward(
    req: Request<Incoming>,
    resolver: Arc<dyn Resolver>,
) -> Response<BoxBody<Bytes, hyper::Error>> {
    match forward_inner(req, resolver).await {
        Ok(resp) => resp,
        Err(reason) => bad_gateway(&reason),
    }
}

async fn forward_inner(
    req: Request<Incoming>,
    resolver: Arc<dyn Resolver>,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, String> {
    let uri = req.uri().clone();
    let url = parse_uri_as_url(&uri).map_err(|e| format!("invalid request URI {uri}: {e}"))?;
    let host = url
        .host_str()
        .ok_or_else(|| format!("upstream URL {url} has no host"))?
        .to_owned();
    let port = url.port_or_known_default().unwrap_or(80);
    let path_and_query = uri
        .path_and_query()
        .map(|p| p.to_string())
        .unwrap_or_else(|| "/".to_string());

    // DoH-resolve the host, then connect to (ip, port). Bypassing the OS
    // resolver is half the privacy story.
    let ip = resolver
        .resolve(&host)
        .await
        .map_err(|e| format!("dns {host}: {e}"))?
        .into_iter()
        .next()
        .ok_or_else(|| format!("no addresses for {host}"))?;

    let stream = TcpStream::connect((ip, port))
        .await
        .map_err(|e| format!("connect {ip}:{port}: {e}"))?;
    let io = TokioIo::new(stream);

    let (mut sender, conn) = hyper::client::conn::http1::handshake::<_, Incoming>(io)
        .await
        .map_err(|e| format!("handshake: {e}"))?;
    tokio::spawn(async move {
        if let Err(err) = conn.await {
            tracing::debug!("upstream conn closed: {err}");
        }
    });

    // Rebuild the request: origin-form URI, hop-by-hop headers dropped,
    // explicit Host added (the origin server expects it).
    let (mut parts, body) = req.into_parts();
    strip_hop_by_hop(&mut parts.headers);
    parts.headers.insert(
        "host",
        host.parse().expect("hostname is a valid header value"),
    );
    parts.uri = path_and_query
        .parse::<Uri>()
        .map_err(|e| format!("invalid origin-form uri: {e}"))?;

    let upstream_req = Request::from_parts(parts, body);

    let upstream_resp = sender
        .send_request(upstream_req)
        .await
        .map_err(|e| format!("send_request: {e}"))?;
    let (mut resp_parts, resp_body) = upstream_resp.into_parts();
    strip_hop_by_hop(&mut resp_parts.headers);
    let body = resp_body.boxed();

    Ok(Response::from_parts(resp_parts, body))
}

fn parse_uri_as_url(uri: &Uri) -> Result<Url, String> {
    let s = uri.to_string();
    Url::parse(&s).map_err(|e| e.to_string())
}

pub(super) fn strip_hop_by_hop(headers: &mut HeaderMap) {
    const HOP_BY_HOP: &[&str] = &[
        "connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailers",
        "transfer-encoding",
        "upgrade",
    ];

    let extra: Vec<String> = headers
        .get("connection")
        .and_then(|v| v.to_str().ok())
        .map(|v| {
            v.split(',')
                .map(|s| s.trim().to_ascii_lowercase())
                .collect()
        })
        .unwrap_or_default();

    for name in HOP_BY_HOP {
        headers.remove(*name);
    }
    for name in extra {
        headers.remove(name.as_str());
    }
}

pub(crate) fn bad_gateway(reason: &str) -> Response<BoxBody<Bytes, hyper::Error>> {
    tracing::warn!(reason, "upstream forward failed");
    Response::builder()
        .status(502)
        .body(full(format!("502 Bad Gateway: {reason}")))
        .expect("response builder valid")
}

pub(crate) fn full(s: impl Into<Bytes>) -> BoxBody<Bytes, hyper::Error> {
    Full::new(s.into()).map_err(|never| match never {}).boxed()
}

pub(crate) fn empty() -> BoxBody<Bytes, hyper::Error> {
    Empty::<Bytes>::new()
        .map_err(|never| match never {})
        .boxed()
}
