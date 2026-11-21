//! Hit testing: which element a press at a point reaches, and where an element can be pressed
//! (docs/beryl-3d.md, "The test driver" and the "Reachable" rule of "Interface checks").
//!
//! An element's area is the quadrilateral of its four corners, cut to its clip. The flat layer
//! is drawn over the table without a depth test, so any element of the flat layer covers every
//! card. Within the flat layer a later element of the tree is drawn over an earlier one; on the
//! table a nearer element (smaller `depth`) covers a farther one. A pile's element lies at its
//! top card's depth and is written after the cards, so a press on a pile reaches the pile, which
//! opens its viewer. An arrow never catches a press, nor does an element with no opacity.

use glam::Vec2;

use kit_frame::frame::{Frame, Place, Resolved, Role, StateBits};
use kit_ui::input::{Rect, Size};

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

/// The first element of the flat layer that is drawn over element `index` at `point` on a
/// panel of its own (`StateBits::PAINTED`), if any. Elements inside `index` are not counted.
/// One the pointer goes through leaves a press at `point` reaching `index`, so `hit` misses it,
/// but its fill still covers `index`: a scrim drawn after a prompt's buttons dims them, so
/// they look disabled though they work.
#[must_use]
pub fn painted_over(frame: &Frame, index: u32, point: Vec2) -> Option<u32> {
    let start = index as usize + 1;
    (frame.resolved[start..].iter().enumerate())
        .map(|(at, element)| ((start + at) as u32, element))
        .find(|&(other, element)| {
            element.place == Place::Layer
                && element.state.has(StateBits::PAINTED)
                && element.opacity > 0.0
                && within(element, point)
                && !is_within(frame, other, index)
        })
        .map(|(other, _)| other)
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
/// The element's bounding box is cut into cells along the edges of every box that overlaps it
/// and can decide what a press there reaches, and into cells at most `CELL` points large. A cell
/// is free when a press at its centre and just inside each of its corners reaches the element.
/// The answer is the largest run of free cells that forms a rectangle. A slanted edge that cuts
/// a cell without reaching any of those five points is missed, which can only make the answer
/// slightly too large.
#[must_use]
pub fn free_rect(frame: &Frame, index: u32, min: Size) -> Option<Rect> {
    let grid = Grid::new(frame, index, everything(frame), Shear::NONE);
    let free = grid.free_cells(frame, index, true);
    grid.search(&free, min, false)
}

/// Whether element `index` has a rectangle at least `min` large where a press reaches it or one
/// of its descendants, upright or leaning as the element's own sides lean. `overlaps` must have
/// been built from `frame`. It stops at the first such rectangle, which is what the "reachable"
/// check asks.
///
/// Upright, it is whether `free_rect` would find one. A battlefield card leans toward the web's
/// vanishing point (`Pose::shear`), and a narrow one near a window's edge holds no upright
/// square of `min` though a press across most of its width reaches it. So when no upright
/// rectangle is found, the same search runs in the element's own leaning frame, where its sides
/// are upright: a rectangle there is a parallelogram on screen, `min.width` wide along every
/// line across it and `min.height` tall.
#[must_use]
pub fn has_free_rect(frame: &Frame, overlaps: &Overlaps, index: u32, min: Size) -> bool {
    let grid = Grid::new(
        frame,
        index,
        overlaps.near(grid_area(frame, index)),
        Shear::NONE,
    );
    if grid.clear_square(frame, index, min) {
        return true;
    }
    let free = grid.free_cells(frame, index, true);
    if grid.search(&free, min, true).is_some() {
        return true;
    }
    let shear = Shear::of(frame.resolved[index as usize].corners);
    if shear == Shear::NONE {
        return false;
    }
    let area = shear.screen_bounds(shear.bounds(&frame.resolved[index as usize]));
    let grid = Grid::new(frame, index, overlaps.near(area), shear);
    let free = grid.free_cells(frame, index, true);
    grid.search(&free, min, true).is_some()
}

/// A horizontal shear that maps a point on screen at `(x, y)` to `(x - lean * y, y)`: the frame
/// in which an element whose sides lean by `lean` points across for each point down stands
/// upright. `Grid` measures in it, and maps each point back to the screen to test a press.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Shear {
    lean: f32,
}

/// The least lean, in points across for each point down, that `Shear::of` counts: below it a
/// card's sides are upright to within a hundredth of a point over its height.
const LEAST_LEAN: f32 = 1e-4;

