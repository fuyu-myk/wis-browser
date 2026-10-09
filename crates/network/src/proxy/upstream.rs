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
use hyper::header::HeaderValue;
use hyper::{HeaderMap, Request, Response, Uri};
use hyper_util::rt::TokioIo;
use url::Url;
use wis_core::UpstreamProxy;

use super::live::Stamped;
use super::signals::{dial, ended, Signals};
use crate::Resolver;

/// Forward an HTTP request through to its upstream and return the
/// response (with the body streamed through unchanged). Hop-by-hop
/// headers are stripped on both legs. When `upstream` is set the connection
/// is routed through it. `global_privacy_control` adds `Sec-GPC: 1`. The
/// connection ends early on what `signals` carry, as a tunnel does.
pub async fn forward(
    req: Request<Incoming>,
    resolver: Arc<dyn Resolver>,
    upstream: Option<UpstreamProxy>,
    global_privacy_control: bool,
    signals: Signals,
) -> Response<BoxBody<Bytes, hyper::Error>> {
    match forward_inner(req, resolver, upstream, global_privacy_control, signals).await {
        Ok(resp) => resp,
        Err(reason) => bad_gateway(&reason),
    }
}

async fn forward_inner(
    req: Request<Incoming>,
    resolver: Arc<dyn Resolver>,
    upstream: Option<UpstreamProxy>,
    global_privacy_control: bool,
    mut signals: Signals,
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

    // Connect directly (DoH-resolve + dial) or through the upstream proxy.
    let live = signals.register();
    let stream = dial(
        &host,
        port,
        upstream.as_ref(),
        &resolver,
        &mut signals,
        &live,
    )
    .await?;
    let io = TokioIo::new(Stamped::new(stream, live.clone()));

    let (mut sender, conn) = hyper::client::conn::http1::handshake::<_, Incoming>(io)
        .await
        .map_err(|e| format!("handshake: {e}"))?;
    tokio::spawn(async move {
        tokio::select! {
            done = conn => {
                if let Err(err) = done {
                    tracing::debug!("upstream conn closed: {err}");
                }
            }
            why = ended(&mut signals, &live) => {
                tracing::debug!("upstream conn cut: {}", why.reason());
            }
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
    if global_privacy_control {
        parts
            .headers
            .insert("sec-gpc", HeaderValue::from_static("1"));
    } else {
        parts.headers.remove("sec-gpc");
    }
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
