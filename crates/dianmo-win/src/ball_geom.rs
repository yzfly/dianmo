//! Geometry of the floating ball (`handle.rs`), kept free of Win32 types so it builds and is
//! unit-tested on every platform.
//!
//! Windows starts its edge gestures (left = Task View / Timeline, right = Action Center, top =
//! drag-to-close in tablet mode) from contacts that begin within a few mm of the screen edge. On
//! the Surface (2880x1920, 200 %) 1 DIP ≈ 0.19 mm, so that band is roughly the outer 10–20 DIP.
//! The ball therefore never touches the edge of its work area:
//!
//! - **Out** (normal): the 48 DIP ball, its outer rim [`EDGE_GAP_DIP`] (10 DIP) from the edge, so
//!   its centre is 34 DIP in.
//! - **Tucked** (idle 3 s): instead of half the ball sliding off-screen (the visible half then
//!   starts at 0 px and every grab of it is an edge swipe), it shrinks to a translucent 32 DIP
//!   ball, still 10 DIP clear of the edge: footprint 10–42 DIP, centre 26 DIP in.
//!
//! The work area's edge is never outside the screen's, so a gap from the work area is at least as
//! large from the screen (taskbar on that side only adds to it).
//!
//! Everything here is in physical pixels unless the name says DIP; `scale` is DPI / 96.

#![cfg_attr(not(windows), allow(dead_code))]

/// Screen edge the ball sits on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BallEdge {
    Left,
    Right,
}

/// Diameter of the ball when it is out.
pub(crate) const DIAMETER_DIP: f32 = 48.0;
/// Diameter of the tucked (idle) ball.
pub(crate) const TUCKED_DIAMETER_DIP: f32 = 32.0;
/// Room around the ball for the shadow and the listening halo.
pub(crate) const MARGIN_DIP: f32 = 14.0;
/// Room around the tucked ball for its shadow (it never shows the halo).
pub(crate) const TUCKED_MARGIN_DIP: f32 = 8.0;
/// Gap between the ball's rim and the edge of the work area, out or tucked: keeps the ball out of
/// the system edge-gesture band.
pub(crate) const EDGE_GAP_DIP: f32 = 10.0;

/// A rectangle in physical pixels (same fields as Win32 `RECT`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// Where and how big the ball's window is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Layout {
    /// Top-left of the (square, layered) window.
    pub x: i32,
    pub y: i32,
    /// Side of the window: the ball plus its margin on each side.
    pub size: i32,
    /// Ball radius.
    pub radius: f32,
    /// Ball centre relative to the window origin (the margin plus the radius).
    pub center_off: f32,
}

/// Window size, ball radius and margin (px) for a ball of `diameter_dip` with `margin_dip` around
/// it. The ball is centred in the window.
pub(crate) fn window_size(diameter_dip: f32, margin_dip: f32, scale: f32) -> (i32, f32, i32) {
    let r = diameter_dip * scale / 2.0;
    let m = (margin_dip * scale).round() as i32;
    ((2.0 * r).ceil() as i32 + 2 * m, r, m)
}

/// Window size, ball radius and margin for the out (`tucked == false`) or tucked ball.
pub(crate) fn ball_size(tucked: bool, scale: f32) -> (i32, f32, i32) {
    if tucked {
        window_size(TUCKED_DIAMETER_DIP, TUCKED_MARGIN_DIP, scale)
    } else {
        window_size(DIAMETER_DIP, MARGIN_DIP, scale)
    }
}

/// The ball's window on `edge` of `work` with its centre at `y_frac` of the work area's height
/// (clamped so the ball keeps the gap from the top and bottom too).
pub(crate) fn layout(work: Rect, scale: f32, edge: BallEdge, y_frac: f32, tucked: bool) -> Layout {
    let (size, r, m) = ball_size(tucked, scale);
    let gap = EDGE_GAP_DIP * scale;
    let h = (work.bottom - work.top) as f32;
    let (top, bottom) = (work.top as f32 + r + gap, work.bottom as f32 - r - gap);
    let cy = (work.top as f32 + y_frac * h).clamp(top, bottom.max(top));
    let cx = match edge {
        BallEdge::Left => work.left as f32 + gap + r,
        BallEdge::Right => work.right as f32 - gap - r,
    };
    Layout {
        x: (cx - r).round() as i32 - m,
        y: (cy - r).round() as i32 - m,
        size,
        radius: r,
        center_off: m as f32 + r,
    }
}

