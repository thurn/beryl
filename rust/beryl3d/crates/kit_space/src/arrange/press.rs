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

    fn push(&mut self, value: T) {
        if let Some(heap) = &mut self.heap {
            heap.push(value);
        } else if let Err(full) = self.inline.try_push(value) {
            let mut heap = Vec::with_capacity(CAPACITY * 2);
            heap.extend(self.inline.drain(..));
            heap.push(full.element());
            self.heap = Some(heap);
        }
    }

    fn as_slice(&self) -> &[T] {
        self.heap.as_deref().unwrap_or(&self.inline)
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

    fn clip(self, polygon: &[Vec2]) -> Polygon {
        let mut result = Polygon::new();
        for (a, b) in edges(polygon) {
            let from = self.inward.dot(a) - self.limit;
            let to = self.inward.dot(b) - self.limit;
            if from >= 0.0 {
                result.push(a);
            }
            if (from < 0.0) != (to < 0.0) {
                result.push(a.lerp(b, from / (from - to)));
            }
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

fn edges(polygon: &[Vec2]) -> impl Iterator<Item = (Vec2, Vec2)> + '_ {
    polygon
        .iter()
        .copied()
        .zip(polygon.iter().copied().cycle().skip(1))
}

#[derive(Clone, Copy)]
pub(super) struct Quad(pub [Vec2; 4]);

impl Quad {
    pub(super) fn has_area(self) -> bool {
        let [a, b, c, d] = self.0;
        (c - a).perp_dot(d - b).abs() > f32::EPSILON
    }

    fn planes(self) -> impl Iterator<Item = HalfPlane> {
        let centre = self.0.iter().copied().sum::<Vec2>() / 4.0;
        (0..4).map(move |i| HalfPlane::edge(self.0[i], self.0[(i + 1) % 4], centre))
    }

    fn along(self, axis: Vec2) -> (f32, f32) {
        self.0
            .map(|point| point.dot(axis))
            .into_iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), point| {
                (lo.min(point), hi.max(point))
            })
    }

    pub(super) fn bounds_overlap(self, other: Self) -> bool {
        [Vec2::X, Vec2::Y].into_iter().all(|axis| {
            let (lo, hi) = self.along(axis);
            let (other_lo, other_hi) = other.along(axis);
            lo < other_hi && other_lo < hi
        })
    }
}

pub(super) fn safe_area(camera: &Camera) -> Quad {
    let inset = camera.window.safe_area;
    let size = camera.window.size;
    let (x, y) = (size.width - inset.right, size.height - inset.bottom);
    Quad([
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
    pub(super) fn new(item: Quad, screen: Quad, side: f32) -> Self {
        let axis = ((item.0[1] - item.0[0]) + (item.0[2] - item.0[3])).normalize_or_zero();
        let mut area = Self {
            centres: item.0.into_iter().collect(),
            axes: [axis, axis.perp()],
            half: side / 2.0,
        };
        for plane in item.planes().chain(screen.planes()) {
            area.centres = area.eroded(plane).clip(area.centres.as_slice());
        }
        area
    }

    fn radius(&self, axis: Vec2) -> f32 {
        self.axes
            .iter()
            .map(|way| axis.dot(*way).abs())
            .sum::<f32>()
            * (self.half + CLEARANCE)
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

    pub(super) fn witness(&self, blockers: impl Iterator<Item = Quad>) -> Option<[Vec2; 4]> {
        let forbidden: Buffer<_, 12> = blockers.map(|quad| self.forbidden(quad)).collect();
        let centre = uncovered(self.centres.as_slice(), forbidden.as_slice())?;
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
    fn forbidden(&self, blocker: Quad) -> [HalfPlane; 8] {
        let expand = |mut plane: HalfPlane| {
            plane.limit -= self.radius(plane.inward);
            plane
        };
        let [x, y] = self.axes;
        let mut bounds = [x, y, -x, -y].map(|inward| HalfPlane {
            inward,
            limit: blocker.along(inward).0,
        });
        for plane in &mut bounds {
            *plane = expand(*plane);
        }
        let mut edges = blocker.planes().map(expand);
        std::array::from_fn(|i| {
            if i < bounds.len() {
                bounds[i]
            } else {
                edges.next().expect("a quad has four edges")
            }
        })
    }
}

/// Find a centre outside all the forbidden polygons. For each obstacle, its outside is the
/// union of the outsides of its edges. Keep those pieces and discard their common inside.
fn uncovered(polygon: &[Vec2], forbidden: &[[HalfPlane; 8]]) -> Option<Vec2> {
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
    // Earlier blockers excluded the centre, but can still cover the other pieces.
    let (obstacle, rest) = forbidden
        .split_first()
        .expect("an obstacle covers the centre");
    if first > 0
        && obstacle
            .iter()
            .any(|plane| polygon.iter().all(|&point| plane.outside().contains(point)))
    {
        return uncovered(polygon, rest);
    }
    let mut inside: Polygon = polygon.iter().copied().collect();
    for plane in obstacle {
        let outside = plane.outside().clip(inside.as_slice());
        if let Some(point) = uncovered(outside.as_slice(), rest) {
            return Some(point);
        }
        inside = plane.clip(inside.as_slice());
    }
    None
}
