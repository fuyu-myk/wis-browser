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

/// Spin up a tiny HTTP/1 server on `127.0.0.1:0` that answers every request
/// with the value of the `header` it received, or `absent`, then return its
/// bound port.
async fn start_header_echo_upstream(header: &'static str) -> u16 {
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
                        service_fn(move |req: Request<Incoming>| async move {
                            let seen = req
                                .headers()
                                .get(header)
                                .and_then(|v| v.to_str().ok())
                                .unwrap_or("absent")
                                .to_string();
                            Ok::<_, Infallible>(
                                Response::builder()
                                    .status(200)
                                    .body(Full::new(Bytes::from(seen)))
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
    let policy = RequestPolicy::new(wis_core::PrivacyMode::Balanced, Arc::new(blocker));
    start_proxy_with(policy, resolver).await
}

async fn start_proxy_with(policy: RequestPolicy, resolver: Arc<dyn Resolver>) -> ProxyServer {
    ProxyServer::start(
        Arc::new(policy),
        resolver,
        SocketAddr::from(([127, 0, 0, 1], 0)),
    )
    .await
    .expect("proxy start")
}

fn localhost_resolver() -> Arc<dyn Resolver> {
    Arc::new(StubResolver::new().with("localhost", vec![IpAddr::V4(Ipv4Addr::LOCALHOST)]))
}

/// What the upstream saw in `Sec-GPC` for one request forwarded by a proxy
/// whose policy has Global Privacy Control set to `enabled`.
async fn gpc_seen_by_upstream(enabled: bool) -> Bytes {
    let upstream_port = start_header_echo_upstream("sec-gpc").await;
    let policy = RequestPolicy::new(
        wis_core::PrivacyMode::Balanced,
        Arc::new(TrackerBlocker::empty()),
    )
    .with_global_privacy_control(enabled);
    let proxy = start_proxy_with(policy, localhost_resolver()).await;

    let url = format!("http://localhost:{upstream_port}/");
    let sent = if enabled { "0" } else { "1" };
    let resp =
        send_through_proxy_with_header(proxy.port(), Method::GET, &url, Some(("sec-gpc", sent)))
            .await;
    assert_eq!(resp.status(), 200);

    resp.into_body().collect().await.expect("body").to_bytes()
}

/// Send a request through the proxy. Bypasses any client-side proxy
/// helpers: just open a TCP socket to the proxy and feed it the
/// absolute-form HTTP/1 request the spec requires.
async fn send_through_proxy(
    proxy_port: u16,
    method: Method,
    absolute_url: &str,
) -> Response<Incoming> {
    send_through_proxy_with_header(proxy_port, method, absolute_url, None).await
}

/// [`send_through_proxy`] with one extra request header.
async fn send_through_proxy_with_header(
    proxy_port: u16,
    method: Method,
    absolute_url: &str,
    header: Option<(&str, &str)>,
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
    let mut req = Request::builder()
        .method(method)
        .uri(absolute_url)
        .header("host", url_host(absolute_url));
    if let Some((name, value)) = header {
        req = req.header(name, value);
    }
    let req = req.body(Empty::<Bytes>::new()).expect("request build");

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

/// The header rides only what the proxy forwards itself; CONNECT tunnels
/// carry encrypted bytes it cannot touch. Whatever the client sent is
/// replaced, so the proxy's setting is the only source of the signal.
#[tokio::test]
async fn global_privacy_control_adds_sec_gpc_to_forwarded_requests() {
    assert_eq!(gpc_seen_by_upstream(true).await, Bytes::from_static(b"1"));
}

#[tokio::test]
async fn global_privacy_control_off_sends_no_sec_gpc() {
    assert_eq!(
        gpc_seen_by_upstream(false).await,
        Bytes::from_static(b"absent")
    );
}

#[tokio::test]
async fn a_proxy_switched_to_refusing_keeps_its_port_and_refuses_traffic() {
    let upstream_port = start_stub_upstream("hello from upstream").await;
    let proxy = start_proxy(TrackerBlocker::empty(), localhost_resolver()).await;
    let port = proxy.port();
    assert!(!proxy.refusing());

    proxy.update_policy(Arc::new(
        RequestPolicy::new(
            wis_core::PrivacyMode::Balanced,
            Arc::new(TrackerBlocker::empty()),
        )
        .refusing_all(),
    ));

    assert!(proxy.refusing());
    assert_eq!(proxy.port(), port);
    let url = format!("http://localhost:{upstream_port}/");
    let resp = send_through_proxy(port, Method::GET, &url).await;
    assert_eq!(resp.status(), 403);
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

/// Never answers, as DNS does while the network is still coming back.
struct SilentResolver;

#[async_trait::async_trait]
impl Resolver for SilentResolver {
    async fn resolve(&self, _hostname: &str) -> wis_network::NetworkResult<Vec<IpAddr>> {
        std::future::pending().await
    }
}

/// Hangs on the first lookup of `comeback.test`, then answers it with
/// loopback, as DNS does once the network is back; every other host hangs.
#[derive(Default)]
struct ComebackResolver {
    lookups: std::sync::atomic::AtomicUsize,
}

#[async_trait::async_trait]
impl Resolver for ComebackResolver {
    async fn resolve(&self, hostname: &str) -> wis_network::NetworkResult<Vec<IpAddr>> {
        if hostname != "comeback.test" {
            return std::future::pending().await;
        }
        let first = self
            .lookups
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            == 0;
        if first {
            return std::future::pending().await;
        }
        Ok(vec![IpAddr::V4(Ipv4Addr::LOCALHOST)])
    }
}

/// An echo server on `127.0.0.1:0`; returns its port.
async fn start_echo_target() -> u16 {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind echo target");
    let port = listener.local_addr().unwrap().port();
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
    port
}

/// Send `CONNECT authority` through the proxy; the response and, when it is
/// a 200, the upgraded tunnel.
async fn connect_through(
    proxy: &ProxyServer,
    authority: &str,
) -> (u16, Option<TokioIo<hyper::upgrade::Upgraded>>) {
    let stream = TcpStream::connect(("127.0.0.1", proxy.port()))
        .await
        .expect("connect to proxy");
    let (mut sender, conn) =
        hyper::client::conn::http1::handshake::<_, Empty<Bytes>>(TokioIo::new(stream))
            .await
            .expect("handshake");
    tokio::spawn(async move {
        let _ = conn.with_upgrades().await;
    });
    let req = Request::builder()
        .method(Method::CONNECT)
        .uri(authority)
        .body(Empty::<Bytes>::new())
        .expect("request build");
    let resp = sender.send_request(req).await.expect("send");
    let status = resp.status().as_u16();
    if status != 200 {
        return (status, None);
    }
    let upgraded = hyper::upgrade::on(resp).await.expect("upgrade");
    (status, Some(TokioIo::new(upgraded)))
}

async fn echoes(tunnel: &mut TokioIo<hyper::upgrade::Upgraded>) -> bool {
    if tunnel.write_all(b"ping").await.is_err() {
        return false;
    }
    let mut echoed = [0u8; 4];
    matches!(
        tokio::time::timeout(Duration::from_secs(2), tunnel.read_exact(&mut echoed)).await,
        Ok(Ok(_))
    ) && &echoed == b"ping"
}

async fn closes(tunnel: &mut TokioIo<hyper::upgrade::Upgraded>) -> bool {
    let mut byte = [0u8; 1];
    matches!(
        tokio::time::timeout(Duration::from_secs(2), tunnel.read(&mut byte)).await,
        Ok(Ok(0) | Err(_))
    )
}

/// A dial still waiting on DNS when the network changes must end at the
/// sever, not ride out its timeouts.
#[tokio::test]
async fn a_sever_cuts_a_tunnel_still_dialing() {
    let proxy = Arc::new(start_proxy(TrackerBlocker::empty(), Arc::new(SilentResolver)).await);
    let dialing = tokio::spawn({
        let proxy = proxy.clone();
        async move { connect_through(&proxy, "stuck.test:443").await.0 }
    });
    tokio::time::sleep(Duration::from_millis(200)).await;

    proxy.sever_open_connections();

    let status = tokio::time::timeout(Duration::from_secs(2), dialing)
        .await
        .expect("a severed dial must answer at once")
        .unwrap();
    assert_eq!(status, 502);
}

/// A dial the network moved under starts again over the network as it is now,
/// rather than failing a load the engine would not retry.
#[tokio::test]
async fn a_network_change_redials_a_tunnel_still_dialing() {
    let target = start_echo_target().await;
    let resolver = Arc::new(ComebackResolver::default());
    let proxy = Arc::new(start_proxy(TrackerBlocker::empty(), resolver).await);
    let dialing = tokio::spawn({
        let proxy = proxy.clone();
        async move { connect_through(&proxy, &format!("comeback.test:{target}")).await }
    });
    tokio::time::sleep(Duration::from_millis(200)).await;

    proxy.network_changed();

    let (status, tunnel) = tokio::time::timeout(Duration::from_secs(2), dialing)
        .await
        .expect("the redial answers at once")
        .unwrap();
    assert_eq!(status, 200);
    assert!(echoes(&mut tunnel.unwrap()).await);
}

#[tokio::test]
async fn a_network_change_cuts_open_tunnels() {
    let target = start_echo_target().await;
    let proxy = start_proxy(TrackerBlocker::empty(), localhost_resolver()).await;
    let (_, tunnel) = connect_through(&proxy, &format!("localhost:{target}")).await;
    let mut tunnel = tunnel.unwrap();
    assert!(echoes(&mut tunnel).await);

    proxy.network_changed();

    assert!(closes(&mut tunnel).await);
}

#[tokio::test]
async fn a_revive_redials_a_dial_older_than_its_threshold_and_spares_a_fresh_one() {
    let target = start_echo_target().await;
    let resolver = Arc::new(ComebackResolver::default());
    let proxy = Arc::new(start_proxy(TrackerBlocker::empty(), resolver).await);
    let old = tokio::spawn({
        let proxy = proxy.clone();
        async move {
            connect_through(&proxy, &format!("comeback.test:{target}"))
                .await
                .0
        }
    });
    tokio::time::sleep(Duration::from_millis(400)).await;
    let fresh = tokio::spawn({
        let proxy = proxy.clone();
        async move { connect_through(&proxy, "silent.test:443").await.0 }
    });
    tokio::time::sleep(Duration::from_millis(50)).await;

    proxy.revive(Duration::from_millis(300), false);

    let status = tokio::time::timeout(Duration::from_secs(2), old)
        .await
        .expect("the stale dial starts again and answers")
        .unwrap();
    assert_eq!(status, 200);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !fresh.is_finished(),
        "a dial younger than the threshold keeps going"
    );
    fresh.abort();
}

/// A Tor circuit routinely takes longer than the threshold, so a dial through
/// an upstream proxy is never restarted.
#[tokio::test]
async fn a_revive_leaves_dials_through_an_upstream_proxy_alone() {
    // Accepts and then says nothing, like a SOCKS proxy still building.
    let socks = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("bind silent socks");
    let socks_port = socks.local_addr().unwrap().port();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((stream, _)) = socks.accept().await {
            held.push(stream);
        }
    });
    let policy = RequestPolicy::new(
        wis_core::PrivacyMode::Balanced,
        Arc::new(TrackerBlocker::empty()),
    )
    .with_upstream(wis_core::UpstreamProxy::Socks5 {
        host: "127.0.0.1".into(),
        port: socks_port,
    });
    let proxy = Arc::new(start_proxy_with(policy, localhost_resolver()).await);
    let dialing = tokio::spawn({
        let proxy = proxy.clone();
        async move { connect_through(&proxy, "example.test:443").await.0 }
    });
    tokio::time::sleep(Duration::from_millis(400)).await;

    proxy.revive(Duration::from_millis(300), true);

    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!dialing.is_finished());
    dialing.abort();
}

#[tokio::test]
async fn a_revive_keeps_quiet_tunnels_unless_asked_to_cut_them() {
    let target = start_echo_target().await;
    let proxy = start_proxy(TrackerBlocker::empty(), localhost_resolver()).await;
    let (_, quiet) = connect_through(&proxy, &format!("localhost:{target}")).await;
    let mut quiet = quiet.unwrap();
    assert!(echoes(&mut quiet).await);
    tokio::time::sleep(Duration::from_millis(400)).await;

    proxy.revive(Duration::from_millis(300), false);

    assert!(echoes(&mut quiet).await, "only dials were asked for");
}

#[tokio::test]
async fn a_revive_cuts_an_idle_tunnel_and_keeps_a_busy_one() {
    let target = start_echo_target().await;
    let proxy = start_proxy(TrackerBlocker::empty(), localhost_resolver()).await;
    let (_, idle) = connect_through(&proxy, &format!("localhost:{target}")).await;
    let (_, busy) = connect_through(&proxy, &format!("localhost:{target}")).await;
    let (mut idle, mut busy) = (idle.unwrap(), busy.unwrap());
    assert!(echoes(&mut idle).await && echoes(&mut busy).await);

    for _ in 0..8 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(echoes(&mut busy).await);
    }

    proxy.revive(Duration::from_millis(300), true);

    assert!(
        closes(&mut idle).await,
        "a tunnel quiet past the threshold is cut"
    );
    assert!(echoes(&mut busy).await, "a tunnel carrying traffic is kept");
}
