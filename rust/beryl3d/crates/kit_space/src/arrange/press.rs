//! Proves that a square fits in the exposed part of a projected rectangle. Convex clipping
//! works for tilted and perspective-projected items as well as face-on ones.

use arrayvec::ArrayVec;
use glam::Vec2;

use super::{Camera, Region};

type Polygon = Buffer<Vec2, 16>;

/// Keep ordinary clips and nearby blockers on the stack. Dense layouts can exceed the
/// inline capacity, so the same search spills to the heap when it needs more room.
struct Buffer<T, const CAPACITY: usize> {
    inline: ArrayVec<T, CAPACITY>,
    heap: Option<Vec<T>>,
}

impl<T, const CAPACITY: usize> Buffer<T, CAPACITY> {
    fn new() -> Self {
        Self {
            inline: ArrayVec::new(),
            heap: None,
        }
    }

    #[inline]
    fn push(&mut self, value: T) {
        if let Some(heap) = &mut self.heap {
            heap.push(value);
        } else if let Err(full) = self.inline.try_push(value) {
            self.spill(full.element());
        }
    }

    #[cold]
    fn spill(&mut self, value: T) {
        let mut heap = Vec::with_capacity(CAPACITY * 2);
        heap.extend(self.inline.drain(..));
        heap.push(value);
        self.heap = Some(heap);
    }

    fn as_slice(&self) -> &[T] {
        self.heap.as_deref().unwrap_or(&self.inline)
    }

    fn as_mut_slice(&mut self) -> &mut [T] {
        self.heap.as_deref_mut().unwrap_or(&mut self.inline)
    }
}

impl<T, const CAPACITY: usize> FromIterator<T> for Buffer<T, CAPACITY> {
    fn from_iter<I: IntoIterator<Item = T>>(items: I) -> Self {
        let mut buffer = Self::new();
        for item in items {
            buffer.push(item);
        }
        buffer
    }
}

/// Extra space left around the witness, in logical points, for projection rounding.
const CLEARANCE: f32 = 0.002;

#[derive(Clone, Copy)]
pub(super) struct HalfPlane {
    inward: Vec2,
    limit: f32,
}

impl HalfPlane {
    fn edge(a: Vec2, b: Vec2, inside: Vec2) -> Self {
        let mut inward = (b - a).perp().normalize_or_zero();
        if inward.dot(inside - a) < 0.0 {
            inward = -inward;
        }
        Self {
            inward,
            limit: inward.dot(a),
        }
    }

    fn outside(self) -> Self {
        Self {
            inward: -self.inward,
            limit: -self.limit,
        }
    }

    fn contains(self, point: Vec2) -> bool {
        self.inward.dot(point) >= self.limit
    }

    fn strictly_contains(self, point: Vec2) -> bool {
        self.inward.dot(point) > self.limit
    }

    fn clip(self, polygon: &[Vec2]) -> Polygon {
        let mut result = Polygon::new();
        let Some((&first, rest)) = polygon.split_first() else {
            return result;
        };
        let mut a = first;
        let mut from = self.inward.dot(first) - self.limit;
        for &b in rest.iter().chain(std::iter::once(&first)) {
            let to = self.inward.dot(b) - self.limit;
            if from >= 0.0 {
                result.push(a);
            }
            if (from < 0.0) != (to < 0.0) {
                result.push(a.lerp(b, from / (from - to)));
            }
            a = b;
            from = to;
        }
        result
    }
}

/// The side of the line at `distance` that contains earlier items, projected from the
/// region's plane. The later items advance along `direction`.
pub(super) fn strip(camera: &Camera, region: Region, at: Vec2, direction: Vec2) -> HalfPlane {
    let across = direction.perp();
    HalfPlane::edge(
        camera.to_screen(region.point(at - across)),
        camera.to_screen(region.point(at + across)),
        camera.to_screen(region.point(at - direction)),
    )
}

