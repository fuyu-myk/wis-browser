//! The connections a proxy is carrying, so a revive can tell a stale one from
//! a busy one at the moment it is asked.

use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::futures::Notified;
use tokio::sync::Notify;
use tokio::time::Instant;

/// The registry is first pruned at this size, then at twice what survived.
const PRUNE_FLOOR: usize = 64;

/// One CONNECT, from its dial to the end of its tunnel.
pub struct Live {
    started: Instant,
    /// Milliseconds after `started` that bytes last moved, or the current dial
    /// began.
    last_ms: AtomicU64,
    dialing: AtomicBool,
    cut: Notify,
}

impl Live {
    pub fn touch(&self) {
        let now = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.last_ms.store(now, Ordering::Relaxed);
    }

    /// The dial is done: from here it is a tunnel, quiet from now.
    pub fn established(&self) {
        self.dialing.store(false, Ordering::Relaxed);
        self.touch();
    }

    fn idle_for(&self) -> Duration {
        let last = Duration::from_millis(self.last_ms.load(Ordering::Relaxed));
        self.started.elapsed().saturating_sub(last)
    }

    /// Resolves when a revive finds this connection stale after this call;
    /// an earlier revive is not carried over.
    pub fn cut(&self) -> Notified<'_> {
        self.cut.notified()
    }
}

/// What a revive may cut.
#[derive(Debug, Clone, Copy)]
pub struct Revive {
    pub stale_after: Duration,
    /// Dials running that long start again.
    pub dials: bool,
    /// Tunnels quiet that long end.
    pub tunnels: bool,
}

#[derive(Default)]
struct Entries {
    live: Vec<Weak<Live>>,
    prune_at: usize,
}

#[derive(Clone, Default)]
pub struct Registry(Arc<Mutex<Entries>>);

impl Registry {
    #[must_use]
    pub fn register(&self) -> Arc<Live> {
        let live = Arc::new(Live {
            started: Instant::now(),
            last_ms: AtomicU64::new(0),
            dialing: AtomicBool::new(true),
            cut: Notify::new(),
        });
        let mut entries = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if entries.live.len() >= entries.prune_at.max(PRUNE_FLOOR) {
            entries.live.retain(|entry| entry.strong_count() > 0);
            entries.prune_at = entries.live.len() * 2;
        }

        entries.live.push(Arc::downgrade(&live));
        live
    }

    pub fn revive(&self, revive: Revive) {
        let mut entries = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        entries.live.retain(|entry| {
            let Some(live) = entry.upgrade() else {
                return false;
            };
            let wanted = if live.dialing.load(Ordering::Relaxed) {
                revive.dials
            } else {
                revive.tunnels
            };
            if wanted && live.idle_for() >= revive.stale_after {
                live.cut.notify_waiters();
            }

            true
        });
    }
}

/// `inner`, recording on `live` each read that moves bytes. A tunnel wraps
/// both its ends, so a read on either side is traffic in one direction.
pub struct Stamped<T> {
    inner: T,
    live: Arc<Live>,
}

impl<T> Stamped<T> {
    pub fn new(inner: T, live: Arc<Live>) -> Self {
        Self { inner, live }
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for Stamped<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buf.filled().len();
        let polled = Pin::new(&mut self.inner).poll_read(cx, buf);
        if matches!(polled, Poll::Ready(Ok(()))) && buf.filled().len() > before {
            self.live.touch();
        }

        polled
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for Stamped<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: Revive = Revive {
        stale_after: Duration::from_millis(40),
        dials: true,
        tunnels: true,
    };

    /// Whether `live` is cut by a revive issued once it is being watched, as
    /// the proxy's `select!` watches it.
    async fn cut_by(registry: &Registry, live: &Live, revive: Revive) -> bool {
        let cut = live.cut();
        registry.revive(revive);
        tokio::time::timeout(Duration::from_millis(1), cut)
            .await
            .is_ok()
    }

    #[tokio::test(start_paused = true)]
    async fn a_revive_cuts_only_what_has_been_quiet_long_enough() {
        let registry = Registry::default();
        let quiet = registry.register();
        tokio::time::advance(Duration::from_millis(60)).await;
        let fresh = registry.register();

        assert!(cut_by(&registry, &quiet, ALL).await);
        assert!(!cut_by(&registry, &fresh, ALL).await);
    }

    #[tokio::test(start_paused = true)]
    async fn a_revive_reaches_only_the_phases_it_names() {
        let registry = Registry::default();
        let dial = registry.register();
        let tunnel = registry.register();
        tunnel.established();
        tokio::time::advance(Duration::from_millis(60)).await;

        let dials_only = Revive {
            tunnels: false,
            ..ALL
        };
        assert!(cut_by(&registry, &dial, dials_only).await);
        assert!(!cut_by(&registry, &tunnel, dials_only).await);
        let tunnels_only = Revive {
            dials: false,
            ..ALL
        };
        assert!(!cut_by(&registry, &dial, tunnels_only).await);
        assert!(cut_by(&registry, &tunnel, tunnels_only).await);
    }

    #[tokio::test(start_paused = true)]
    async fn a_revive_before_the_future_exists_is_not_carried_over() {
        let registry = Registry::default();
        let live = registry.register();
        tokio::time::advance(Duration::from_millis(60)).await;
        registry.revive(ALL);
        live.established();

        assert!(tokio::time::timeout(Duration::from_millis(1), live.cut())
            .await
            .is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn bytes_moving_keep_a_tunnel_from_counting_as_quiet() {
        let registry = Registry::default();
        let live = registry.register();
        live.established();
        tokio::time::advance(Duration::from_millis(60)).await;
        let (near, mut far) = tokio::io::duplex(64);
        let mut stamped = Stamped::new(near, live.clone());
        tokio::io::AsyncWriteExt::write_all(&mut far, b"x")
            .await
            .unwrap();
        let mut byte = [0u8; 1];
        tokio::io::AsyncReadExt::read_exact(&mut stamped, &mut byte)
            .await
            .unwrap();

        assert!(!cut_by(&registry, &live, ALL).await);
    }

    #[test]
    fn finished_connections_are_pruned_and_the_next_prune_waits_for_growth() {
        let registry = Registry::default();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        rt.block_on(async {
            for _ in 0..PRUNE_FLOOR {
                drop(registry.register());
            }
            let kept: Vec<_> = (0..PRUNE_FLOOR).map(|_| registry.register()).collect();
            let entries = registry.0.lock().unwrap();
            assert_eq!(entries.live.len(), PRUNE_FLOOR);
            assert_eq!(entries.prune_at, 0, "pruned down to nothing, then refilled");
            drop(kept);
        });
    }
}
