//! Screen bounds for batching draw records and measuring their area.

use glam::{Mat4, Vec2, Vec4};

use super::super::{Draw, Mesh, sprites};

/// The most vertices a quad has after being cut by the six planes of the view volume: each
/// plane adds at most one.
const CLIPPED_VERTICES: usize = 4 + 6;

/// How far a record's rectangle on screen is grown on each side before two are compared, in
/// normalized device coordinates, where the screen is 2 across: more than the one pixel the
/// soft edge of a card reaches outside the card, in any window 200 or more pixels wide.
const OVERLAP_MARGIN: f32 = 0.01;

/// A rectangle on screen in normalized device coordinates: left, bottom, right, top. An empty
/// one has its left past its right.
pub(super) type Rect = [f32; 4];

const EMPTY: Rect = [1.0, 1.0, -1.0, -1.0];

pub(super) fn overlap(a: &Rect, b: &Rect) -> bool {
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}

/// Writes into `polygon` the corners of the rectangle `half` large about the origin of `draw`'s
/// mesh, in clip space, cut to the view volume, and returns how many there are: 0 when none of
/// it is in view.
fn clipped(
    draw: &Draw,
    view_projection: &Mat4,
    half: Vec2,
    polygon: &mut [Vec4; CLIPPED_VERTICES],
) -> usize {
    let m = &draw.model;
    let corner = |x: f32, y: f32| {
        *view_projection
            * Vec4::new(
                m[0] * x + m[1] * y + m[3],
                m[4] * x + m[5] * y + m[7],
                m[8] * x + m[9] * y + m[11],
                1.0,
            )
    };
    polygon[..4].copy_from_slice(&[
        corner(-half.x, -half.y),
        corner(half.x, -half.y),
        corner(half.x, half.y),
        corner(-half.x, half.y),
    ]);
    let mut count = 4;
    // A quad wholly inside the view volume, as most are, keeps its four corners, so it is cut
    // only when one of them lies outside.
    let inside = polygon[..4].iter().all(|point| {
        point.x.abs() <= point.w && point.y.abs() <= point.w && point.z >= 0.0 && point.z <= point.w
    });
    if !inside {
        for plane in PLANES {
            count = cut(polygon, count, plane);
            if count < 3 {
                return 0;
            }
        }
    }
    count
}

/// The share of the screen inside the rectangle `half` large about `draw`'s origin, cut to
/// the view volume.
fn rect_area(draw: &Draw, view_projection: &Mat4, half: Vec2) -> f32 {
    let mut polygon = [Vec4::ZERO; CLIPPED_VERTICES];
    let count = clipped(draw, view_projection, half, &mut polygon);
    if count < 3 {
        return 0.0;
    }
    // Twice the area by the shoelace formula, in normalized device coordinates, where the
    // screen is 2 by 2.
    let points = polygon[..count]
        .iter()
        .map(|point| point.truncate().truncate() / point.w);
    let mut twice = 0.0;
    let mut previous = polygon[count - 1].truncate().truncate() / polygon[count - 1].w;
    for point in points {
        twice += previous.perp_dot(point);
        previous = point;
    }
    twice.abs() / 8.0
}

/// Half the size of the rectangle that bounds `mesh` in its own space, where a card mesh is
/// `card` large.
fn bounds(mesh: Mesh, card: Vec2) -> Vec2 {
    match mesh {
        Mesh::Card | Mesh::CardEdge => card / 2.0,
        Mesh::Quad => Vec2::splat(0.5),
        Mesh::Outline => Vec2::from(sprites::OUTLINE_OUTER),
    }
}

/// The part of the screen `draw` covers, as a fraction of the screen's area. The record's
/// footprint is the rectangle that bounds its mesh, cut to the view volume, less the outline
/// mesh's empty middle. The rounded corners of a card are counted as covered. The edge strip is
/// one pixel wide, so it is counted as covering nothing. A clip rectangle is not taken into
/// account.
pub(super) fn screen_area(draw: &Draw, view_projection: &Mat4, card: Vec2) -> f32 {
    match mesh_of(draw) {
        Mesh::CardEdge => 0.0,
        Mesh::Outline => {
            rect_area(draw, view_projection, bounds(Mesh::Outline, card))
                - rect_area(draw, view_projection, Vec2::from(sprites::OUTLINE_HOLE))
        }
        mesh => rect_area(draw, view_projection, bounds(mesh, card)),
    }
}

