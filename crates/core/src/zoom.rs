//! Page zoom: the steps a keypress moves through, and which zoom a host gets.
//!
//! Zoom moves along a fixed ladder of percentages. Percentages are whole
//! numbers, and no value can be NaN. The per-site factor is derived at the
//! point of use.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::user_agent::normalize_host;

/// Every zoom the browser will apply, ascending mirroring Firefox's range.
///
/// The far ends can be narrowed by the engine rather than by this list.
/// WebView2 clamps its own zoom factor, so the topmost rungs may land short
/// on Windows.
pub const ZOOM_LADDER: [u16; 26] = [
    20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120, 130, 140, 150, 160, 170, 180, 190, 200, 220,
    240, 260, 280, 300, 400, 500,
];

/// The ends of the ladder.
pub const MIN_ZOOM_PERCENT: u16 = ZOOM_LADDER[0];
pub const MAX_ZOOM_PERCENT: u16 = ZOOM_LADDER[ZOOM_LADDER.len() - 1];
/// Unzoomed, and the fallback whenever a stored value is unusable.
pub const DEFAULT_ZOOM_PERCENT: u16 = 100;

/// What a zoom keypress or menu item asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ZoomStep {
    In,
    Out,
    /// Back to the default, which also drops the site's own zoom.
    Reset,
}

/// The rung nearest `percent`. Ties go to the smaller rung.
pub fn sane_zoom_percent(percent: u16) -> u16 {
    ZOOM_LADDER
        .into_iter()
        .min_by_key(|rung| rung.abs_diff(percent))
        .unwrap_or(DEFAULT_ZOOM_PERCENT)
}

/// The engine-facing factor for a percentage (100 -> 1.0).
pub fn zoom_factor(percent: u16) -> f64 {
    f64::from(sane_zoom_percent(percent)) / 100.0
}

/// Where `rung` sits on the ladder.
fn ladder_index(rung: u16) -> usize {
    ZOOM_LADDER
        .iter()
        .position(|candidate| *candidate == rung)
        .unwrap_or_else(|| {
            ZOOM_LADDER
                .iter()
                .position(|candidate| *candidate == DEFAULT_ZOOM_PERCENT)
                .unwrap_or(0)
        })
}

/// The zoom after one step from `current`. `Reset` returns `default`.
pub fn stepped_zoom_percent(current: u16, step: ZoomStep, default: u16) -> u16 {
    let index = ladder_index(sane_zoom_percent(current));
    match step {
        ZoomStep::In => ZOOM_LADDER[(index + 1).min(ZOOM_LADDER.len() - 1)],
        ZoomStep::Out => ZOOM_LADDER[index.saturating_sub(1)],
        ZoomStep::Reset => sane_zoom_percent(default),
    }
}

