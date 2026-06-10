//! Debounced, ordered persistence of [`BrowserState`] snapshots.
//!
//! Every mutation produces a full-state snapshot, so high-frequency bursts
//! (drag reorders, zoom ticks, per-page load/title sync) used to pay one
//! write + fsync each. The coalescer keeps only the **latest** snapshot and
//! writes it once after a short quiet period; destructive transitions still
//! write immediately through [`SaveCoalescer::write_now`].
//!
//! Ordering safety: snapshots are stamped with a monotonically increasing
//! sequence under the state lock, and [`SaveCoalescer::write_now`] refuses to
//! write a snapshot older than the last committed one, so a delayed debounced
//! write can never roll the database back behind an immediate write that
//! followed it.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use wis_core::BrowserState;
use wis_storage::{Storage, StorageResult};

/// Quiet period before a queued snapshot is written.
const DEBOUNCE: Duration = Duration::from_millis(200);

pub struct SaveCoalescer {
    /// Latest queued snapshot (newer queues replace older, unwritten ones).
    pending: Mutex<Option<(u64, BrowserState)>>,
    /// Whether the drain task is alive.
    scheduled: AtomicBool,
    /// Stamp source. Incremented under the browser state lock, so sequence
    /// order matches mutation order.
    seq: AtomicU64,
    /// Highest sequence actually written to disk.
    committed: AtomicU64,
    /// Serializes writers so the committed check-and-write is atomic.
    write_lock: Mutex<()>,
}

impl SaveCoalescer {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(None),
            scheduled: AtomicBool::new(false),
            seq: AtomicU64::new(0),
            committed: AtomicU64::new(0),
            write_lock: Mutex::new(()),
        }
    }

    /// Next snapshot stamp. Call while holding the browser state lock so the
    /// stamp order matches the mutation order.
    pub fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Write `state` unless a newer snapshot has already been committed
    /// (then it is a no-op success: the newer write covered this one).
    /// Blocking; call from a blocking-ok context.
    pub fn write_now(
        &self,
        storage: &Arc<dyn Storage>,
        seq: u64,
        state: &BrowserState,
    ) -> StorageResult<()> {
        let _guard = self.write_lock.lock().expect("save write lock poisoned");
        if seq <= self.committed.load(Ordering::SeqCst) {
            return Ok(());
        }

        storage.save_state(state)?;
        self.committed.store(seq, Ordering::SeqCst);

        Ok(())
    }

    /// Queue a debounced write of `state`, replacing any not-yet-written
    /// snapshot. Call while holding the browser state lock (stamping order).
    /// Failures are logged and self-heal: the next mutation queues a newer
    /// snapshot that supersedes the failed one.
    pub fn schedule(self: &Arc<Self>, storage: Arc<dyn Storage>, state: BrowserState) {
        let seq = self.next_seq();
        *self.pending.lock().expect("save pending poisoned") = Some((seq, state));
        if self.scheduled.swap(true, Ordering::SeqCst) {
            return;
        }

        let this = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(DEBOUNCE).await;
                let taken = this.pending.lock().expect("save pending poisoned").take();
                match taken {
                    Some((seq, state)) => {
                        let writer = Arc::clone(&this);
                        let storage = Arc::clone(&storage);
                        let result = tauri::async_runtime::spawn_blocking(move || {
                            writer.write_now(&storage, seq, &state)
                        })
                        .await;
                        match result {
                            Ok(Ok(())) => {}
                            Ok(Err(err)) => log::error!(
                                "coalesced state save failed (next change retries): {err}"
                            ),
                            Err(err) => log::error!("coalesced save task panicked: {err}"),
                        }
                    }
                    None => {
                        this.scheduled.store(false, Ordering::SeqCst);
                        // A writer may have queued between the take() and the
                        // store above (it saw `scheduled` still true and
                        // didn't spawn); reclaim the slot if so.
                        let stranded = this
                            .pending
                            .lock()
                            .expect("save pending poisoned")
                            .is_some();
                        if stranded && !this.scheduled.swap(true, Ordering::SeqCst) {
                            continue;
                        }

                        break;
                    }
                }
            }
        });
    }

    /// Synchronously write any pending snapshot. Quit path: must run before
    /// the process exits so a debounced save is never lost.
    pub fn flush(&self, storage: &Arc<dyn Storage>) {
        let taken = self.pending.lock().expect("save pending poisoned").take();
        if let Some((seq, state)) = taken {
            if let Err(err) = self.write_now(storage, seq, &state) {
                log::error!("state flush on quit failed: {err}");
            }
        }
    }
}

impl Default for SaveCoalescer {
    fn default() -> Self {
        Self::new()
    }
}
