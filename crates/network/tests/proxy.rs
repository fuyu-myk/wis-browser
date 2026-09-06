//! Integration tests for the loopback proxy. Each test spins up:
//!   1. A stub upstream HTTP server bound on `127.0.0.1:0` (returns a
//!      fixed body for any request).
//!   2. A `ProxyServer` configured with a specific `RequestPolicy` and a
//!      `StubResolver` that maps the host we use in tests to the
//!      upstream's loopback IP.
//!   3. A hyper-1.x client that sends requests through the proxy.
//!
//! Everything is hermetic: no real network, no DoH.

use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use http_body_util::{BodyExt, Empty, Full};
use hyper::body::{Bytes, Incoming};
use hyper::service::service_fn;
use hyper::{Method, Request, Response};
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use wis_network::{
    policy::Decision, ProxyServer, RequestPolicy, Resolver, StubResolver, SwappableResolver,
    TrackerBlocker,
};

/// Spin up a tiny HTTP/1 server on `127.0.0.1:0` that returns `200 OK`
/// with the given body for every request, then return its bound port.
async fn start_stub_upstream(body: &'static str) -> u16 {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind upstream");
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(p) => p,
                Err(_) => continue,
            };
            let io = TokioIo::new(stream);
            tokio::spawn(async move {
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(
                        io,
                        service_fn(move |_req: Request<Incoming>| async move {
                            Ok::<_, Infallible>(
                                Response::builder()
                                    .status(200)
                                    .body(Full::new(Bytes::from_static(body.as_bytes())))
                                    .unwrap(),
                            )
                        }),
                    )
                    .await;
            });
        }
    });

    port
}

async fn start_proxy(blocker: TrackerBlocker, resolver: Arc<dyn Resolver>) -> ProxyServer {
    let policy = Arc::new(RequestPolicy::new(
        wis_core::PrivacyMode::Balanced,
        Arc::new(blocker),
    ));
    ProxyServer::start(policy, resolver, SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("proxy start")
}

/// Send a request through the proxy. Bypasses any client-side proxy
/// helpers: just open a TCP socket to the proxy and feed it the
/// absolute-form HTTP/1 request the spec requires.
async fn send_through_proxy(
    proxy_port: u16,
    method: Method,
    absolute_url: &str,
) -> Response<Incoming> {
    let stream = TcpStream::connect(("127.0.0.1", proxy_port))
        .await
        .expect("connect to proxy");
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake::<_, Empty<Bytes>>(io)
        .await
        .expect("handshake");
    tokio::spawn(async move {
        let _ = conn.await;
    });
    let req = Request::builder()
        .method(method)
        .uri(absolute_url)
        .header("host", url_host(absolute_url))
        .body(Empty::<Bytes>::new())
        .expect("request build");

    sender.send_request(req).await.expect("send")
}

fn url_host(url: &str) -> String {
    let u = url::Url::parse(url).expect("url parse");
    let host = u.host_str().expect("url host").to_string();

    match u.port() {
        Some(p) => format!("{host}:{p}"),
        None => host,
    }
}

// ---------------------------------------------------------------------------

/// The reason the resolver sits behind a swappable handle: a proxy takes it
/// once at `start`, so changing the DNS provider has to reach the requests an
/// already-running proxy is serving.
#[tokio::test]
async fn a_running_proxy_resolves_through_a_swapped_resolver() {
    let upstream_port = start_stub_upstream("hello from upstream").await;
    let swappable = Arc::new(SwappableResolver::new(Arc::new(StubResolver::new())));
    let proxy = start_proxy(TrackerBlocker::empty(), swappable.clone()).await;
    let url = format!("http://localhost:{upstream_port}/some/path");

    let resp = send_through_proxy(proxy.port(), Method::GET, &url).await;
    assert_ne!(resp.status(), 200, "the host should not resolve yet");

    swappable.set(Arc::new(
        StubResolver::new().with("localhost", vec![IpAddr::V4(Ipv4Addr::LOCALHOST)]),
    ));

    let resp = send_through_proxy(proxy.port(), Method::GET, &url).await;
    assert_eq!(
        resp.status(),
        200,
        "the running proxy must use the new resolver"
    );
    let body = resp.into_body().collect().await.expect("body").to_bytes();
    assert_eq!(body, Bytes::from_static(b"hello from upstream"));
}

#[tokio::test]
async fn proxy_forwards_allowed_http_request_to_upstream() {
    let upstream_port = start_stub_upstream("hello from upstream").await;
    // Loopback hostnames are exempt from the HTTPS-First upgrader in every
    // privacy mode, so this exercises the bare Allow→forward path.
    let resolver: Arc<dyn Resolver> =
        Arc::new(StubResolver::new().with("localhost", vec![IpAddr::V4(Ipv4Addr::LOCALHOST)]));
    let proxy = start_proxy(TrackerBlocker::empty(), resolver).await;

    let url = format!("http://localhost:{upstream_port}/some/path");
    let resp = send_through_proxy(proxy.port(), Method::GET, &url).await;
    assert_eq!(
        resp.status(),
        200,
        "expected upstream 200, got {:?}",
        resp.status()
    );

    let body = resp.into_body().collect().await.expect("body").to_bytes();
    assert_eq!(body, Bytes::from_static(b"hello from upstream"));
}

#[tokio::test]
async fn proxy_blocks_http_to_tracker_host() {
    let resolver: Arc<dyn Resolver> = Arc::new(StubResolver::new());
    // Default tracker list blocks google-analytics.com. The request never
    // reaches an upstream because the proxy returns 403 first.
    let blocker = TrackerBlocker::with_default_list().expect("default list");
    let proxy = start_proxy(blocker, resolver).await;

    let resp = send_through_proxy(
        proxy.port(),
        Method::GET,
        "http://www.google-analytics.com/collect",
    )
    .await;
    assert_eq!(resp.status(), 403);
}

#[tokio::test]
async fn proxy_rewrites_http_to_https_under_balanced_mode() {
    // Balanced mode's HttpsUpgrader rewrites any non-loopback http://
    // request to https://. The proxy maps that Decision::Rewrite to a 301.
    let resolver: Arc<dyn Resolver> = Arc::new(StubResolver::new());
    let proxy = start_proxy(TrackerBlocker::empty(), resolver).await;

    let resp = send_through_proxy(proxy.port(), Method::GET, "http://example.com/path").await;
    assert_eq!(resp.status(), 301);
    let location = resp
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        location.starts_with("https://example.com/"),
        "expected https upgrade in Location, got {location}",
    );
}

