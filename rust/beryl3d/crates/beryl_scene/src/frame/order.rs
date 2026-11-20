//! Putting a frame's draw list in drawing order and counting its load (docs/beryl-3d.md, "The
//! frame"): the opaque records first, cards nearest the camera first and the table last; then
//! the eight blended bands in order, farthest first within a band, and the flat layer's band in
//! the order of its tree. Nothing here allocates once the frame's arrays have their capacity:
//! the sort keys and the reordered records live in a scratch array the frame keeps.
//!
//! Depth order alone would cut a blended band into many draw calls wherever its records
//! alternate between meshes or texture arrays, as a fading card among the marks of the cards
//! around it does. So within each blended band of the table, `batch` then moves records
//! earlier, next to records drawn the same way, past only records they do not overlap on
//! screen. Two records that cover no pixel in common blend the same in either order, so the
//! picture is unchanged.

use glam::{Mat4, Vec2, Vec4};

use super::{BANDS, Band, Draw, Frame, Material, Mesh, sprites};
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
    batching: Batching,
    /// The call key of the first record pushed to each band, by `Band` value.
    first_calls: [Option<u8>; BANDS],
    /// A bit for each band, by `Band` value, holding records of more than one call key, which
    /// alone `batch` may need to reorder.
    mixed: u16,
}

/// What `batch` works in: for each record of one band, its rectangle on screen and whether it
/// has been placed, and the sort keys in their new order. Sized for every record of a frame.
#[derive(Clone, Debug, Default, PartialEq)]
struct Batching {
    rects: Vec<Rect>,
    additive: Vec<bool>,
    placed: Vec<bool>,
    /// Records passed over in the current run, which the records after them may not pass
    /// if they overlap.
    passed: Vec<u32>,
    keys: Vec<u64>,
}

impl Scratch {
    pub(super) fn with_capacity(draws: usize) -> Self {
        Self {
            keys: Vec::with_capacity(draws),
            draws: Vec::with_capacity(draws),
            batching: Batching {
                rects: Vec::with_capacity(draws),
                additive: Vec::with_capacity(draws),
                placed: Vec::with_capacity(draws),
                passed: Vec::with_capacity(draws),
                keys: Vec::with_capacity(draws),
            },
            first_calls: [None; BANDS],
            mixed: 0,
        }
    }

    pub(super) fn clear(&mut self) {
        self.keys.clear();
        self.draws.clear();
        self.first_calls = [None; BANDS];
        self.mixed = 0;
    }

    /// Notes that the record at `index` of the draw list, `draw`, belongs to `band`.
    pub(super) fn push(&mut self, band: Band, index: usize, draw: &Draw) {
        debug_assert!(
            (index as u64) <= INDEX_MASK,
            "a frame holds fewer than 2^24 records"
        );
        self.keys.push(((band as u64) << BAND_SHIFT) | index as u64);
        let first = self.first_calls[band as usize].get_or_insert(draw.flags);
        if *first != draw.flags {
            self.mixed |= 1 << band as u16;
        }
    }
}

/// Puts `frame.draws` in drawing order, sets `opaque`, `layer` and `bands`, and counts the
/// load, all but the screen areas, which `areas` measures when a gate asks for them.
pub(super) fn finish(frame: &mut Frame) {
    let view_projection = frame.view_projection;
    let Frame { draws, scratch, .. } = frame;
    for key in &mut scratch.keys {
        let draw = &draws[(*key & INDEX_MASK) as usize];
        *key |= u64::from(depth_key(band_of(*key), draw, view_projection)) << DEPTH_SHIFT;
    }
    scratch.keys.sort_unstable();
    batch_bands(scratch, draws, &view_projection);
    scratch.draws.extend(
        scratch
            .keys
            .iter()
            .map(|key| draws[(*key & INDEX_MASK) as usize]),
    );
    std::mem::swap(draws, &mut scratch.draws);
    if !frame.owners.is_empty() {
        let pushed = std::mem::take(&mut frame.owners);
        frame.owners = (scratch.keys.iter())
            .map(|key| pushed[(*key & INDEX_MASK) as usize])
            .collect();
    }

    let (opaque, layer) = set_bands(frame);

    let records = super::index(frame.draws.len());
    frame.load.instances = records;
    frame.load.upload_bytes =
        records * super::index(size_of::<Draw>()) + super::index(frame.paints.bytes().len());
    frame.load.draw_calls = draw_calls(&frame.draws, opaque, layer);
}

/// The screen areas of a finished frame's opaque and blended records. Only the gates and tests
/// read them, so the client measures them only when asked: measuring every record took about a
/// fifth of the instructions of a stress board's frame between scripts.
pub(super) fn areas(frame: &Frame) -> super::Areas {
    let opaque = frame.opaque as usize;
    let mut areas = super::Areas::default();
    for (index, draw) in frame.draws.iter().enumerate() {
        let area = screen_area(draw, &frame.view_projection);
        if index < opaque {
            areas.opaque += area;
        } else {
            areas.blended += area;
        }
    }
    areas
}

