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
use tokio::sync::watch;
use wis_core::UpstreamProxy;

use crate::resolver::Resolver;

use super::connect::connect_target;
use super::live::{Registry, Stamped};
use super::upstream::{bad_gateway, empty};

/// What can end a CONNECT early: the workspace's route changing, the network
/// changing, or a revive finding it stale.
#[derive(Clone)]
pub struct Signals {
    pub(super) route: watch::Receiver<u64>,
    pub(super) network: watch::Receiver<u64>,
    pub(super) live: Registry,
}

impl Signals {
    /// A copy for one request that sees only the bumps from now on: a cloned
    /// receiver inherits the version its parent last saw, and the parent here
    /// never observes one.
    pub(super) fn fresh(&self) -> Self {
        let mut signals = self.clone();
        signals.route.mark_unchanged();
        signals.network.mark_unchanged();
        signals
    }
}

enum Dial<T> {
    Done(Result<T, String>),
    Rerouted,
    Again,
}

pub async fn tunnel(
    req: Request<Incoming>,
    resolver: Arc<dyn Resolver>,
    upstream: Option<UpstreamProxy>,
    signals: Signals,
) -> Response<BoxBody<Bytes, hyper::Error>> {
    let (host, port) = match parse_authority(&req) {
        Some(pair) => pair,
        None => return bad_gateway("CONNECT target missing host:port"),
    };
    let Signals {
        mut route,
        mut network,
        live,
    } = signals;
    let live = live.register();

    // Open the upstream eagerly (directly or via the proxy) so a meaningful
    // 502 can still be returned before sending 200. A dial the network moved
    // under starts once more rather than failing, since the 200 is not sent.
    let mut redialed = false;
    let target = loop {
        let outcome = tokio::select! {
            dialed = connect_target(&host, port, upstream.as_ref(), &resolver) => Dial::Done(dialed),
            () = bumped(&mut route) => Dial::Rerouted,
            () = bumped(&mut network) => Dial::Again,
            () = live.cut() => Dial::Again,
        };
        match outcome {
            Dial::Done(Ok(stream)) => break stream,
            Dial::Done(Err(err)) => return bad_gateway(&err),
            Dial::Rerouted => return bad_gateway("cut while dialing: the route changed"),
            Dial::Again if !redialed => {
                redialed = true;
                live.touch();
            }
            Dial::Again => return bad_gateway("cut while dialing, twice"),
        }
    };
    live.established();

    tokio::spawn(async move {
        match hyper::upgrade::on(req).await {
            Ok(upgraded) => {
                let mut upgraded = Stamped::new(TokioIo::new(upgraded), live.clone());
                let mut target = Stamped::new(target, live.clone());

                tokio::select! {
                    copied = tokio::io::copy_bidirectional(&mut upgraded, &mut target) => {
                        if let Err(err) = copied {
                            tracing::debug!("tunnel closed: {err}");
                        }
                    }
                    () = bumped(&mut route) => {
                        tracing::debug!("tunnel cut: the workspace route changed");
                    }
                    () = bumped(&mut network) => {
                        tracing::debug!("tunnel cut: the network changed");
                    }
                    () = live.cut() => {
                        tracing::debug!("tunnel cut: a reload found it idle");
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

/// Resolves on the next bump of `rx`; never, once the proxy is gone.
async fn bumped(rx: &mut watch::Receiver<u64>) {
    if rx.changed().await.is_ok() {
        return;
    }

    std::future::pending::<()>().await
}

/// Extract `host` and `port` from a `CONNECT host:port` request URI.
fn parse_authority(req: &Request<Incoming>) -> Option<(String, u16)> {
    let authority = req.uri().authority()?;
    let host = authority.host().to_owned();
    let port = authority.port_u16().unwrap_or(443);

    Some((host, port))
}
