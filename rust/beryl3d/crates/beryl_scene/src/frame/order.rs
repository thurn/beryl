//! Putting a frame's draw list in drawing order and counting its load (docs/beryl-3d.md, "The
//! frame"): the opaque records first, cards nearest the camera first and the table last; then
//! the seven blended bands in order, farthest first within a band, and the flat layer's band in
//! the order of its tree. Nothing here allocates once the frame's arrays have their capacity:
//! the sort keys and the reordered records live in a scratch array the frame keeps.

use glam::{Mat4, Vec2, Vec4};

use super::{Band, Draw, Frame, Material, Mesh};
use crate::table::{CARD_HEIGHT, CARD_WIDTH};

/// The low bits of a sort key hold the record's index in push order, so that records that tie
/// keep the order they were pushed in, and the sort is the same on every machine.
const INDEX_BITS: u32 = 24;
const INDEX_MASK: u64 = (1 << INDEX_BITS) - 1;
/// The 32 bits above the index order records within a band by depth.
const DEPTH_SHIFT: u32 = INDEX_BITS;
/// The top bits hold the band, so the bands come in their declared order.
const BAND_SHIFT: u32 = 60;

/// What ordering needs between frames: one sort key per record and room for the records in
/// their new order. Both are sized when the frame is made.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Scratch {
    keys: Vec<u64>,
    draws: Vec<Draw>,
}

impl Scratch {
    pub(super) fn with_capacity(draws: usize) -> Self {
        Self {
            keys: Vec::with_capacity(draws),
            draws: Vec::with_capacity(draws),
        }
    }

    pub(super) fn clear(&mut self) {
        self.keys.clear();
        self.draws.clear();
    }

    /// Notes that the record at `index` of the draw list belongs to `band`.
    pub(super) fn push(&mut self, band: Band, index: usize) {
        debug_assert!(
            (index as u64) <= INDEX_MASK,
            "a frame holds fewer than 2^24 records"
        );
        self.keys.push(((band as u64) << BAND_SHIFT) | index as u64);
    }
}

/// Puts `frame.draws` in drawing order, sets `opaque` and `layer`, and counts the load.
pub(super) fn finish(frame: &mut Frame) {
    let view_projection = frame.view_projection;
    let Frame {
        draws,
        scratch,
        load,
        ..
    } = frame;
    for key in &mut scratch.keys {
        let draw = &draws[(*key & INDEX_MASK) as usize];
        *key |= u64::from(depth_key(band_of(*key), draw, view_projection)) << DEPTH_SHIFT;
    }
    scratch.keys.sort_unstable();
    scratch.draws.extend(
        scratch
            .keys
            .iter()
            .map(|key| draws[(*key & INDEX_MASK) as usize]),
    );
    std::mem::swap(draws, &mut scratch.draws);

    let opaque = scratch
        .keys
        .partition_point(|&key| band_of(key) == Band::Opaque);
    let layer = scratch
        .keys
        .partition_point(|&key| band_of(key) < Band::Layer);
    frame.opaque = super::index(opaque);
    frame.layer = super::index(layer);

    let records = super::index(frame.draws.len());
    load.instances = records;
    load.upload_bytes = records * super::index(size_of::<Draw>());
    load.draw_calls = draw_calls(&frame.draws, opaque, layer);
    for (index, draw) in frame.draws.iter().enumerate() {
        let area = screen_area(draw, view_projection);
        if index < opaque {
            load.opaque_area += area;
        } else {
            load.blended_area += area;
        }
    }
}

impl Frame {
    /// The band of the record at `index` of `draws`, once the frame is finished.
    #[must_use]
    pub fn band(&self, index: usize) -> Option<Band> {
        self.scratch.keys.get(index).map(|&key| band_of(key))
    }
}

/// The band a sort key holds.
fn band_of(key: u64) -> Band {
    match key >> BAND_SHIFT {
        0 => Band::Opaque,
        1 => Band::ContactShadows,
        2 => Band::TableMarks,
        3 => Band::RaisedMarks,
        4 => Band::Glows,
        5 => Band::Layer,
        6 => Band::LayerCards,
        _ => Band::LayerCover,
    }
}

