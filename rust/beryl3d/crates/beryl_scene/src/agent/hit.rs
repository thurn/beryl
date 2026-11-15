//! Hit testing: which element a press at a point reaches, and where an element can be pressed
//! (docs/beryl-3d.md, "The test driver" and the "Reachable" rule of "Interface checks").
//!
//! An element's area is the quadrilateral of its four corners, cut to its clip. The flat layer
//! is drawn over the table without a depth test, so any element of the flat layer covers every
//! card. Within the flat layer a later element of the tree is drawn over an earlier one; on the
//! table a nearer element (smaller `depth`) covers a farther one. An arrow never catches a
//! press, and neither does an element with no opacity.

use glam::Vec2;

use crate::frame::{Frame, Place, Resolved, Role};
use crate::input::{Rect, Size};

/// The index of the element a press at `point` (in logical points) reaches, if any.
#[must_use]
pub fn hit(frame: &Frame, point: Vec2) -> Option<u32> {
    let mut best: Option<u32> = None;
    for (index, element) in frame.resolved.iter().enumerate() {
        if !catches(element, point) {
            continue;
        }
        let index = index as u32;
        best = match best {
            Some(other) if covers(frame, other, index) => Some(other),
            _ => Some(index),
        };
    }
    best
}

/// The largest rectangle, by area, inside element `index` where a press reaches that element
/// or one of its descendants. `None` when no part of it can be pressed.
#[must_use]
pub fn uncovered(frame: &Frame, index: u32) -> Option<Rect> {
    free_rect(frame, index, Size::default())
}

/// The largest rectangle, by area, at least `min` large, inside element `index` where a press
/// reaches that element or one of its descendants.
///
/// The element's bounding box is cut into cells along the edges of its own box and of every
/// box that overlaps it. A cell is free when a press at its centre and just inside each of its
/// corners reaches the element. The answer is the largest run of free cells that forms a
/// rectangle. A slanted edge that cuts a cell without reaching any of those five points is
/// missed, which can only make the answer slightly too large on a strongly rotated element.
#[must_use]
pub fn free_rect(frame: &Frame, index: u32, min: Size) -> Option<Rect> {
    let grid = Grid::new(frame, index);
    let free = grid.free_cells(frame, index);
    let (columns, rows) = (grid.xs.len() - 1, grid.ys.len() - 1);
    let mut best: Option<(f32, Rect)> = None;
    for left in 0..columns {
        // `open[row]` is whether every cell from `left` to `right` in that row is free.
        let mut open = vec![true; rows];
        for right in left..columns {
            let width = grid.xs[right + 1] - grid.xs[left];
            for (row, open) in open.iter_mut().enumerate() {
                *open &= free[row * columns + right];
            }
            if width < min.width {
                continue;
            }
            let mut top = 0;
            for (bottom, &open) in open.iter().enumerate() {
                if !open {
                    top = bottom + 1;
                    continue;
                }
                let height = grid.ys[bottom + 1] - grid.ys[top];
                if height < min.height {
                    continue;
                }
                let area = width * height;
                if best.is_none_or(|(most, _)| area > most) {
                    best = Some((
                        area,
                        Rect {
                            x: grid.xs[left],
                            y: grid.ys[top],
                            width,
                            height,
                        },
                    ));
                }
            }
        }
    }
    best.map(|(_, rect)| rect)
}

/// How much of element `index`'s area is covered by other elements, from 0 to 1, measured on
/// the same cells as `free_rect`.
#[must_use]
pub fn covered(frame: &Frame, index: u32) -> f32 {
    let grid = Grid::new(frame, index);
    let free = grid.free_cells(frame, index);
    let element = &frame.resolved[index as usize];
    let columns = grid.xs.len() - 1;
    let (mut inside, mut open) = (0.0, 0.0);
    for (row, pair) in grid.ys.windows(2).enumerate() {
        for (column, xs) in grid.xs.windows(2).enumerate() {
            let center = Vec2::new(f32::midpoint(xs[0], xs[1]), f32::midpoint(pair[0], pair[1]));
            if !within(element, center) {
                continue;
            }
            let area = (xs[1] - xs[0]) * (pair[1] - pair[0]);
            inside += area;
            if free[row * columns + column] {
                open += area;
            }
        }
    }
    if inside <= 0.0 {
        return 0.0;
    }
    1.0 - open / inside
}

/// The axis-aligned bounding box of an element's corners, in logical points.
#[must_use]
pub fn bounds(element: &Resolved) -> Rect {
    let min = element
        .corners
        .into_iter()
        .reduce(Vec2::min)
        .unwrap_or_default();
    let max = element
        .corners
        .into_iter()
        .reduce(Vec2::max)
        .unwrap_or_default();
    Rect {
        x: min.x,
        y: min.y,
        width: max.x - min.x,
        height: max.y - min.y,
    }
}

/// Whether `ancestor` is `index` or one of its ancestors in the tree.
#[must_use]
pub fn is_within(frame: &Frame, index: u32, ancestor: u32) -> bool {
    let mut at = index;
    while at != u32::MAX {
        if at == ancestor {
            return true;
        }
        at = frame.resolved[at as usize].parent;
    }
    false
}