impl Shear {
    const NONE: Self = Self { lean: 0.0 };

    /// The shear that makes the sides of the quadrilateral `corners` upright: of its two pairs
    /// of opposite sides, the pair that runs nearer to straight down, averaged. A card drawn
    /// in perspective leans each side a little differently, so the frame stands both nearly
    /// upright. `NONE` when they are upright already or the quadrilateral has no height.
    fn of(corners: [Vec2; 4]) -> Self {
        let across = (corners[1] - corners[0]) + (corners[2] - corners[3]);
        let down = (corners[3] - corners[0]) + (corners[2] - corners[1]);
        let side = if across.y.abs() * down.x.abs() > down.y.abs() * across.x.abs() {
            across
        } else {
            down
        };
        if side.y.abs() <= f32::EPSILON {
            return Self::NONE;
        }
        let lean = side.x / side.y;
        if lean.abs() < LEAST_LEAN {
            Self::NONE
        } else {
            Self { lean }
        }
    }

    /// The point on screen at `point` in the sheared frame.
    fn to_screen(self, point: Vec2) -> Vec2 {
        Vec2::new(point.x + self.lean * point.y, point.y)
    }

    /// The box, in the sheared frame, of an element's corners.
    fn bounds(self, element: &Resolved) -> Rect {
        let mut sheared = *element;
        for corner in &mut sheared.corners {
            *corner = Vec2::new(corner.x - self.lean * corner.y, corner.y);
        }
        bounds(&sheared)
    }

    /// The box on screen of `rect` in the sheared frame.
    fn screen_bounds(self, rect: Rect) -> Rect {
        let mut element_box = [
            Vec2::new(rect.x, rect.y),
            Vec2::new(rect.x + rect.width, rect.y),
            Vec2::new(rect.x + rect.width, rect.y + rect.height),
            Vec2::new(rect.x, rect.y + rect.height),
        ];
        for corner in &mut element_box {
            *corner = self.to_screen(*corner);
        }
        let min = element_box
            .into_iter()
            .reduce(Vec2::min)
            .unwrap_or_default();
        let max = element_box
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
}

/// The elements of a frame filed by the squares of the screen their boxes touch, so that finding
/// the elements whose boxes overlap an area costs about as much as there are near it, not as
/// many as the frame has. Built once per frame by the checks that ask this of every element.
pub struct Overlaps {
    /// The top left corner of the square at column 0 and row 0, and each square's side.
    origin: Vec2,
    side: f32,
    columns: usize,
    rows: usize,
    /// The elements that touch each square, row by row, each in index order. Arrows never catch
    /// a press, so they are left out.
    squares: Vec<Vec<u32>>,
    boxes: Vec<Rect>,
}

/// The most squares `Overlaps` cuts the frame into along one axis.
const MOST_SQUARES: f32 = 64.0;

impl Overlaps {
    #[must_use]
    pub fn new(frame: &Frame) -> Self {
        let boxes: Vec<Rect> = frame.resolved.iter().map(bounds).collect();
        let (mut min, mut max) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        for rect in &boxes {
            min = min.min(Vec2::new(rect.x, rect.y));
            max = max.max(Vec2::new(rect.x + rect.width, rect.y + rect.height));
        }
        if boxes.is_empty() {
            (min, max) = (Vec2::ZERO, Vec2::ONE);
        }
        let extent = (max - min).max(Vec2::ONE);
        let side = (extent.max_element() / MOST_SQUARES).max(1.0);
        let columns = (extent.x / side).ceil() as usize + 1;
        let rows = (extent.y / side).ceil() as usize + 1;
        let mut overlaps = Self {
            origin: min,
            side,
            columns,
            rows,
            squares: vec![Vec::new(); columns * rows],
            boxes,
        };
        for (index, element) in frame.resolved.iter().enumerate() {
            if never_catches(element.role) {
                continue;
            }
            let (left, top, right, bottom) = overlaps.span(overlaps.boxes[index]);
            for row in top..=bottom {
                for column in left..=right {
                    overlaps.squares[row * columns + column].push(index as u32);
                }
            }
        }
        overlaps
    }

