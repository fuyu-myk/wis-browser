//! Pure popout-window geometry (screen coordinates, logical pixels): the
//! default/initial frame, aspect clamping, and aspect-locked corner resizes.

pub const MIN_WIDTH: f64 = 240.0;
const DEFAULT_WIDTH: f64 = 440.0;
/// Breathing room kept between the popout and the screen work-area edges.
const WORK_MARGIN: f64 = 12.0;
/// Default docking inset from the main window's bottom-right corner.
const DOCK_MARGIN: f64 = 24.0;
/// Per-message resize delta cap; anything larger is a bogus payload.
pub(crate) const MAX_DELTA: f64 = 4000.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    NorthWest,
    NorthEast,
    SouthWest,
    SouthEast,
}

impl Corner {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "nw" => Some(Self::NorthWest),
            "ne" => Some(Self::NorthEast),
            "sw" => Some(Self::SouthWest),
            "se" => Some(Self::SouthEast),
            _ => None,
        }
    }

    fn east(self) -> bool {
        matches!(self, Self::NorthEast | Self::SouthEast)
    }

    fn south(self) -> bool {
        matches!(self, Self::SouthWest | Self::SouthEast)
    }
}

/// Sane width/height ratio; garbage collapses to 16:9.
pub fn clamp_aspect(aspect: f64) -> f64 {
    if !aspect.is_finite() || aspect <= 0.0 {
        16.0 / 9.0
    } else {
        aspect.clamp(0.2, 5.0)
    }
}

fn max_width_for(aspect: f64, work: Frame) -> f64 {
    let by_width = work.w - 2.0 * WORK_MARGIN;
    let by_height = (work.h - 2.0 * WORK_MARGIN) * aspect;
    by_width.min(by_height).max(MIN_WIDTH)
}

fn clamp_axis(v: f64, lo: f64, hi: f64) -> f64 {
    if hi < lo {
        lo
    } else {
        v.clamp(lo, hi)
    }
}

/// First frame of a fresh popout: always the predefined default size (only
/// the height varies, derived from the video's aspect), at the remembered
/// position when it still fits the work area, else docked at the main
/// window's bottom-right corner.
pub fn initial_frame(
    aspect: f64,
    main: Frame,
    work: Frame,
    remembered_pos: Option<(f64, f64)>,
) -> Frame {
    let aspect = clamp_aspect(aspect);
    let max_w = max_width_for(aspect, work);
    let w = DEFAULT_WIDTH.clamp(MIN_WIDTH, max_w);
    let h = w / aspect;
    let (x, y) = match remembered_pos {
        Some((x, y)) if x.is_finite() && y.is_finite() => (x, y),
        _ => (
            main.x + main.w - w - DOCK_MARGIN,
            main.y + main.h - h - DOCK_MARGIN,
        ),
    };

    Frame {
        x: clamp_axis(x, work.x + WORK_MARGIN, work.x + work.w - w - WORK_MARGIN),
        y: clamp_axis(y, work.y + WORK_MARGIN, work.y + work.h - h - WORK_MARGIN),
        w,
        h,
    }
}

