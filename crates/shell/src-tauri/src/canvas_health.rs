//! Whether a canvas's HTML cards stopped its tab responding. While cards run,
//! the page reports a start, a beat every couple of seconds and a stop; a start
//! after a run that went silent, or that never stopped, is answered "safe", and
//! the page then keeps every card paused until the user runs them. Canvases
//! whose tab closed, or whose app quit, while silent stay stuck across launches.

use std::collections::{HashMap, HashSet};

use serde::Deserialize;
use wis_core::CanvasId;

/// How long a running canvas may go without a beat before it counts as stuck.
const STALE_MS: i64 = 10_000;
/// Closing or quitting: a page beating every 2 s that has been silent this long.
const CLOSE_STALE_MS: i64 = 5_000;
const MAX_TRACKED: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Pulse {
    Start,
    Beat,
    Stop,
}

#[derive(Debug, Clone, Copy)]
struct Run {
    running: bool,
    at: i64,
}

#[derive(Debug, Default)]
pub struct CardHealth {
    runs: HashMap<(CanvasId, String), Run>,
    stuck: HashSet<CanvasId>,
    changed: bool,
}

impl CardHealth {
    /// Records a report from `label`'s page; true when a start should be safe.
    pub fn pulse(&mut self, canvas: CanvasId, label: &str, pulse: Pulse, now: i64) -> bool {
        let remembered = pulse == Pulse::Start && self.stuck.remove(&canvas);
        self.changed |= remembered;
        let safe = remembered || (pulse == Pulse::Start && self.stuck(canvas, label, now));
        if safe {
            for ((c, _), run) in self.runs.iter_mut() {
                if *c == canvas {
                    run.running = false;
                }
            }
        }

        let running = pulse != Pulse::Stop && !safe;
        self.make_room(canvas, label);
        self.runs
            .insert((canvas, label.to_string()), Run { running, at: now });

        safe
    }

    /// A tab that closes while its cards had gone silent leaves them stuck.
    pub fn closed(&mut self, label: &str, now: i64) {
        let mut stuck = Vec::new();
        self.runs.retain(|(canvas, l), run| {
            if l != label {
                return true;
            }
            if run.running && now - run.at > CLOSE_STALE_MS {
                stuck.push(*canvas);
            }
            false
        });
        self.remember(stuck);
    }

    /// Quitting judges every run as if its tab closed.
    pub fn quit(&mut self, now: i64) {
        let stuck = self
            .runs
            .drain()
            .filter(|(_, run)| run.running && now - run.at > CLOSE_STALE_MS)
            .map(|((canvas, _), _)| canvas)
            .collect();
        self.remember(stuck);
    }

    /// Canvases left stuck by an earlier launch.
    pub fn restore(&mut self, stuck: impl IntoIterator<Item = CanvasId>) {
        self.stuck.extend(stuck.into_iter().take(MAX_TRACKED));
    }

    pub fn stuck_canvases(&self) -> Vec<CanvasId> {
        self.stuck.iter().copied().collect()
    }

    /// Whether the stuck set changed since the last call, so it needs saving.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    fn remember(&mut self, stuck: Vec<CanvasId>) {
        for canvas in stuck {
            if self.stuck.len() < MAX_TRACKED {
                self.changed |= self.stuck.insert(canvas);
            }
        }
    }

    fn stuck(&self, canvas: CanvasId, label: &str, now: i64) -> bool {
        self.runs.iter().any(|((c, l), run)| {
            *c == canvas && run.running && (now - run.at > STALE_MS || l == label)
        })
    }

    fn make_room(&mut self, canvas: CanvasId, label: &str) {
        if self.runs.len() < MAX_TRACKED || self.runs.contains_key(&(canvas, label.to_string())) {
            return;
        }

        let oldest = self
            .runs
            .iter()
            .min_by_key(|(_, run)| (run.running, run.at))
            .map(|(key, _)| key.clone());
        if let Some(key) = oldest {
            self.runs.remove(&key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "tab-a";
    const B: &str = "tab-b";

    #[test]
    fn a_first_start_runs() {
        let mut health = CardHealth::default();
        assert!(!health.pulse(CanvasId::new(), A, Pulse::Start, 0));
    }

    #[test]
    fn a_tab_closed_soon_after_freezing_reopens_safe() {
        let mut health = CardHealth::default();
        let canvas = CanvasId::new();
        health.pulse(canvas, A, Pulse::Start, 0);
        health.pulse(canvas, A, Pulse::Beat, 2_000);
        health.closed(A, 8_000);
        assert!(health.take_changed());
        assert!(health.pulse(canvas, B, Pulse::Start, 9_000));
        assert!(health.take_changed(), "consuming it needs saving too");
    }

    #[test]
    fn quitting_while_frozen_carries_over_to_the_next_launch() {
        let mut health = CardHealth::default();
        let (frozen, fine) = (CanvasId::new(), CanvasId::new());
        health.pulse(frozen, A, Pulse::Start, 0);
        health.pulse(fine, B, Pulse::Start, 0);
        health.pulse(fine, B, Pulse::Beat, 7_000);
        health.quit(8_000);
        let mut next = CardHealth::default();
        next.restore(health.stuck_canvases());
        assert!(next.pulse(frozen, A, Pulse::Start, 0));
        assert!(!next.pulse(fine, B, Pulse::Start, 0));
    }

    #[test]
    fn a_tab_closed_while_answering_reopens_normally() {
        let mut health = CardHealth::default();
        let canvas = CanvasId::new();
        health.pulse(canvas, A, Pulse::Start, 0);
        health.pulse(canvas, A, Pulse::Beat, 2_000);
        health.closed(A, 3_000);
        assert!(!health.pulse(canvas, B, Pulse::Start, 60_000));
    }

    #[test]
    fn a_tab_closed_while_stuck_reopens_safe_once() {
        let mut health = CardHealth::default();
        let canvas = CanvasId::new();
        health.pulse(canvas, A, Pulse::Start, 0);
        health.closed(A, 30_000);
        assert!(health.pulse(canvas, B, Pulse::Start, 40_000));
        assert!(!health.pulse(canvas, B, Pulse::Start, 41_000), "running them anyway is allowed");
    }

    #[test]
    fn a_page_that_restarts_without_stopping_starts_safe() {
        let mut health = CardHealth::default();
        let canvas = CanvasId::new();
        health.pulse(canvas, A, Pulse::Start, 0);
        assert!(health.pulse(canvas, A, Pulse::Start, 1_000));
    }

    #[test]
    fn stopping_and_other_tabs_do_not_count() {
        let mut health = CardHealth::default();
        let canvas = CanvasId::new();
        health.pulse(canvas, A, Pulse::Start, 0);
        assert!(!health.pulse(canvas, B, Pulse::Start, 1_000), "the same canvas in another tab");
        health.pulse(canvas, A, Pulse::Stop, 2_000);
        assert!(!health.pulse(canvas, A, Pulse::Start, 3_000));
    }

    #[test]
    fn a_late_beat_after_sleep_is_not_judged() {
        let mut health = CardHealth::default();
        let canvas = CanvasId::new();
        health.pulse(canvas, A, Pulse::Start, 0);
        assert!(!health.pulse(canvas, A, Pulse::Beat, 600_000));
    }

    #[test]
    fn tracking_stays_bounded() {
        let mut health = CardHealth::default();
        for i in 0..(MAX_TRACKED as i64 + 50) {
            health.pulse(CanvasId::new(), A, Pulse::Stop, i);
        }
        assert!(health.runs.len() <= MAX_TRACKED);
    }
}
