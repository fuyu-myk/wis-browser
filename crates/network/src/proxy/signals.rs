//! What can end a proxied connection early, shared by CONNECT tunnels and
//! plain-HTTP forwards.

use std::sync::Arc;

use tokio::net::TcpStream;
use tokio::sync::watch;
use wis_core::UpstreamProxy;

use crate::resolver::Resolver;

use super::connect::connect_target;
use super::live::{Live, Registry};

/// The workspace's route changing, the network changing, or a revive finding
/// a connection stale.
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

    pub(super) fn register(&self) -> Arc<Live> {
        self.live.register()
    }
}

/// Why a connection ended early.
pub(super) enum Ended {
    Rerouted,
    NetworkChanged,
    Revived,
}

impl Ended {
    pub(super) fn reason(&self) -> &'static str {
        match self {
            Self::Rerouted => "the workspace route changed",
            Self::NetworkChanged => "the network changed",
            Self::Revived => "a reload found it idle",
        }
    }
}

enum Dial {
    Done(Result<TcpStream, String>),
    Rerouted,
    Again,
}

/// Dial `host:port` (directly or through `upstream`), watching `signals`. A
/// route change fails the dial, since its upstream is stale; a network change,
/// or a revive finding the dial stale, starts it once more over the network.
/// `live` is marked established on success.
pub(super) async fn dial(
    host: &str,
    port: u16,
    upstream: Option<&UpstreamProxy>,
    resolver: &Arc<dyn Resolver>,
    signals: &mut Signals,
    live: &Live,
) -> Result<TcpStream, String> {
    let mut redialed = false;
    loop {
        let outcome = tokio::select! {
            dialed = connect_target(host, port, upstream, resolver) => Dial::Done(dialed),
            () = bumped(&mut signals.route) => Dial::Rerouted,
            () = bumped(&mut signals.network) => Dial::Again,
            () = live.cut() => Dial::Again,
        };
        match outcome {
            Dial::Done(dialed) => {
                if dialed.is_ok() {
                    live.established();
                }
                return dialed;
            }
            Dial::Rerouted => return Err("cut while dialing: the route changed".into()),
            Dial::Again if !redialed => {
                redialed = true;
                live.touch();
            }
            Dial::Again => return Err("cut while dialing, twice".into()),
        }
    }
}

/// Resolves when an established connection should end, saying why.
pub(super) async fn ended(signals: &mut Signals, live: &Live) -> Ended {
    tokio::select! {
        () = bumped(&mut signals.route) => Ended::Rerouted,
        () = bumped(&mut signals.network) => Ended::NetworkChanged,
        () = live.cut() => Ended::Revived,
    }
}

/// Resolves on the next bump of `rx`; never, once the proxy is gone.
async fn bumped(rx: &mut watch::Receiver<u64>) {
    if rx.changed().await.is_ok() {
        return;
    }

    std::future::pending::<()>().await
}