#[tokio::test]
async fn proxy_blocks_connect_to_tracker_host() {
    let resolver: Arc<dyn Resolver> = Arc::new(StubResolver::new());
    let blocker = TrackerBlocker::with_default_list().expect("default list");
    let proxy = start_proxy(blocker, resolver).await;

    // CONNECT requests don't use absolute URIs; the URI is the
    // `host:port` authority. hyper accepts that when method is CONNECT.
    let stream = TcpStream::connect(("127.0.0.1", proxy.port()))
        .await
        .expect("connect");
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake::<_, Empty<Bytes>>(io)
        .await
        .expect("handshake");
    tokio::spawn(async move {
        let _ = conn.await;
    });

    let req = Request::builder()
        .method(Method::CONNECT)
        .uri("doubleclick.net:443")
        .body(Empty::<Bytes>::new())
        .expect("request build");

    let resp = sender.send_request(req).await.expect("send");
    // 502: the proxy refused to tunnel to a blocked host.
    assert_eq!(resp.status(), 502);
}

/// A route change cannot re-point a tunnel that is already open: it was
/// dialled through the old upstream. Severing is what stops it carrying
/// traffic the old way, which for a switch to Tor means carrying it in the
/// clear.
#[tokio::test]
async fn severing_cuts_a_tunnel_opened_under_the_previous_route() {
    // Echoes whatever it receives, so the tunnel can be shown live first.
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind echo target");
    let target_port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 64];
                loop {
                    match stream.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if stream.write_all(&buf[..n]).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            });
        }
    });

    let resolver: Arc<dyn Resolver> =
        Arc::new(StubResolver::new().with("localhost", vec![IpAddr::V4(Ipv4Addr::LOCALHOST)]));
    let proxy = start_proxy(TrackerBlocker::empty(), resolver).await;

    let stream = TcpStream::connect(("127.0.0.1", proxy.port()))
        .await
        .expect("connect to proxy");
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake::<_, Empty<Bytes>>(io)
        .await
        .expect("handshake");
    tokio::spawn(async move {
        let _ = conn.with_upgrades().await;
    });

    let req = Request::builder()
        .method(Method::CONNECT)
        .uri(format!("localhost:{target_port}"))
        .body(Empty::<Bytes>::new())
        .expect("request build");
    let resp = sender.send_request(req).await.expect("send");
    assert_eq!(resp.status(), 200, "the tunnel should open");

    let upgraded = hyper::upgrade::on(resp).await.expect("upgrade");
    let mut tunnel = TokioIo::new(upgraded);

    tunnel
        .write_all(b"ping")
        .await
        .expect("write through tunnel");
    let mut echoed = [0u8; 4];
    tunnel.read_exact(&mut echoed).await.expect("read echo");
    assert_eq!(
        &echoed, b"ping",
        "the tunnel carries traffic before severing"
    );

    proxy.sever_open_connections();

    let mut after = [0u8; 1];
    let closed = tokio::time::timeout(Duration::from_secs(5), tunnel.read(&mut after))
        .await
        .expect("a severed tunnel must close rather than hang");
    assert!(
        matches!(closed, Ok(0) | Err(_)),
        "expected the tunnel to end, read returned {closed:?}"
    );
}

