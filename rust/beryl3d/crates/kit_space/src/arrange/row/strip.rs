use glam::{Vec2, Vec3};

use crate::camera::Camera;

/// Find the narrowest strip whose projection contains a screen-aligned press square
/// at either end of the row. Use the shortest item's depth so the same strip works
/// for every item in the row.
pub(super) fn least_strip(
    camera: &Camera,
    [left, right]: [f32; 2],
    y: f32,
    depth: f32,
    side: f32,
) -> f32 {
    if side <= 0.0 {
        return 0.0;
    }
    let flat = side / camera.points_per_millimetre(Vec3::new(0.0, y, 0.0));
    let holds = |strip: f32| {
        [left, right - strip]
            .into_iter()
            .all(|left| holds_square(camera, (left, left + strip), y, depth, side))
    };
    // Every strip at least `flat` and too narrow to hold the square is narrower than every
    // strip that holds it, so a bisection finds the least to within a hundredth of a millimetre.
    let (mut low, mut high) = (flat, flat);
    while !holds(high) {
        low = high;
        high *= 2.0;
        if high > right - left {
            return high;
        }
    }
    while high - low > 0.01 {
        let middle = f32::midpoint(low, high);
        if holds(middle) {
            high = middle;
        } else {
            low = middle;
        }
    }
    high
}

/// Whether the strip between `left` and `right`, across the shortest item's depth
/// centred on `y`, holds a square `side` points on a side on screen. Each edge of the part is a
/// straight line on screen, and points at one height of the table share a height on screen, so
/// the width a band of the screen holds is the least gap between the edges at its top and its
/// bottom.
fn holds_square(camera: &Camera, (left, right): (f32, f32), y: f32, depth: f32, side: f32) -> bool {
    let point = |x: f32, along: f32| camera.to_screen(Vec3::new(x, y + along, 0.0));
    let half = depth / 2.0;
    let (near_left, far_left) = (point(left, -half), point(left, half));
    let (near_right, far_right) = (point(right, -half), point(right, half));
    let (top, bottom) = (far_left.y.min(near_left.y), far_left.y.max(near_left.y));
    if bottom - top < side {
        return false;
    }
    // Where the line from `from` to `to` crosses height `at` of the screen.
    let across =
        |from: Vec2, to: Vec2, at: f32| from.x + (to.x - from.x) * (at - from.y) / (to.y - from.y);
    let gap = |start: f32| {
        let end = start + side;
        let left = across(near_left, far_left, start).max(across(near_left, far_left, end));
        let right = across(near_right, far_right, start).min(across(near_right, far_right, end));
        right - left
    };
    (0..=SQUARE_BANDS)
        .map(|band| top + (bottom - top - side) * f32::from(band) / f32::from(SQUARE_BANDS))
        .any(|start| gap(start) >= side)
}

/// How many steps `holds_square` tries a band of the screen at, from the part's top to its
/// bottom.
const SQUARE_BANDS: u8 = 16;