/// Sets `frame.bands`, `opaque` and `layer` from its sort keys, which are in drawing order, and
/// returns `opaque` and `layer`.
fn set_bands(frame: &mut Frame) -> (usize, usize) {
    for (band, start) in frame.bands.iter_mut().enumerate() {
        *start = super::index(
            (frame.scratch.keys).partition_point(|&key| (band_of(key) as usize) < band),
        );
    }
    let (opaque, layer) = (
        frame.bands[Band::ContactShadows as usize] as usize,
        frame.bands[Band::Layer as usize] as usize,
    );
    frame.opaque = super::index(opaque);
    frame.layer = super::index(layer);
    if let Some(backdrop) = &mut frame.backdrop {
        // Within the flat layer's band, records keep their push order (`depth_key`), so the
        // push indexes rise.
        let layer_end = frame.bands[Band::LayerCards as usize] as usize;
        let before = frame.scratch.keys[layer..layer_end]
            .partition_point(|key| key & INDEX_MASK < u64::from(backdrop.pushed));
        backdrop.at = super::index(layer + before);
    }
    (opaque, layer)
}

impl Frame {
    /// Keeps only the records of a finished, `looking` frame whose element (`Frame::owners`)
    /// `keep` accepts, by its index in `resolved`, so that the frame draws those elements
    /// alone. A record that belongs to no element is dropped. The frame's load is left as it
    /// was. `just b3d png --only` draws one element's subtree this way.
    pub fn keep_only(&mut self, keep: impl Fn(u32) -> bool) {
        let kept: Vec<bool> = (0..self.draws.len())
            .map(|at| {
                self.owners
                    .get(at)
                    .is_some_and(|&owner| owner != u32::MAX && keep(owner))
            })
            .collect();
        self.keep_kept(&kept);
    }

    /// Keeps only the records of a finished frame that `keep` accepts, by their index in
    /// `draws`, so that the frame draws without the others. The frame's load is left as it
    /// was. The light test draws each arrow and fight mark's frame without its records this
    /// way, to see that they changed the picture.
    pub fn keep_records(&mut self, keep: impl Fn(usize) -> bool) {
        let kept: Vec<bool> = (0..self.draws.len()).map(keep).collect();
        self.keep_kept(&kept);
    }

    /// Keeps the records of `draws` whose entry of `kept` is true, with their sort keys and
    /// owners, and sets the bands again.
    fn keep_kept(&mut self, kept: &[bool]) {
        let mut at = 0;
        self.draws.retain(|_| (kept[at], at += 1).0);
        at = 0;
        self.scratch.keys.retain(|_| (kept[at], at += 1).0);
        at = 0;
        self.owners.retain(|_| (kept[at], at += 1).0);
        set_bands(self);
    }

    /// The band of the record at `index` of `draws`, once the frame is finished.
    #[must_use]
    pub fn band(&self, index: usize) -> Option<Band> {
        self.scratch.keys.get(index).map(|&key| band_of(key))
    }