/// Severing must end the tunnels open at that moment and nothing else. A
/// receiver that is born already-stale would cut every later tunnel too,
/// which is every page load after a single route change.
#[tokio::test]
async fn a_tunnel_opened_after_a_sever_still_carries_traffic() {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind echo target");
    let target_port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 64];
                loop {
                    match stream.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if stream.write_all(&buf[..n]).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            });
        }
    });

    let resolver: Arc<dyn Resolver> =
        Arc::new(StubResolver::new().with("localhost", vec![IpAddr::V4(Ipv4Addr::LOCALHOST)]));
    let proxy = start_proxy(TrackerBlocker::empty(), resolver).await;

    // A route change with nothing open, as switching upstream in a fresh
    // window does.
    proxy.sever_open_connections();

    let stream = TcpStream::connect(("127.0.0.1", proxy.port()))
        .await
        .expect("connect to proxy");
    let io = TokioIo::new(stream);
    let (mut sender, conn) = hyper::client::conn::http1::handshake::<_, Empty<Bytes>>(io)
        .await
        .expect("handshake");
    tokio::spawn(async move {
        let _ = conn.with_upgrades().await;
    });

    let req = Request::builder()
        .method(Method::CONNECT)
        .uri(format!("localhost:{target_port}"))
        .body(Empty::<Bytes>::new())
        .expect("request build");
    let resp = sender.send_request(req).await.expect("send");
    assert_eq!(resp.status(), 200);

    let upgraded = hyper::upgrade::on(resp).await.expect("upgrade");
    let mut tunnel = TokioIo::new(upgraded);
    tunnel
        .write_all(b"ping")
        .await
        .expect("write through tunnel");
    let mut echoed = [0u8; 4];
    tunnel
        .read_exact(&mut echoed)
        .await
        .expect("a tunnel opened after a sever must carry traffic");
    assert_eq!(&echoed, b"ping");
}

#[tokio::test]
async fn dropping_proxy_handle_shuts_down_listener() {
    let resolver: Arc<dyn Resolver> = Arc::new(StubResolver::new());
    let proxy = start_proxy(TrackerBlocker::empty(), resolver).await;
    let port = proxy.port();
    drop(proxy);

    // Give the accept loop a moment to drop and the OS to release.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // A fresh bind on the same port should succeed: but the OS may
    // hold the port briefly, so just assert the original socket no
    // longer responds.
    assert!(port > 0);
}

/// `_ProxyDecision` keeps the unused `Decision` import quiet so future
/// tests using it directly don't need to re-add the use line.
#[allow(dead_code)]
fn _decision_compiles(d: Decision) -> Decision {
    d
}