/// Aspect-locked corner resize: the dragged corner follows the dominant axis
/// of the delta while the opposite corner stays anchored.
pub fn resized_frame(
    frame: Frame,
    corner: Corner,
    dx: f64,
    dy: f64,
    aspect: f64,
    work: Frame,
) -> Frame {
    let aspect = clamp_aspect(aspect);
    let dx = if dx.is_finite() {
        dx.clamp(-MAX_DELTA, MAX_DELTA)
    } else {
        0.0
    };
    let dy = if dy.is_finite() {
        dy.clamp(-MAX_DELTA, MAX_DELTA)
    } else {
        0.0
    };
    let dw_horizontal = if corner.east() { dx } else { -dx };
    let dw_vertical = (if corner.south() { dy } else { -dy }) * aspect;
    let dw = if dw_horizontal.abs() >= dw_vertical.abs() {
        dw_horizontal
    } else {
        dw_vertical
    };

    let w = (frame.w + dw).clamp(MIN_WIDTH, max_width_for(aspect, work));
    let h = w / aspect;

    Frame {
        x: if corner.east() {
            frame.x
        } else {
            frame.x + (frame.w - w)
        },
        y: if corner.south() {
            frame.y
        } else {
            frame.y + (frame.h - h)
        },
        w,
        h,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Frame = Frame {
        x: 0.0,
        y: 0.0,
        w: 1728.0,
        h: 1080.0,
    };
    const MAIN: Frame = Frame {
        x: 100.0,
        y: 60.0,
        w: 1280.0,
        h: 820.0,
    };

    #[test]
    fn clamp_aspect_rejects_garbage() {
        assert_eq!(clamp_aspect(f64::NAN), 16.0 / 9.0);
        assert_eq!(clamp_aspect(0.0), 16.0 / 9.0);
        assert_eq!(clamp_aspect(-2.0), 16.0 / 9.0);
        assert_eq!(clamp_aspect(100.0), 5.0);
        assert_eq!(clamp_aspect(0.01), 0.2);
        assert_eq!(clamp_aspect(1.5), 1.5);
    }

    #[test]
    fn initial_frame_docks_bottom_right_of_main() {
        let f = initial_frame(16.0 / 9.0, MAIN, WORK, None);
        assert_eq!(f.w, 440.0);
        assert!((f.h - 440.0 * 9.0 / 16.0).abs() < 0.001);
        assert!((f.x - (MAIN.x + MAIN.w - f.w - 24.0)).abs() < 0.001);
        assert!((f.y - (MAIN.y + MAIN.h - f.h - 24.0)).abs() < 0.001);
    }

    #[test]
    fn initial_frame_always_starts_at_the_default_width() {
        // A remembered position never changes the predefined starting size.
        let f = initial_frame(16.0 / 9.0, MAIN, WORK, Some((300.0, 200.0)));
        assert_eq!(f.w, 440.0);
        assert_eq!((f.x, f.y), (300.0, 200.0));
    }

    #[test]
    fn initial_frame_clamps_remembered_position_into_work_area() {
        let f = initial_frame(16.0 / 9.0, MAIN, WORK, Some((-500.0, 99999.0)));
        assert!(f.x >= WORK.x && f.x + f.w <= WORK.x + WORK.w);
        assert!(f.y >= WORK.y && f.y + f.h <= WORK.y + WORK.h);
        assert!((f.h - f.w / (16.0 / 9.0)).abs() < 0.001);
    }

    #[test]
    fn initial_frame_caps_width_by_work_height() {
        // A short work area cannot hold the default width at 16:9; the
        // width must shrink so the height fits.
        let short = Frame {
            x: 0.0,
            y: 0.0,
            w: 800.0,
            h: 200.0,
        };
        let f = initial_frame(16.0 / 9.0, MAIN, short, None);
        assert!(f.w < 440.0);
        assert!(f.h <= short.h);
        assert!((f.h - f.w / (16.0 / 9.0)).abs() < 0.001);
    }

    #[test]
    fn resize_se_keeps_origin_anchored() {
        let frame = Frame {
            x: 200.0,
            y: 150.0,
            w: 480.0,
            h: 270.0,
        };
        let next = resized_frame(frame, Corner::SouthEast, 160.0, 10.0, 16.0 / 9.0, WORK);
        assert_eq!(next.x, frame.x);
        assert_eq!(next.y, frame.y);
        assert_eq!(next.w, 640.0);
        assert!((next.h - 360.0).abs() < 0.001);
    }

    #[test]
    fn resize_nw_keeps_bottom_right_anchored() {
        let frame = Frame {
            x: 400.0,
            y: 300.0,
            w: 480.0,
            h: 270.0,
        };
        let next = resized_frame(frame, Corner::NorthWest, -160.0, -10.0, 16.0 / 9.0, WORK);
        assert!((next.x + next.w - (frame.x + frame.w)).abs() < 0.001);
        assert!((next.y + next.h - (frame.y + frame.h)).abs() < 0.001);
        assert_eq!(next.w, 640.0);
    }

    #[test]
    fn resize_clamps_to_min_width() {
        let frame = Frame {
            x: 200.0,
            y: 150.0,
            w: 300.0,
            h: 168.75,
        };
        let next = resized_frame(frame, Corner::SouthEast, -5000.0, 0.0, 16.0 / 9.0, WORK);
        assert_eq!(next.w, MIN_WIDTH);
        assert!((next.h - MIN_WIDTH / (16.0 / 9.0)).abs() < 0.001);
    }

    #[test]
    fn resize_follows_dominant_axis() {
        let frame = Frame {
            x: 200.0,
            y: 150.0,
            w: 480.0,
            h: 270.0,
        };
        // Vertical delta dominates: width grows by dy * aspect.
        let next = resized_frame(frame, Corner::SouthEast, 10.0, 90.0, 16.0 / 9.0, WORK);
        assert!((next.w - (480.0 + 90.0 * 16.0 / 9.0)).abs() < 0.001);
    }

    #[test]
    fn zero_delta_resize_relocks_height_to_new_aspect() {
        // The aspect-changed path: same width and anchor, height re-derived.
        let frame = Frame {
            x: 200.0,
            y: 150.0,
            w: 480.0,
            h: 270.0,
        };
        let next = resized_frame(frame, Corner::SouthEast, 0.0, 0.0, 4.0 / 3.0, WORK);
        assert_eq!(next.x, frame.x);
        assert_eq!(next.y, frame.y);
        assert_eq!(next.w, frame.w);
        assert!((next.h - 480.0 / (4.0 / 3.0)).abs() < 0.001);
    }

    #[test]
    fn corner_parse_rejects_unknown() {
        assert_eq!(Corner::parse("nw"), Some(Corner::NorthWest));
        assert_eq!(Corner::parse("se"), Some(Corner::SouthEast));
        assert_eq!(Corner::parse("xx"), None);
        assert_eq!(Corner::parse(""), None);
    }
}