pub(super) struct Quad {
    corners: [Vec2; 4],
    pub(super) bounds: Bounds,
    planes: [HalfPlane; 4],
}

/// The outline of a screen-facing item. The local x axis points along its bottom edge.
#[derive(Clone, Copy)]
pub(super) struct Rectangle {
    pub(super) centre: Vec2,
    pub(super) half: Vec2,
    pub(super) axis: Vec2,
}

impl Rectangle {
    fn corners(self) -> [Vec2; 4] {
        let x = self.axis * self.half.x;
        let y = -self.axis.perp() * self.half.y;
        [
            self.centre - x - y,
            self.centre + x - y,
            self.centre + x + y,
            self.centre - x + y,
        ]
    }

    pub(super) fn bounds(self) -> Bounds {
        Bounds::of(self.corners())
    }

    pub(super) fn overlaps(self, other: Self) -> bool {
        let offset = other.centre - self.centre;
        // A pair of rectangles can be separated only along an axis of one of the faces.
        [self.axis, self.axis.perp(), other.axis, other.axis.perp()]
            .into_iter()
            .all(|axis| offset.dot(axis).abs() < self.extent(axis) + other.extent(axis))
    }

    fn extent(self, axis: Vec2) -> f32 {
        self.half.dot(Vec2::new(
            axis.dot(self.axis).abs(),
            axis.perp_dot(self.axis).abs(),
        ))
    }
}

impl Quad {
    pub(super) fn new(corners: [Vec2; 4]) -> Self {
        let centre = corners.iter().copied().sum::<Vec2>() / 4.0;
        let planes =
            std::array::from_fn(|i| HalfPlane::edge(corners[i], corners[(i + 1) % 4], centre));
        Self {
            corners,
            bounds: Bounds::of(corners),
            planes,
        }
    }

    pub(super) fn has_area(&self) -> bool {
        let [a, b, c, d] = self.corners;
        (c - a).perp_dot(d - b).abs() > f32::EPSILON
    }

    fn along(&self, axis: Vec2) -> (f32, f32) {
        let [a, b, c, d] = self.corners.map(|point| point.dot(axis));
        (a.min(b).min(c.min(d)), a.max(b).max(c.max(d)))
    }
}

#[derive(Clone, Copy)]
pub(super) struct Bounds {
    min: Vec2,
    max: Vec2,
}

impl Bounds {
    fn of([a, b, c, d]: [Vec2; 4]) -> Self {
        Self {
            min: a.min(b).min(c.min(d)),
            max: a.max(b).max(c.max(d)),
        }
    }

    pub(super) fn overlaps(self, other: Self) -> bool {
        self.min.cmplt(other.max).all() && other.min.cmplt(self.max).all()
    }

    fn contains_with_clearance(self, other: Self) -> bool {
        other.min.cmpge(self.min + Vec2::splat(CLEARANCE)).all()
            && other.max.cmple(self.max - Vec2::splat(CLEARANCE)).all()
    }
}

pub(super) fn safe_area(camera: &Camera) -> Quad {
    let inset = camera.window.safe_area;
    let size = camera.window.size;
    let (x, y) = (size.width - inset.right, size.height - inset.bottom);
    Quad::new([
        Vec2::new(inset.left, inset.top),
        Vec2::new(x, inset.top),
        Vec2::new(x, y),
        Vec2::new(inset.left, y),
    ])
}

/// The possible centres of a square turned to follow the projected item's local x axis.
/// Each half plane that contains the square is moved inward by the square's radius in that
/// direction. Their intersection is the set of centres for which all four corners fit.
pub(super) struct PressArea {
    centres: Polygon,
    axes: [Vec2; 2],
    half: f32,
}

impl PressArea {
    pub(super) fn new(item: &Quad, screen: &Quad, side: f32) -> Self {
        let [a, b, c, d] = item.corners;
        let axis = ((b - a) + (c - d)).normalize_or_zero();
        let mut area = Self {
            centres: item.corners.into_iter().collect(),
            axes: [axis, axis.perp()],
            half: side / 2.0,
        };
        area.clip_to(&item.planes);
        area.clip_to(&screen.planes);
        area
    }

