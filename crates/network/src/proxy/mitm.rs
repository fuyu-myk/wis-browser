//! MITM tunnel: terminate the WebView's TLS with a per-SNI leaf cert, then
//! re-initiate TLS to the real upstream. Each inner HTTP request runs
//! through the same policy as plain HTTP, so path-level tracker rules
//! finally apply inside HTTPS.

use std::convert::Infallible;
use std::sync::Arc;

use http_body_util::combinators::BoxBody;
use http_body_util::BodyExt;
use hyper::body::{Bytes, Incoming};
use hyper::service::service_fn;
use hyper::{Request, Response, Uri};
use hyper_util::rt::TokioIo;
use rustls::pki_types::ServerName;
use tokio::net::TcpStream;
use tokio_rustls::{TlsAcceptor, TlsConnector};
use url::Url;

use crate::policy::Decision;
use crate::resolver::Resolver;
use crate::{OutboundRequest, RequestPolicy};

use super::ca::MitmConfig;
use super::request_type::classify;
use super::upstream::{bad_gateway, empty, full};

/// Drive a MITM tunnel after the proxy has already decided to honour the
/// CONNECT. `req` is the original `CONNECT host:port` request; the caller
/// has already returned `200 Connection Established` upstream.
pub async fn run_mitm(
    req: Request<Incoming>,
    host: String,
    _port: u16,
    policy: Arc<RequestPolicy>,
    resolver: Arc<dyn Resolver>,
    mitm: Arc<MitmConfig>,
) {
    let server_config = match mitm.issuer.server_config_for(&host).await {
        Ok(cfg) => cfg,
        Err(err) => {
            tracing::warn!("mitm: issue leaf for {host}: {err}");
            return;
        }
    };

    let upgraded = match hyper::upgrade::on(req).await {
        Ok(u) => u,
        Err(err) => {
            tracing::warn!("mitm: CONNECT upgrade failed: {err}");
            return;
        }
    };
    let webview_io = TokioIo::new(upgraded);

    let acceptor = TlsAcceptor::from(server_config);
    let tls_stream = match acceptor.accept(webview_io).await {
        Ok(s) => s,
        Err(err) => {
            // Common when the WebView hasn't trusted our CA: it rejects
            // our leaf and closes the connection.
            tracing::debug!("mitm: WebView TLS handshake to {host} failed: {err}");
            return;
        }
    };

    let host_arc = Arc::new(host);
    let service = service_fn({
        let policy = policy.clone();
        let resolver = resolver.clone();
        let mitm = mitm.clone();
        let host = host_arc.clone();
        move |req: Request<Incoming>| {
            let policy = policy.clone();
            let resolver = resolver.clone();
            let mitm = mitm.clone();
            let host = host.clone();
            async move {
                let resp = handle_inner(req, host.as_str(), policy, resolver, mitm).await;
                Ok::<_, Infallible>(resp)
            }
        }
    });

    let inner_io = TokioIo::new(tls_stream);
    if let Err(err) = hyper::server::conn::http1::Builder::new()
        .serve_connection(inner_io, service)
        .await
    {
        tracing::debug!("mitm: inner http1 conn for {} closed: {err}", host_arc);
    }
}

/// Handle a single decrypted request inside the MITM session. Mirrors
/// [`super::handler::handle`] for plain HTTP, but the URL is reconstructed
/// from `(host, path)` because origin-form request URIs don't carry the
/// scheme or authority.
async fn handle_inner(
    req: Request<Incoming>,
    host: &str,
    policy: Arc<RequestPolicy>,
    resolver: Arc<dyn Resolver>,
    mitm: Arc<MitmConfig>,
) -> Response<BoxBody<Bytes, hyper::Error>> {
    let path_and_query = req
        .uri()
        .path_and_query()
        .map(|p| p.to_string())
        .unwrap_or_else(|| "/".to_string());
    let url = match Url::parse(&format!("https://{host}{path_and_query}")) {
        Ok(u) => u,
        Err(err) => return bad_gateway(&format!("mitm: invalid url: {err}")),
    };
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
        Decision::Allow => forward_https(req, host, resolver, mitm).await,
        Decision::Rewrite { url: new_url } => Response::builder()
            .status(301)
            .header("Location", new_url.as_str())
            .body(empty())
            .expect("301 builder valid"),
        Decision::Block { reason } => {
            tracing::info!(target = %url, %reason, "blocked HTTPS (MITM)");
            Response::builder()
                .status(403)
                .body(full(format!("403 Blocked by Wis: {reason}")))
                .expect("403 builder valid")
        }
    }
}

async fn forward_https(
    req: Request<Incoming>,
    host: &str,
    resolver: Arc<dyn Resolver>,
    mitm: Arc<MitmConfig>,
) -> Response<BoxBody<Bytes, hyper::Error>> {
    match forward_https_inner(req, host, resolver, mitm).await {
        Ok(resp) => resp,
        Err(err) => bad_gateway(&err),
    }
}

async fn forward_https_inner(
    req: Request<Incoming>,
    host: &str,
    resolver: Arc<dyn Resolver>,
    mitm: Arc<MitmConfig>,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, String> {
    let ip = resolver
        .resolve(host)
        .await
        .map_err(|e| format!("dns {host}: {e}"))?
        .into_iter()
        .next()
        .ok_or_else(|| format!("no addresses for {host}"))?;

    let stream = TcpStream::connect((ip, 443))
        .await
        .map_err(|e| format!("connect {ip}:443: {e}"))?;
    let connector = TlsConnector::from(mitm.client_config.clone());
    let server_name =
        ServerName::try_from(host.to_string()).map_err(|e| format!("server name {host}: {e}"))?;
    let tls = connector
        .connect(server_name, stream)
        .await
        .map_err(|e| format!("upstream TLS {host}: {e}"))?;
    let io = TokioIo::new(tls);

    let (mut sender, conn) = hyper::client::conn::http1::handshake::<_, Incoming>(io)
        .await
        .map_err(|e| format!("upstream handshake: {e}"))?;
    tokio::spawn(async move {
        if let Err(err) = conn.await {
            tracing::debug!("mitm upstream conn closed: {err}");
        }
    });

    let (mut parts, body) = req.into_parts();
    super::upstream::strip_hop_by_hop(&mut parts.headers);
    parts.headers.insert(
        "host",
        host.parse().map_err(|e| format!("hostname header: {e}"))?,
    );
    parts.uri = parts
        .uri
        .path_and_query()
        .map(|p| p.to_string())
        .unwrap_or_else(|| "/".to_string())
        .parse::<Uri>()
        .map_err(|e| format!("origin-form uri: {e}"))?;

    let upstream_req = Request::from_parts(parts, body);
    let upstream_resp = sender
        .send_request(upstream_req)
        .await
        .map_err(|e| format!("send_request: {e}"))?;
    let (mut resp_parts, resp_body) = upstream_resp.into_parts();
    super::upstream::strip_hop_by_hop(&mut resp_parts.headers);
    let body = resp_body.boxed();

    Ok(Response::from_parts(resp_parts, body))
}