    /// The columns and rows of the squares `rect` touches, clamped to the frame's: left, top,
    /// right and bottom, inclusive.
    fn span(&self, rect: Rect) -> (usize, usize, usize, usize) {
        let cell = |value: f32, origin: f32, most: usize| {
            (((value - origin) / self.side).floor().max(0.0) as usize).min(most - 1)
        };
        (
            cell(rect.x, self.origin.x, self.columns),
            cell(rect.y, self.origin.y, self.rows),
            cell(rect.x + rect.width, self.origin.x, self.columns),
            cell(rect.y + rect.height, self.origin.y, self.rows),
        )
    }

    /// The elements, other than those that never catch a press, whose boxes share some area with `area`, in index order.
    fn near(&self, area: Rect) -> Vec<u32> {
        let (left, top, right, bottom) = self.span(area);
        let mut near = Vec::new();
        for row in top..=bottom {
            for column in left..=right {
                near.extend(&self.squares[row * self.columns + column]);
            }
        }
        near.sort_unstable();
        near.dedup();
        near.retain(|&other| intersection(area, self.boxes[other as usize]).is_some());
        near
    }
}

/// The elements drawn over element `index` that share part of its box and can catch a press,
/// other than its own descendants, in index order: what can keep a press from reaching it.
/// `overlaps` must have been built from `frame`.
#[must_use]
pub fn covering(frame: &Frame, overlaps: &Overlaps, index: u32) -> Vec<u32> {
    let mut over = overlaps.near(bounds(&frame.resolved[index as usize]));
    over.retain(|&other| {
        !is_within(frame, other, index)
            && catches_anywhere(&frame.resolved[other as usize])
            && covers(frame, other, index)
    });
    over
}

/// How much of element `index`'s area is covered by other elements, from 0 to 1, measured on
/// the same cells as `free_rect`, each counted by its centre.
#[must_use]
pub fn covered(frame: &Frame, index: u32) -> f32 {
    let grid = Grid::new(frame, index, everything(frame), Shear::NONE);
    let element = &frame.resolved[index as usize];
    if grid.alone && catches_anywhere(element) {
        // Nothing but the element and its descendants overlaps it, and the element catches a
        // press anywhere inside it, so a press inside it always reaches it or a descendant.
        return 0.0;
    }
    let free = grid.free_cells(frame, index, false);
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
    catches_anywhere(element) && within(element, point)
}

/// Whether a press inside `element` lands on it, ignoring what covers it. One the pointer goes
/// through (`StateBits::PASSES`) never does.
fn catches_anywhere(element: &Resolved) -> bool {
    !never_catches(element.role) && element.opacity > 0.0 && !element.state.has(StateBits::PASSES)
}

/// Whether an element of `role` never catches a press: an arrow.
fn never_catches(role: Role) -> bool {
    role == Role::Arrow
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
        // A card in flight passes over a pile's element, which names the pile's top card at
        // rest: a card landing on the pile is drawn, and pressed, above it.
        (false, false) if flying(first) && second.role == Role::Pile => true,
        (false, false) if first.role == Role::Pile && flying(second) => false,
        (false, false) => first.depth.total_cmp(&second.depth).then(b.cmp(&a)).is_lt(),
    }
}

/// Whether `element` is a card a script or the pointer is moving.
fn flying(element: &Resolved) -> bool {
    element.role == Role::Card && element.state.has(StateBits::MOVING)
}

/// Every element of `frame` other than those that never catch a press, in index order.
fn everything(frame: &Frame) -> Vec<u32> {
    (0..frame.resolved.len() as u32)
        .filter(|&index| !never_catches(frame.resolved[index as usize].role))
        .collect()
}

/// The part of an element's box a press can land on: its box cut to its clip, or `None` when
/// the clip hides all of it.
#[must_use]
pub fn visible_bounds(element: &Resolved) -> Option<Rect> {
    let area = bounds(element);
    match element.clip {
        Some(clip) => intersection(area, clip),
        None => Some(area),
    }
}

/// The area `Grid` measures element `index` on: its box, cut to its clip.
fn grid_area(frame: &Frame, index: u32) -> Rect {
    let element = &frame.resolved[index as usize];
    visible_bounds(element).unwrap_or_else(|| {
        let area = bounds(element);
        Rect {
            x: area.x,
            y: area.y,
            width: 0.0,
            height: 0.0,
        }
    })
}

