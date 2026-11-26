//! A cascade beside or over a fan (`CascadeFan`), as the put-back tray lays the pile cards go
//! onto and the hand. The generic interface checks cannot see this layout's fault: the pile's
//! card backs are drawn by the tray and are not elements of the frame, so nothing reports them
//! covering the pile's heading.

use glam::Vec2;
use kit_base::geometry::Rect;
use kit_space::arrange::screen::{CascadeFan, FanShape, ScreenFit};

/// A narrow window's put-back tray, in its own units: the pile's band over the opponent's
/// rows, the hand's band over the viewer's, and two places on the pile for cards put back.
fn stacked_put_back(base_edge: f32) -> (CascadeFan, ScreenFit, Rect, Rect) {
    let policy = CascadeFan {
        cascade_step: 0.165,
        cascade_turns: Vec2::new(-1.6, 1.4),
        cascade_rise: 0.05,
        base_edge,
        separation: 0.5,
        fan: FanShape {
            step: 0.65,
            drop: 0.0,
            turn: 0.0,
        },
        fan_min_step: 0.0,
        preferred_step: 52.0,
        heading: 18.0,
    };
    let fit = ScreenFit {
        aspect: 1.4,
        most: 104.0,
        least: 54.0,
        gap: 6.0,
        press: 44.0,
    };
    let band = Rect {
        x: 8.0,
        y: 96.0,
        width: 377.0,
        height: 200.0,
    };
    let second = Rect {
        x: 8.0,
        y: 381.0,
        width: 377.0,
        height: 140.0,
    };
    (policy, fit, band, second)
}

/// Stacked, the pile has a band to itself and fills it. The deck it is drawn as, its base raised
/// and its backs reaching `base_edge` higher, once covered the "Top" heading above it.
#[test]
fn a_stacked_pile_leaves_its_heading_uncovered() {
    for base_edge in [0.0, 7.5] {
        let (policy, fit, band, second) = stacked_put_back(base_edge);
        let paired = policy.fit(fit, band, Some(second), [0, 4], [2, 4]);
        let heading_foot = paired.bounds[0].y + policy.heading;
        let drawn_top = paired.base.y - base_edge;
        assert!(
            heading_foot <= drawn_top + 0.01,
            "with backs {base_edge} up, the heading ends at {heading_foot} but the pile is \
             drawn from {drawn_top}"
        );
        assert!(
            paired.bounds[0].y >= band.y,
            "the heading stays in its band"
        );
        let foot = paired.slots[paired.slots.len() - 1];
        assert!(
            foot.y + foot.height <= band.y + band.height + 0.01,
            "the pile stays in its band"
        );
    }
}