/// Where a ball dropped with its centre at (`cx`, `cy`) snaps to: the nearer left/right edge and
/// the centre's height as a fraction of the work area.
pub(crate) fn snap(work: Rect, cx: f32, cy: f32) -> (BallEdge, f32) {
    let mid = (work.left + work.right) as f32 / 2.0;
    let edge = if cx < mid { BallEdge::Left } else { BallEdge::Right };
    let h = (work.bottom - work.top).max(1) as f32;
    (edge, ((cy - work.top as f32) / h).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Surface Pro: 2880x1920 at 200 %, taskbar at the bottom.
    const WORK: Rect = Rect { left: 0, top: 0, right: 2880, bottom: 1840 };
    const SCALE: f32 = 2.0;
    /// Upper end of the band Windows treats as an edge swipe (≈ 4 mm on the Surface).
    const GESTURE_BAND_DIP: f32 = 20.0;

    /// The ball's horizontal extent (left rim, centre, right rim) on screen.
    fn ball_x(l: &Layout) -> (f32, f32, f32) {
        let c = l.x as f32 + l.center_off;
        (c - l.radius, c, c + l.radius)
    }

    /// Distance of the ball's rim and centre from the screen edge it sits on, in DIP.
    fn clearance(work: Rect, scale: f32, edge: BallEdge, tucked: bool) -> (f32, f32) {
        let l = layout(work, scale, edge, 0.5, tucked);
        let (lo, c, hi) = ball_x(&l);
        match edge {
            BallEdge::Left => ((lo - work.left as f32) / scale, (c - work.left as f32) / scale),
            BallEdge::Right => ((work.right as f32 - hi) / scale, (work.right as f32 - c) / scale),
        }
    }

    #[test]
    fn sizes_at_200_percent() {
        assert_eq!(ball_size(false, SCALE), (96 + 2 * 28, 48.0, 28));
        assert_eq!(ball_size(true, SCALE), (64 + 2 * 16, 32.0, 16));
    }

    #[test]
    fn tucked_ball_stays_out_of_the_edge_gesture_band() {
        for edge in [BallEdge::Left, BallEdge::Right] {
            let (rim, centre) = clearance(WORK, SCALE, edge, true);
            assert!((rim - EDGE_GAP_DIP).abs() < 0.6, "{edge:?} rim {rim}");
            assert!((centre - (EDGE_GAP_DIP + TUCKED_DIAMETER_DIP / 2.0)).abs() < 0.6, "{edge:?} centre {centre}");
            assert!(centre > GESTURE_BAND_DIP, "{edge:?}: centre {centre} DIP is inside the gesture band");
        }
    }

    #[test]
    fn out_ball_keeps_the_gap_too() {
        for edge in [BallEdge::Left, BallEdge::Right] {
            let (rim, centre) = clearance(WORK, SCALE, edge, false);
            assert!((rim - EDGE_GAP_DIP).abs() < 0.6, "{edge:?} rim {rim}");
            assert!((centre - 34.0).abs() < 0.6, "{edge:?} centre {centre}");
        }
    }

    #[test]
    fn ball_is_entirely_on_screen() {
        for scale in [1.0, 1.25, 1.5, 2.0, 2.5, 3.0] {
            for edge in [BallEdge::Left, BallEdge::Right] {
                for tucked in [false, true] {
                    for y in [0.0, 0.62, 1.0] {
                        let l = layout(WORK, scale, edge, y, tucked);
                        let (lo, _, hi) = ball_x(&l);
                        let cy = l.y as f32 + l.center_off;
                        let gap = EDGE_GAP_DIP * scale - 1.0;
                        assert!(lo >= WORK.left as f32 + gap && hi <= WORK.right as f32 - gap, "{scale} {edge:?} {tucked}");
                        assert!(cy - l.radius >= WORK.top as f32 + gap, "{scale} {edge:?} {tucked} {y}");
                        assert!(cy + l.radius <= WORK.bottom as f32 - gap, "{scale} {edge:?} {tucked} {y}");
                    }
                }
            }
        }
    }

    #[test]
    fn tucked_ball_keeps_its_height_and_shrinks_toward_the_edge() {
        let out = layout(WORK, SCALE, BallEdge::Left, 0.62, false);
        let tucked = layout(WORK, SCALE, BallEdge::Left, 0.62, true);
        assert!((out.y as f32 + out.center_off - (tucked.y as f32 + tucked.center_off)).abs() <= 1.0);
        assert!(ball_x(&tucked).1 < ball_x(&out).1);
        assert!(tucked.size < out.size);
        // The tucked window (shadow margin included) doesn't hang off the screen.
        assert!(tucked.x >= WORK.left);
    }

    #[test]
    fn second_monitor_with_negative_coordinates() {
        // A 1920x1080 monitor at 125 % to the left of the primary one.
        let work = Rect { left: -1920, top: 0, right: 0, bottom: 1040 };
        let l = layout(work, 1.25, BallEdge::Right, 0.5, true);
        let (_, _, hi) = ball_x(&l);
        assert!((work.right as f32 - hi - EDGE_GAP_DIP * 1.25).abs() < 1.0);
        let l = layout(work, 1.25, BallEdge::Left, 0.5, true);
        let (lo, _, _) = ball_x(&l);
        assert!((lo - work.left as f32 - EDGE_GAP_DIP * 1.25).abs() < 1.0);
    }

    #[test]
    fn tiny_work_area_does_not_panic() {
        let work = Rect { left: 0, top: 0, right: 40, bottom: 40 };
        let l = layout(work, 2.0, BallEdge::Left, 0.5, false);
        assert!(l.size > 0);
    }

    #[test]
    fn snap_picks_the_nearer_edge_and_clamps_the_height() {
        assert_eq!(snap(WORK, 100.0, 920.0), (BallEdge::Left, 0.5));
        assert_eq!(snap(WORK, 2800.0, -50.0), (BallEdge::Right, 0.0));
        assert_eq!(snap(WORK, 1500.0, 5000.0), (BallEdge::Right, 1.0));
    }

    #[test]
    fn snap_round_trips_through_layout() {
        for tucked in [false, true] {
            let l = layout(WORK, SCALE, BallEdge::Right, 0.3, tucked);
            let (edge, y) = snap(WORK, l.x as f32 + l.center_off, l.y as f32 + l.center_off);
            assert_eq!(edge, BallEdge::Right);
            assert!((y - 0.3).abs() < 0.002, "{y}");
        }
    }
}