/// The zoom for `host`: its own percentage when set, else `default`.
pub fn zoom_percent_for_host(host: &str, overrides: &HashMap<String, u16>, default: u16) -> u16 {
    overrides
        .get(&normalize_host(host))
        .copied()
        .map(sane_zoom_percent)
        .unwrap_or_else(|| sane_zoom_percent(default))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stepping_in_then_out_returns_to_the_same_rung_everywhere() {
        // The property the old multiply-and-divide arithmetic silently broke:
        // it held near 100% but failed once a press reached a bound, leaving
        // no way to key back to 100%.
        for rung in ZOOM_LADDER {
            if rung != MAX_ZOOM_PERCENT {
                let up = stepped_zoom_percent(rung, ZoomStep::In, DEFAULT_ZOOM_PERCENT);
                assert!(up > rung, "in did not enlarge {rung}");
                assert_eq!(
                    stepped_zoom_percent(up, ZoomStep::Out, DEFAULT_ZOOM_PERCENT),
                    rung,
                    "in then out drifted from {rung}"
                );
            }
            if rung != MIN_ZOOM_PERCENT {
                let down = stepped_zoom_percent(rung, ZoomStep::Out, DEFAULT_ZOOM_PERCENT);
                assert!(down < rung, "out did not shrink {rung}");
                assert_eq!(
                    stepped_zoom_percent(down, ZoomStep::In, DEFAULT_ZOOM_PERCENT),
                    rung,
                    "out then in drifted from {rung}"
                );
            }
        }
    }

    #[test]
    fn the_way_back_from_a_bound_passes_through_the_default() {
        // Hold zoom out to the floor, then key back up: every rung climbed
        // must be one that was descended, and 100% must be among them.
        let mut percent = DEFAULT_ZOOM_PERCENT;
        for _ in 0..ZOOM_LADDER.len() * 2 {
            percent = stepped_zoom_percent(percent, ZoomStep::Out, DEFAULT_ZOOM_PERCENT);
        }
        assert_eq!(percent, MIN_ZOOM_PERCENT);

        let mut climbed = Vec::new();
        for _ in 0..ZOOM_LADDER.len() * 2 {
            percent = stepped_zoom_percent(percent, ZoomStep::In, DEFAULT_ZOOM_PERCENT);
            climbed.push(percent);
        }
        assert_eq!(percent, MAX_ZOOM_PERCENT);
        assert!(
            climbed.contains(&DEFAULT_ZOOM_PERCENT),
            "cannot key back to 100%: {climbed:?}"
        );
    }

    #[test]
    fn resetting_returns_the_default_not_one_hundred() {
        assert_eq!(stepped_zoom_percent(200, ZoomStep::Reset, 120), 120);
    }

    #[test]
    fn an_off_ladder_or_out_of_range_value_snaps_to_a_rung() {
        // Values an older release stored, and hand-edited nonsense.
        assert_eq!(sane_zoom_percent(125), 120);
        assert_eq!(sane_zoom_percent(133), 130);
        assert_eq!(sane_zoom_percent(0), MIN_ZOOM_PERCENT);
        assert_eq!(sane_zoom_percent(60_000), MAX_ZOOM_PERCENT);
        assert_eq!(
            stepped_zoom_percent(60_000, ZoomStep::In, 100),
            MAX_ZOOM_PERCENT
        );
        assert_eq!(
            stepped_zoom_percent(1, ZoomStep::Out, 100),
            MIN_ZOOM_PERCENT
        );
        assert_eq!(
            stepped_zoom_percent(150, ZoomStep::Reset, 9_000),
            MAX_ZOOM_PERCENT
        );
    }

    #[test]
    fn the_ladder_matches_the_asked_for_increments() {
        let expected: Vec<u16> = (20..=200)
            .step_by(10)
            .chain((220..=300).step_by(20))
            .chain([400, 500])
            .collect();
        assert_eq!(ZOOM_LADDER.to_vec(), expected);
    }

    #[test]
    fn the_ladder_is_ascending_and_holds_the_default() {
        assert!(ZOOM_LADDER.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(ZOOM_LADDER.contains(&DEFAULT_ZOOM_PERCENT));
    }

    #[test]
    fn a_host_uses_its_own_zoom_when_it_has_one() {
        let overrides = HashMap::from([("docs.example".to_string(), 120)]);
        assert_eq!(
            zoom_percent_for_host("WWW.Docs.Example", &overrides, 100),
            120
        );
        assert_eq!(zoom_percent_for_host("other.example", &overrides, 110), 110);
    }

    #[test]
    fn a_stored_zoom_out_of_range_falls_back_to_the_bounds() {
        let overrides = HashMap::from([("huge.example".to_string(), 9_000)]);
        assert_eq!(
            zoom_percent_for_host("huge.example", &overrides, 100),
            MAX_ZOOM_PERCENT
        );
        assert_eq!(
            zoom_percent_for_host("none.example", &overrides, 0),
            MIN_ZOOM_PERCENT
        );
    }

    #[test]
    fn the_factor_is_the_percentage_over_one_hundred() {
        assert_eq!(zoom_factor(100), 1.0);
        assert_eq!(zoom_factor(120), 1.2);
        assert_eq!(zoom_factor(9_000), 5.0);
    }

    #[test]
    fn steps_deserialize_from_the_names_the_chrome_sends() {
        assert_eq!(
            serde_json::from_str::<ZoomStep>("\"in\"").unwrap(),
            ZoomStep::In
        );
        assert_eq!(
            serde_json::from_str::<ZoomStep>("\"out\"").unwrap(),
            ZoomStep::Out
        );
        assert_eq!(
            serde_json::from_str::<ZoomStep>("\"reset\"").unwrap(),
            ZoomStep::Reset
        );
    }
}