/// The 32 bits that order a record within its band, smallest first.
fn depth_key(band: Band, draw: &Draw, view_projection: Mat4) -> u32 {
    match band {
        // The table comes after every card, so that a pixel a card covers is never shaded.
        Band::Opaque if draw.material == Material::Table as u8 => u32::MAX,
        Band::Opaque => ordered(depth(draw, view_projection)),
        // The flat layer is drawn in the order of its tree, which is the order it was pushed.
        Band::Layer => 0,
        // Blended records are drawn farthest first, so that nearer ones blend over them.
        _ => !ordered(depth(draw, view_projection)),
    }
}

/// The depth of a record's origin after projection: 0 at the near plane and 1 at the far one.
fn depth(draw: &Draw, view_projection: Mat4) -> f32 {
    let m = &draw.model;
    let clip = view_projection * Vec4::new(m[3], m[7], m[11], 1.0);
    clip.z / clip.w
}

/// `value`'s bits, rearranged so that comparing them as unsigned integers orders the floats
/// as `f32::total_cmp` does.
fn ordered(value: f32) -> u32 {
    let bits = value.to_bits();
    if bits & (1 << 31) == 0 {
        bits | (1 << 31)
    } else {
        !bits
    }
}

/// How many instanced draw calls the renderer makes for `draws`: one for each run of
/// consecutive records with the same mesh, texture array and clip. The renderer also starts a
/// new call where the opaque records end and where the flat layer's begin, because each of the
/// three is drawn with its own pipeline.
fn draw_calls(draws: &[Draw], opaque: usize, layer: usize) -> u32 {
    let pass = |index: usize| u8::from(index >= opaque) + u8::from(index >= layer);
    let mut calls = 0;
    let mut previous = None;
    for (index, draw) in draws.iter().enumerate() {
        // `flags` holds exactly the texture array, the mesh and the clip.
        let key = (pass(index), draw.flags);
        if previous != Some(key) {
            calls += 1;
            previous = Some(key);
        }
    }
    calls
}

/// The most vertices a quad has after being cut by the six planes of the view volume: each
/// plane adds at most one.
const CLIPPED_VERTICES: usize = 4 + 6;

/// The part of the screen `draw` covers, as a fraction of the screen's area. The record's
/// footprint is the rectangle that bounds its mesh, cut to the view volume. The rounded corners
/// of a card are counted as covered. The edge strip is one pixel wide, so it is counted as
/// covering nothing. A clip rectangle is not taken into account.
fn screen_area(draw: &Draw, view_projection: Mat4) -> f32 {
    let half = match mesh_of(draw) {
        Mesh::Card => Vec2::new(CARD_WIDTH, CARD_HEIGHT) / 2.0,
        Mesh::Quad => Vec2::splat(0.5),
        Mesh::CardEdge => return 0.0,
    };
    let m = &draw.model;
    let corner = |x: f32, y: f32| {
        view_projection
            * Vec4::new(
                m[0] * x + m[1] * y + m[3],
                m[4] * x + m[5] * y + m[7],
                m[8] * x + m[9] * y + m[11],
                1.0,
            )
    };
    let mut polygon = [Vec4::ZERO; CLIPPED_VERTICES];
    polygon[..4].copy_from_slice(&[
        corner(-half.x, -half.y),
        corner(half.x, -half.y),
        corner(half.x, half.y),
        corner(-half.x, half.y),
    ]);
    let mut count = 4;
    // The view volume in clip space, with depth from 0 to w: each plane keeps the points where
    // the dot product with it is at least zero.
    let planes = [
        Vec4::new(1.0, 0.0, 0.0, 1.0),
        Vec4::new(-1.0, 0.0, 0.0, 1.0),
        Vec4::new(0.0, 1.0, 0.0, 1.0),
        Vec4::new(0.0, -1.0, 0.0, 1.0),
        Vec4::new(0.0, 0.0, 1.0, 0.0),
        Vec4::new(0.0, 0.0, -1.0, 1.0),
    ];
    for plane in planes {
        count = cut(&mut polygon, count, plane);
        if count < 3 {
            return 0.0;
        }
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
        _ => Mesh::Quad,
    }
}