/// The cells `free_rect` and `covered` measure an element on: its box cut along the edges of
/// every box that overlaps it and can decide what a press reaches there, and then into cells no
/// larger than `CELL` points, so that the cells along a slanted edge of a card drawn in
/// perspective are small.
struct Grid {
    xs: Vec<f32>,
    ys: Vec<f32>,
    /// The elements whose boxes overlap the element's and that can decide what a press inside
    /// its box reaches, itself among them.
    candidates: Vec<u32>,
    /// Whether every candidate is the element or one of its descendants.
    alone: bool,
    /// The largest width and height of a cell.
    cell: Vec2,
    /// The frame the grid's cells and boxes are measured in.
    shear: Shear,
}

/// The largest side of a cell, in logical points, and the most cuts a grid makes along one
/// axis to keep to it. A slanted edge through a cell leaves the cell out of the free area, so
/// beside each such edge the grid can miss up to a cell of an element's uncovered area.
pub(crate) const CELL: f32 = 4.0;
const MOST_CUTS: f32 = 64.0;

impl Grid {
    /// The grid for element `index`, from `near`: the elements other than arrows whose boxes
    /// may overlap its own, in index order, a superset of those that do.
    ///
    /// Of those, only the element, its descendants and what is drawn over the lowest of them
    /// can decide whether a press reaches it. Where an element drawn under all of them is the
    /// topmost to catch a press, none of them catches it, so the press reaches some other
    /// element whether or not that one is counted. Leaving such elements out keeps a card in
    /// a crowded row from being measured against every card drawn under it.
    ///
    /// The grid is measured in the frame `shear` maps the screen to. With a shear, its area is
    /// the element's whole box in that frame: its clip, a rectangle on screen, is a
    /// parallelogram there, and a press tested outside it reaches nothing anyway.
    fn new(frame: &Frame, index: u32, near: Vec<u32>, shear: Shear) -> Self {
        let area = if shear == Shear::NONE {
            grid_area(frame, index)
        } else {
            shear.bounds(&frame.resolved[index as usize])
        };
        let box_of = |other: u32| shear.bounds(&frame.resolved[other as usize]);
        let (mut xs, mut ys) = (Vec::new(), Vec::new());
        let mut cell = Vec2::ZERO;
        for (edges, start, length, side) in [
            (&mut xs, area.x, area.width, &mut cell.x),
            (&mut ys, area.y, area.height, &mut cell.y),
        ] {
            let cuts = (length / CELL).ceil().clamp(1.0, MOST_CUTS) as u32;
            edges.extend((0..=cuts).map(|cut| start + length * cut as f32 / cuts as f32));
            *side = length / cuts as f32;
        }
        let mut overlapping = Vec::with_capacity(near.len());
        let mut lowest = index;
        for other in near {
            if other != index && intersection(area, box_of(other)).is_none() {
                continue;
            }
            let inside = is_within(frame, other, index);
            if inside && covers(frame, lowest, other) {
                lowest = other;
            }
            overlapping.push((other, inside));
        }
        let mut candidates = Vec::with_capacity(overlapping.len());
        let mut alone = true;
        for (other, inside) in overlapping {
            if !inside && !covers(frame, other, lowest) {
                continue;
            }
            alone &= inside;
            candidates.push(other);
            if other == index {
                continue;
            }
            let shared =
                intersection(area, box_of(other)).expect("only overlapping elements are kept");
            xs.extend([shared.x, shared.x + shared.width]);
            ys.extend([shared.y, shared.y + shared.height]);
        }
        if shear != Shear::NONE {
            // A card drawn in perspective leans its two sides a little differently, so in the
            // sheared frame they stand a fraction of a point off the box's edges. Cut there too,
            // or the cells along both sides would each hold a sliver outside the card and be
            // lost, a cell from each side of the free area.
            for corner in frame.resolved[index as usize].corners {
                xs.push(corner.x - shear.lean * corner.y);
                ys.push(corner.y);
            }
        }
        if candidates.binary_search(&index).is_err() {
            // An element with an empty area overlaps nothing, itself included.
            candidates.push(index);
            candidates.sort_unstable();
        }
        for edges in [&mut xs, &mut ys] {
            edges.sort_by(f32::total_cmp);
            edges.dedup_by(|a, b| (*a - *b).abs() < 0.001);
        }
        Self {
            xs,
            ys,
            candidates,
            alone,
            cell,
            shear,
        }
    }

