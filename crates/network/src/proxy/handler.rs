//! Per-request service handler. Decides: via the workspace's
//! [`crate::RequestPolicy`]: whether to forward, rewrite, or block.

use std::sync::Arc;

use http_body_util::combinators::BoxBody;
use hyper::body::{Bytes, Incoming};
use hyper::{Method, Request, Response};
use url::Url;

use crate::{policy::Decision, OutboundRequest, RequestPolicy, Resolver};

use super::ca::MitmConfig;
use super::request_type::classify;
use super::tunnel::tunnel;
use super::upstream::{bad_gateway, empty, forward, full};

/// Entry point for every request the proxy accepts.
pub async fn handle(
    req: Request<Incoming>,
    policy: Arc<RequestPolicy>,
    resolver: Arc<dyn Resolver>,
    mitm: Option<Arc<MitmConfig>>,
) -> Response<BoxBody<Bytes, hyper::Error>> {
    if req.method() == Method::CONNECT {
        return handle_connect(req, policy, resolver, mitm).await;
    }
    handle_http(req, policy, resolver).await
}

async fn handle_http(
    req: Request<Incoming>,
    policy: Arc<RequestPolicy>,
    resolver: Arc<dyn Resolver>,
) -> Response<BoxBody<Bytes, hyper::Error>> {
    // Parse the URL from the absolute request-target. Proxy-form requests
    // carry the full URL; if an origin-form request is somehow received,
    // reject as it cannot be classified.
    let url = match Url::parse(&req.uri().to_string()) {
        Ok(u) if u.has_host() => u,
        _ => return bad_request("proxy expects absolute-form HTTP requests"),
    };

    // The source URL: the page that originated the request: comes from
    // the `Referer` header when present. The request URL is used when absent,
    // which makes the request first-party (best-effort).
    let referer = req
        .headers()
        .get("referer")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| Url::parse(s).ok())
        .unwrap_or_else(|| url.clone());

    let request_type = classify(req.headers());

    let outbound = OutboundRequest {
        url: &url,
        source_url: &referer,
        request_type,
    };
    let decision = match policy.decide(&outbound) {
        Ok(d) => d,
        Err(err) => return bad_gateway(&format!("policy error: {err}")),
    };

    match decision {
        Decision::Allow => forward(req, resolver).await,
        Decision::Rewrite { url: new_url } => moved_permanently(new_url.as_str()),
        Decision::Block { reason } => {
            tracing::info!(target = %url, %reason, "blocked HTTP");
            forbidden(&reason)
        }
    }
}

async fn handle_connect(
    req: Request<Incoming>,
    policy: Arc<RequestPolicy>,
    resolver: Arc<dyn Resolver>,
    mitm: Option<Arc<MitmConfig>>,
) -> Response<BoxBody<Bytes, hyper::Error>> {
    // The path can't be seen for HTTPS, an https://host/ document request
    // is synthesized and run that through the policy. Host-level
    // blocking works; path-level rules only apply to plain HTTP requests.
    let authority = match req.uri().authority() {
        Some(a) => a,
        None => return bad_gateway("CONNECT missing authority"),
    };
    let synthetic = match Url::parse(&format!("https://{}/", authority.host())) {
        Ok(u) => u,
        Err(err) => return bad_gateway(&format!("invalid CONNECT host: {err}")),
    };

    let outbound = OutboundRequest {
        url: &synthetic,
        source_url: &synthetic,
        request_type: crate::RequestType::Document,
    };
    let decision = match policy.decide(&outbound) {
        Ok(d) => d,
        Err(err) => return bad_gateway(&format!("policy error: {err}")),
    };

    match decision {
        Decision::Allow | Decision::Rewrite { .. } => tunnel(req, resolver, policy, mitm).await,
        Decision::Block { reason } => {
            tracing::info!(target = %synthetic, %reason, "blocked CONNECT");
            Response::builder()
                .status(502)
                .body(full(format!("502 Blocked: {reason}")))
                .expect("blocked response builder valid")
        }
    }
}

fn moved_permanently(location: &str) -> Response<BoxBody<Bytes, hyper::Error>> {
    Response::builder()
        .status(301)
        .header("Location", location)
        .body(empty())
        .expect("301 builder valid")
}

fn forbidden(reason: &str) -> Response<BoxBody<Bytes, hyper::Error>> {
    Response::builder()
        .status(403)
        .body(full(format!("403 Blocked by Wis: {reason}")))
        .expect("403 builder valid")
}

fn bad_request(reason: &str) -> Response<BoxBody<Bytes, hyper::Error>> {
    Response::builder()
        .status(400)
        .body(full(format!("400 Bad Request: {reason}")))
        .expect("400 builder valid")
}