/// Whether `inner` lies wholly inside `outer`, allowing for rounding.
#[must_use]
pub fn contains(outer: Rect, inner: Rect) -> bool {
    const SLACK: f32 = 0.01;
    inner.x >= outer.x - SLACK
        && inner.y >= outer.y - SLACK
        && inner.x + inner.width <= outer.x + outer.width + SLACK
        && inner.y + inner.height <= outer.y + outer.height + SLACK
}

/// The area two rectangles share, as a rectangle, or `None` when they only touch or are apart.
#[must_use]
pub fn intersection(a: Rect, b: Rect) -> Option<Rect> {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = (a.x + a.width).min(b.x + b.width);
    let bottom = (a.y + a.height).min(b.y + b.height);
    (right > x && bottom > y).then_some(Rect {
        x,
        y,
        width: right - x,
        height: bottom - y,
    })
}

/// Whether a press at `point` lands on `element`, ignoring what covers it.
fn catches(element: &Resolved, point: Vec2) -> bool {
    element.role != Role::Arrow && element.opacity > 0.0 && within(element, point)
}

/// Whether `point` is inside the element's quadrilateral and its clip.
fn within(element: &Resolved, point: Vec2) -> bool {
    if let Some(clip) = element.clip
        && !(point.x >= clip.x
            && point.y >= clip.y
            && point.x <= clip.x + clip.width
            && point.y <= clip.y + clip.height)
    {
        return false;
    }
    in_quad(element.corners, point)
}

/// Whether `point` is inside the convex quadrilateral `corners`, in either winding.
fn in_quad(corners: [Vec2; 4], point: Vec2) -> bool {
    // Which side of the edges the point has been on so far, if any.
    let mut side: Option<bool> = None;
    for i in 0..4 {
        let a = corners[i];
        let b = corners[(i + 1) % 4];
        let cross = (b - a).perp_dot(point - a);
        if cross.abs() <= f32::EPSILON {
            continue;
        }
        let left = cross > 0.0;
        if *side.get_or_insert(left) != left {
            return false;
        }
    }
    // A quadrilateral with no area holds no point.
    side.is_some()
}

/// Whether element `a` is drawn over element `b`.
fn covers(frame: &Frame, a: u32, b: u32) -> bool {
    let (first, second) = (&frame.resolved[a as usize], &frame.resolved[b as usize]);
    match (first.place == Place::Layer, second.place == Place::Layer) {
        (true, false) => true,
        (false, true) => false,
        // The flat layer is drawn in the order of its tree.
        (true, true) => a > b,
        (false, false) => first.depth.total_cmp(&second.depth).then(b.cmp(&a)).is_lt(),
    }
}

/// The cells `free_rect` and `covered` measure an element on.
struct Grid {
    xs: Vec<f32>,
    ys: Vec<f32>,
}

impl Grid {
    fn new(frame: &Frame, index: u32) -> Self {
        let element = &frame.resolved[index as usize];
        let mut area = bounds(element);
        if let Some(clip) = element.clip {
            area = intersection(area, clip).unwrap_or(Rect {
                x: area.x,
                y: area.y,
                width: 0.0,
                height: 0.0,
            });
        }
        let (mut xs, mut ys) = (
            vec![area.x, area.x + area.width],
            vec![area.y, area.y + area.height],
        );
        for (other, candidate) in frame.resolved.iter().enumerate() {
            if other as u32 == index || candidate.role == Role::Arrow {
                continue;
            }
            let Some(shared) = intersection(area, bounds(candidate)) else {
                continue;
            };
            xs.extend([shared.x, shared.x + shared.width]);
            ys.extend([shared.y, shared.y + shared.height]);
        }
        for edges in [&mut xs, &mut ys] {
            edges.sort_by(f32::total_cmp);
            edges.dedup_by(|a, b| (*a - *b).abs() < 0.001);
        }
        Self { xs, ys }
    }

    /// Whether each cell is free, row by row.
    fn free_cells(&self, frame: &Frame, index: u32) -> Vec<bool> {
        let reaches = |point: Vec2| hit(frame, point).is_some_and(|at| is_within(frame, at, index));
        let mut free = Vec::with_capacity((self.xs.len() - 1) * (self.ys.len() - 1));
        for ys in self.ys.windows(2) {
            for xs in self.xs.windows(2) {
                let inset = 0.01_f32
                    .min((xs[1] - xs[0]) / 4.0)
                    .min((ys[1] - ys[0]) / 4.0);
                let (left, right) = (xs[0] + inset, xs[1] - inset);
                let (top, bottom) = (ys[0] + inset, ys[1] - inset);
                let points = [
                    Vec2::new(f32::midpoint(xs[0], xs[1]), f32::midpoint(ys[0], ys[1])),
                    Vec2::new(left, top),
                    Vec2::new(right, top),
                    Vec2::new(right, bottom),
                    Vec2::new(left, bottom),
                ];
                free.push(points.into_iter().all(reaches));
            }
        }
        free
    }
}