    /// Whether a square centred on element `index`, larger than `min` by two cells on each
    /// axis, lies inside the element and its clip and overlaps no other candidate's box except
    /// its descendants'. Then a press anywhere in it reaches the element or a descendant, and
    /// the cells wholly inside it, which `free_cells` finds free, span at least `min`: the
    /// grid would find a free rectangle that large, so it need not be built.
    fn clear_square(&self, frame: &Frame, index: u32, min: Size) -> bool {
        let element = &frame.resolved[index as usize];
        if !catches_anywhere(element) {
            return false;
        }
        let centre = element.corners.iter().copied().sum::<Vec2>() / 4.0;
        let half = (Vec2::new(min.width, min.height) + 2.0 * self.cell) / 2.0;
        let square = Rect {
            x: centre.x - half.x,
            y: centre.y - half.y,
            width: 2.0 * half.x,
            height: 2.0 * half.y,
        };
        let corners = [
            centre - half,
            Vec2::new(centre.x + half.x, centre.y - half.y),
            centre + half,
            Vec2::new(centre.x - half.x, centre.y + half.y),
        ];
        // The quadrilateral and the clip are convex, so they hold the square when they hold
        // its corners.
        corners.iter().all(|&corner| within(element, corner))
            && self.candidates.iter().all(|&other| {
                other == index
                    || is_within(frame, other, index)
                    || intersection(square, bounds(&frame.resolved[other as usize])).is_none()
            })
    }

    /// The largest rectangle of free cells at least `min` large, by area, or with `first` the
    /// first one found.
    fn search(&self, free: &[bool], min: Size, first: bool) -> Option<Rect> {
        let (columns, rows) = (self.xs.len() - 1, self.ys.len() - 1);
        let mut best: Option<(f32, Rect)> = None;
        for left in 0..columns {
            // `open[row]` is whether every cell from `left` to `right` in that row is free.
            let mut open = vec![true; rows];
            for right in left..columns {
                let width = self.xs[right + 1] - self.xs[left];
                let mut any = false;
                for (row, open) in open.iter_mut().enumerate() {
                    *open &= free[row * columns + right];
                    any |= *open;
                }
                if !any {
                    // Wider runs from `left` hold no free cell either.
                    break;
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
                    let height = self.ys[bottom + 1] - self.ys[top];
                    if height < min.height {
                        continue;
                    }
                    let area = width * height;
                    if best.is_none_or(|(most, _)| area > most) {
                        best = Some((
                            area,
                            Rect {
                                x: self.xs[left],
                                y: self.ys[top],
                                width,
                                height,
                            },
                        ));
                        if first {
                            return best.map(|(_, rect)| rect);
                        }
                    }
                }
            }
        }
        best.map(|(_, rect)| rect)
    }

    /// Whether a press at `point`, in the grid's frame, reaches element `index` or one of its
    /// descendants.
    fn reaches(&self, frame: &Frame, index: u32, point: Vec2) -> bool {
        let point = self.shear.to_screen(point);
        let mut best: Option<u32> = None;
        for &candidate in &self.candidates {
            if !catches(&frame.resolved[candidate as usize], point) {
                continue;
            }
            best = match best {
                Some(other) if covers(frame, other, candidate) => Some(other),
                _ => Some(candidate),
            };
        }
        best.is_some_and(|at| is_within(frame, at, index))
    }

    /// Whether each cell is free, row by row. With `whole`, a press at the cell's centre and
    /// just inside each of its corners must reach the element; otherwise only at its centre.
    fn free_cells(&self, frame: &Frame, index: u32, whole: bool) -> Vec<bool> {
        let mut free = Vec::with_capacity((self.xs.len() - 1) * (self.ys.len() - 1));
        for ys in self.ys.windows(2) {
            for xs in self.xs.windows(2) {
                let center = Vec2::new(f32::midpoint(xs[0], xs[1]), f32::midpoint(ys[0], ys[1]));
                if !whole {
                    free.push(self.reaches(frame, index, center));
                    continue;
                }
                let inset = 0.01_f32
                    .min((xs[1] - xs[0]) / 4.0)
                    .min((ys[1] - ys[0]) / 4.0);
                let (left, right) = (xs[0] + inset, xs[1] - inset);
                let (top, bottom) = (ys[0] + inset, ys[1] - inset);
                let points = [
                    center,
                    Vec2::new(left, top),
                    Vec2::new(right, top),
                    Vec2::new(right, bottom),
                    Vec2::new(left, bottom),
                ];
                free.push(
                    points
                        .into_iter()
                        .all(|point| self.reaches(frame, index, point)),
                );
            }
        }
        free
    }
}