    /// The screen area of the opaque and of the blended records of the finished frame, each
    /// divided by the screen's: the rectangle that bounds each record's mesh, cut to the view
    /// volume. The one-pixel edge strip counts as no area, and clips are not taken into
    /// account.
    #[must_use]
    pub fn areas(&self) -> super::Areas {
        areas(self)
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
        5 => Band::Particles,
        6 => Band::Layer,
        7 => Band::LayerCards,
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

/// How far a record's rectangle on screen is grown on each side before two are compared, in
/// normalized device coordinates, where the screen is 2 across: more than the one pixel the
/// soft edge of a card reaches outside the card, in any window 200 or more pixels wide.
const OVERLAP_MARGIN: f32 = 0.01;

/// A rectangle on screen in normalized device coordinates: left, bottom, right, top. An empty
/// one has its left past its right.
type Rect = [f32; 4];

const EMPTY: Rect = [1.0, 1.0, -1.0, -1.0];

fn overlap(a: &Rect, b: &Rect) -> bool {
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}

/// Batches the records of each blended band of the table (`batch`). The flat layer's bands
/// keep the order of their tree.
fn batch_bands(scratch: &mut Scratch, draws: &[Draw], view_projection: &Mat4) {
    let Scratch {
        keys,
        batching,
        mixed,
        ..
    } = scratch;
    let mut start = keys.partition_point(|&key| band_of(key) == Band::Opaque);
    let end = keys.partition_point(|&key| band_of(key) < Band::Layer);
    while start < end {
        let band = band_of(keys[start]);
        let len = keys[start..end].partition_point(|&key| band_of(key) == band);
        if *mixed & (1 << band as u16) != 0 {
            batch(
                &mut keys[start..start + len],
                draws,
                view_projection,
                batching,
            );
        }
        start += len;
    }
}

/// Whether `draw` only adds its light to what is beneath it: the glow and streak materials
/// shade a color with no opacity, which the premultiplied blend adds to the target. (On an
/// 8-bit target each addition is rounded, so two such records in the other order can differ in
/// the last bit.)
fn adds(draw: &Draw) -> bool {
    draw.material == Material::Glow as u8 || draw.material == Material::Streak as u8
}

/// The part of the key a draw call is cut on: the record's mesh, texture array and clip.
fn call_key(key: u64, draws: &[Draw]) -> u8 {
    draws[(key & INDEX_MASK) as usize].flags
}

/// Whether every call key in `keys` comes in one unbroken run already, as in a band whose
/// records are all drawn alike, so that reordering would save no call.
fn grouped(keys: &[u64], draws: &[Draw]) -> bool {
    // A bit for each call key whose run has ended.
    let mut ended = [0_u64; 4];
    let mut previous = None;
    for &key in keys {
        let call = call_key(key, draws);
        if previous == Some(call) {
            continue;
        }
        let (word, bit) = (usize::from(call / 64), call % 64);
        if ended[word] & (1 << bit) != 0 {
            return false;
        }
        if let Some(last) = previous {
            ended[usize::from(last / 64)] |= 1 << (last % 64);
        }
        previous = Some(call);
    }
    true
}

/// Reorders `keys`, the sort keys of one band in drawing order, so that records drawn alike
/// come together, without changing the order of any two records that overlap on screen.
///
/// It places records in runs. A run takes the call key of the first record not yet placed,
/// then goes through the records after it in order and places each one with that call key
/// that overlaps none of the records it passed over. The next run starts from the first record
/// still not placed. A record is never placed before one it overlaps that was before it, unless
/// both add their light to what is beneath them (`additive`): sums come out the same in any
/// order.
fn batch(keys: &mut [u64], draws: &[Draw], view_projection: &Mat4, batching: &mut Batching) {
    if grouped(keys, draws) {
        return;
    }
    let Batching {
        rects,
        additive,
        placed,
        passed,
        keys: out,
    } = batching;
    rects.clear();
    additive.clear();
    placed.clear();
    out.clear();
    for &key in keys.iter() {
        let draw = &draws[(key & INDEX_MASK) as usize];
        rects.push(screen_rect(draw, view_projection));
        additive.push(adds(draw));
        placed.push(false);
    }
    let mut first = 0;
    while first < keys.len() {
        let call = call_key(keys[first], draws);
        passed.clear();
        for index in first..keys.len() {
            if placed[index] {
                continue;
            }
            let (rect, adding) = (&rects[index], additive[index]);
            let free = call_key(keys[index], draws) == call
                && passed.iter().all(|&other| {
                    let other = other as usize;
                    (adding && additive[other]) || !overlap(&rects[other], rect)
                });
            if free {
                placed[index] = true;
                out.push(keys[index]);
            } else {
                passed.push(super::index(index));
            }
        }
        while first < keys.len() && placed[first] {
            first += 1;
        }
    }
    keys.copy_from_slice(out);
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

/// Half the size of the rectangle that bounds `mesh` in its own space.
fn bounds(mesh: Mesh) -> Vec2 {
    match mesh {
        Mesh::Card | Mesh::CardEdge => Vec2::new(CARD_WIDTH, CARD_HEIGHT) / 2.0,
        Mesh::Quad => Vec2::splat(0.5),
        Mesh::Outline => Vec2::from(sprites::OUTLINE_OUTER),
    }
}

/// The part of the screen `draw` covers, as a fraction of the screen's area. The record's
/// footprint is the rectangle that bounds its mesh, cut to the view volume, less the outline
/// mesh's empty middle. The rounded corners of a card are counted as covered. The edge strip is
/// one pixel wide, so it is counted as covering nothing. A clip rectangle is not taken into
/// account.
fn screen_area(draw: &Draw, view_projection: &Mat4) -> f32 {
    match mesh_of(draw) {
        Mesh::CardEdge => 0.0,
        Mesh::Outline => {
            rect_area(draw, view_projection, bounds(Mesh::Outline))
                - rect_area(draw, view_projection, Vec2::from(sprites::OUTLINE_HOLE))
        }
        mesh => rect_area(draw, view_projection, bounds(mesh)),
    }
}

/// The rectangle on screen that bounds `draw`'s mesh, grown by `OVERLAP_MARGIN` on each side,
/// or `EMPTY` when none of it is on screen. A record wholly in front of the camera is bounded by
/// its four corners as projected; only one that reaches behind it is cut to the view volume
/// first. Depth and a clip rectangle are not taken into account, so the rectangle may be larger
/// than what is drawn, never smaller.
fn screen_rect(draw: &Draw, view_projection: &Mat4) -> Rect {
    let half = bounds(mesh_of(draw));
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