    /// A square aligned with a screen-facing rectangle fits at every centre of the rectangle
    /// inset by half the square's side, plus clearance. A rectangle that crosses the safe
    /// area's edge still needs clipping against the window's inset planes.
    pub(super) fn in_rectangle(
        rectangle: Rectangle,
        bounds: Bounds,
        screen: &Quad,
        side: f32,
    ) -> Self {
        let half = side / 2.0;
        let inset = Rectangle {
            half: rectangle.half - Vec2::splat(half + CLEARANCE),
            ..rectangle
        };
        let centres = if inset.half.cmpge(Vec2::ZERO).all() {
            inset.corners().into_iter().collect()
        } else {
            Polygon::new()
        };
        let mut area = Self {
            centres,
            axes: [rectangle.axis, rectangle.axis.perp()],
            half,
        };
        if !screen.bounds.contains_with_clearance(bounds) {
            area.clip_to(&screen.planes);
        }
        area
    }

    fn clip_to(&mut self, planes: &[HalfPlane; 4]) {
        for &plane in planes {
            let centres = self.centres.as_slice();
            if centres.is_empty() {
                break;
            }
            let plane = self.eroded(plane);
            if !centres.iter().all(|&centre| plane.contains(centre)) {
                self.centres = plane.clip(centres);
            }
        }
    }

    fn radius(&self, axis: Vec2) -> f32 {
        let [x, y] = self.axes;
        (axis.dot(x).abs() + axis.dot(y).abs()) * (self.half + CLEARANCE)
    }

    fn eroded(&self, mut plane: HalfPlane) -> HalfPlane {
        plane.limit += self.radius(plane.inward);
        plane
    }

    pub(super) fn holds_in(&self, strip: HalfPlane) -> bool {
        let strip = self.eroded(strip);
        self.centres
            .as_slice()
            .iter()
            .any(|&centre| strip.contains(centre))
    }

    pub(super) fn witness<'a>(
        &self,
        blockers: impl Iterator<Item = &'a Quad>,
    ) -> Option<[Vec2; 4]> {
        self.witness_outside(blockers.map(|quad| self.forbidden(quad)))
    }

    pub(super) fn witness_rectangles(
        &self,
        blockers: impl Iterator<Item = Rectangle>,
    ) -> Option<[Vec2; 4]> {
        self.witness_outside(blockers.map(|rectangle| self.forbidden_rectangle(rectangle)))
    }

    fn witness_outside(&self, blockers: impl Iterator<Item = [HalfPlane; 8]>) -> Option<[Vec2; 4]> {
        if self.centres.as_slice().is_empty() {
            return None;
        }
        let mut forbidden: Buffer<_, 12> = blockers.collect();
        let centre = uncovered(self.centres.as_slice(), forbidden.as_mut_slice())?;
        let [x, y] = self.axes.map(|axis| axis * self.half);
        Some([
            centre - x - y,
            centre + x - y,
            centre + x + y,
            centre - x + y,
        ])
    }

    /// The centres for which the square overlaps the blocker. Two convex polygons intersect
    /// when their projections overlap on the normals of both polygons. Each such overlap is a
    /// constraint on the square's centre, giving the forbidden polygon.
    fn forbidden(&self, blocker: &Quad) -> [HalfPlane; 8] {
        let expand = |mut plane: HalfPlane| {
            plane.limit -= self.radius(plane.inward);
            plane
        };
        let [x, y] = self.axes;
        // Reversing an axis negates its upper bound, so each corner only needs one projection
        // onto each of the square's axes.
        let (left, right) = blocker.along(x);
        let (bottom, top) = blocker.along(y);
        // These four normals already follow the square axes. They all have the same radius.
        let radius = self.radius(x);
        let bound = |inward, limit| HalfPlane {
            inward,
            limit: limit - radius,
        };
        let [first, second, third, fourth] = blocker.planes.map(expand);
        [
            bound(x, left),
            bound(y, bottom),
            bound(-x, -right),
            bound(-y, -top),
            first,
            second,
            third,
            fourth,
        ]
    }

    fn forbidden_rectangle(&self, blocker: Rectangle) -> [HalfPlane; 8] {
        let [x, y] = self.axes;
        let right = blocker.axis;
        let up = -right.perp();
        // Both rectangles have perpendicular axes. The two relative projections give the
        // support distances on all four axes without projecting the blocker corner by corner.
        let along = x.dot(right).abs();
        let across = x.dot(up).abs();
        let square_radius = self.radius(x);
        let square_on_blocker = (along + across) * (self.half + CLEARANCE);
        let extent_x = blocker.half.dot(Vec2::new(along, across)) + square_radius;
        let extent_y = blocker.half.dot(Vec2::new(across, along)) + square_radius;
        let extents_on_blocker =
            blocker.half * right.length_squared() + Vec2::splat(square_on_blocker);
        let centre_x = blocker.centre.dot(x);
        let centre_y = blocker.centre.dot(y);
        let centre_right = blocker.centre.dot(right);
        let centre_up = blocker.centre.dot(up);
        let bound = |axis, centre, extent| HalfPlane {
            inward: axis,
            limit: centre - extent,
        };
        [
            bound(x, centre_x, extent_x),
            bound(y, centre_y, extent_y),
            bound(-x, -centre_x, extent_x),
            bound(-y, -centre_y, extent_y),
            bound(up, centre_up, extents_on_blocker.y),
            bound(-right, -centre_right, extents_on_blocker.x),
            bound(-up, -centre_up, extents_on_blocker.y),
            bound(right, centre_right, extents_on_blocker.x),
        ]
    }
}