/// The rectangle on screen that bounds `draw`'s mesh, grown by `OVERLAP_MARGIN` on each side,
/// or `EMPTY` when none of it is on screen. A record wholly in front of the camera is bounded by
/// its four corners as projected; only one that reaches behind it is cut to the view volume
/// first. Depth and a clip rectangle are not taken into account, so the rectangle may be larger
/// than what is drawn, never smaller.
pub(super) fn screen_rect(draw: &Draw, view_projection: &Mat4, card: Vec2) -> Rect {
    let half = bounds(mesh_of(draw), card);
    let m = &draw.model;
    let corner = |x: f32, y: f32| {
        *view_projection
            * Vec4::new(
                m[0] * x + m[1] * y + m[3],
                m[4] * x + m[5] * y + m[7],
                m[8] * x + m[9] * y + m[11],
                1.0,
            )
    };
    let corners = [
        corner(-half.x, -half.y),
        corner(half.x, -half.y),
        corner(half.x, half.y),
        corner(-half.x, half.y),
    ];
    let mut rect = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    let mut widen = |point: &Vec4| {
        let (x, y) = (point.x / point.w, point.y / point.w);
        rect = [
            rect[0].min(x),
            rect[1].min(y),
            rect[2].max(x),
            rect[3].max(y),
        ];
    };
    if corners.iter().all(|point| point.w > f32::EPSILON) {
        corners.iter().for_each(&mut widen);
    } else {
        let mut polygon = [Vec4::ZERO; CLIPPED_VERTICES];
        let count = clipped(draw, view_projection, half, &mut polygon);
        polygon[..count].iter().for_each(&mut widen);
    }
    if rect[0] > 1.0 || rect[2] < -1.0 || rect[1] > 1.0 || rect[3] < -1.0 {
        return EMPTY;
    }
    [
        rect[0].max(-1.0) - OVERLAP_MARGIN,
        rect[1].max(-1.0) - OVERLAP_MARGIN,
        rect[2].min(1.0) + OVERLAP_MARGIN,
        rect[3].min(1.0) + OVERLAP_MARGIN,
    ]
}

/// The view volume in clip space, with depth from 0 to w: each plane keeps the points where the
/// dot product with it is at least zero.
const PLANES: [Vec4; 6] = [
    Vec4::new(1.0, 0.0, 0.0, 1.0),
    Vec4::new(-1.0, 0.0, 0.0, 1.0),
    Vec4::new(0.0, 1.0, 0.0, 1.0),
    Vec4::new(0.0, -1.0, 0.0, 1.0),
    Vec4::new(0.0, 0.0, 1.0, 0.0),
    Vec4::new(0.0, 0.0, -1.0, 1.0),
];

/// Cuts the convex polygon `polygon[..count]` by the half-space where `plane` is at least zero,
/// in place (Sutherland and Hodgman), and returns its new number of vertices.
fn cut(polygon: &mut [Vec4; CLIPPED_VERTICES], count: usize, plane: Vec4) -> usize {
    let input = *polygon;
    let mut out = 0;
    for index in 0..count {
        let current = input[index];
        let next = input[(index + 1) % count];
        let (a, b) = (current.dot(plane), next.dot(plane));
        if a >= 0.0 {
            polygon[out] = current;
            out += 1;
        }
        if (a >= 0.0) != (b >= 0.0) && out < CLIPPED_VERTICES {
            polygon[out] = current + (next - current) * (a / (a - b));
            out += 1;
        }
    }
    out
}

fn mesh_of(draw: &Draw) -> Mesh {
    match (draw.flags >> 2) & 0b11 {
        0 => Mesh::Card,
        1 => Mesh::CardEdge,
        2 => Mesh::Quad,
        _ => Mesh::Outline,
    }
}