/// Find a centre outside all the forbidden polygons. For each obstacle, its outside is the
/// union of the outsides of its edges. Keep those pieces and discard their common inside.
fn uncovered(polygon: &[Vec2], forbidden: &mut [[HalfPlane; 8]]) -> Option<Vec2> {
    if polygon.is_empty() {
        return None;
    }
    let excludes =
        |planes: &[HalfPlane; 8], point| planes.iter().any(|plane| plane.outside().contains(point));
    let centre = polygon.iter().copied().sum::<Vec2>() / polygon.len() as f32;
    let Some(first) = forbidden
        .iter()
        .position(|planes| !excludes(planes, centre))
    else {
        return Some(centre);
    };
    // A convex blocker containing every vertex leaves no valid centre anywhere in the polygon.
    if polygon
        .iter()
        .all(|&point| !excludes(&forbidden[first], point))
    {
        return None;
    }
    // A narrow exposed strip often includes a vertex. Try those before splitting polygons.
    if let Some(point) = polygon
        .iter()
        .copied()
        .find(|&point| forbidden.iter().all(|planes| excludes(planes, point)))
    {
        return Some(point);
    }
    // Split along a blocker that covers the centre. The other blockers still apply to each
    // piece, but need not be split first merely because they were drawn sooner.
    forbidden.swap(0, first);
    let (obstacle, rest) = forbidden
        .split_first_mut()
        .expect("an obstacle covers the centre");
    let mut inside: Option<Polygon> = None;
    for plane in obstacle.iter().copied() {
        let points = inside.as_ref().map_or(polygon, Polygon::as_slice);
        let outside = plane.outside();
        // A plane that never meets the polygon leaves only one piece. Keep exact edge
        // contacts in the clipping path because they can contain a valid centre.
        if points.iter().all(|&point| plane.strictly_contains(point)) {
            continue;
        }
        if points.iter().all(|&point| outside.strictly_contains(point)) {
            return uncovered(points, rest);
        }
        let outside = outside.clip(points);
        if let Some(point) = uncovered(outside.as_slice(), rest) {
            return Some(point);
        }
        inside = Some(plane.clip(points));
    }
    None
}
