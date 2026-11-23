//! Putting a frame's draw list in drawing order and counting its load (docs/beryl-3d.md, "The
//! frame"): the opaque records first, cards nearest the camera first and the table last; then
//! the blended bands in order, farthest first within a band, and the flat layer's band in
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

mod footprint;

use self::footprint::{Rect, overlap, screen_area, screen_rect};
use super::{BANDS, Band, Draw, Frame, FrameBuilder, Material, Resolved, Role, StateBits};

/// The index breaks sort ties, so records at the same depth keep a deterministic order.
const INDEX_BITS: u32 = 24;
const INDEX_MASK: u64 = (1 << INDEX_BITS) - 1;
/// The low bit of a record key selects the opaque overlay pipeline without changing its order.
const OPAQUE_LAYER_CARD: u64 = 1;
const RECORD_INDEX_SHIFT: u32 = 1;
/// The 32 bits above the index order records within a band by depth.
const DEPTH_SHIFT: u32 = INDEX_BITS + RECORD_INDEX_SHIFT;
/// The top bits hold the band, so the bands come in their declared order.
const BAND_SHIFT: u32 = 60;

/// The scene's paint bands, in back-to-front order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PaintBand {
    Table,
    Moving,
    ScreenPanels,
    ShownObjects,
    Overlays,
    LayerControls,
    Dragged,
    Foreground,
}

impl PaintBand {
    /// Assigns a node's band from its space and current state.
    #[must_use]
    pub fn of(node: &Resolved) -> Self {
        match node.pose {
            Some(_) if node.state.has(StateBits::HELD) => Self::Dragged,
            // While a card moves into a preview, the screen's controls stay above it.
            Some(_)
                if node.state.has(StateBits::MOVING)
                    && !node.state.has(StateBits::LAYOUT_MOTION) =>
            {
                Self::Moving
            }
            Some(_) if node.state.has(StateBits::SHOWN) || node.state.has(StateBits::RAISED) => {
                Self::ShownObjects
            }
            Some(_) => Self::Table,
            None if node.state.has(StateBits::FOREGROUND) => Self::Foreground,
            None if node.state.has(StateBits::ABOVE_CARDS) => Self::LayerControls,
            None if node.role == Role::Arrow => Self::Overlays,
            None => Self::ScreenPanels,
        }
    }

    /// The record band of a scene node. Resting table records retain their individual
    /// opaque and blended passes.
    #[must_use]
    pub const fn records(self, resting: Band) -> Band {
        match self {
            Self::Moving if matches!(resting, Band::Opaque) => Band::MovingOpaque,
            Self::Moving => Band::Moving,
            Self::ShownObjects => Band::LayerCards,
            Self::Dragged => Band::Dragged,
            Self::Foreground => Band::Foreground,
            Self::Overlays => Band::LayerCover,
            Self::LayerControls => Band::LayerControls,
            Self::ScreenPanels => Band::Layer,
            Self::Table => resting,
        }
    }
}

/// The ordering of scene nodes follows the record bands and the geometry of each space.
/// Screen nodes keep tree order and projected objects paint back to front.
fn node_key(index: u32, element: &Resolved) -> u64 {
    let depth = if element.pose.is_some() {
        !ordered(element.depth)
    } else {
        0
    };
    ((PaintBand::of(element) as u64) << BAND_SHIFT)
        | (u64::from(depth) << DEPTH_SHIFT)
        | u64::from(index)
}

/// Sort keys for the nodes and draw records, with room to reorder the records. The frame
/// reserves this storage when it is made.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Scratch {
    keys: Vec<u64>,
    draws: Vec<Draw>,
    nodes: Vec<u64>,
    /// The render band of each registered card or ability's face, by node index.
    card_bands: Vec<Option<Band>>,
    batching: Batching,
    /// The call key of the first record pushed to each band, by `Band` value.
    first_calls: [Option<u16>; BANDS],
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
    pub(super) fn with_capacity(draws: usize, nodes: usize) -> Self {
        Self {
            keys: Vec::with_capacity(draws),
            draws: Vec::with_capacity(draws),
            nodes: Vec::with_capacity(nodes),
            card_bands: Vec::with_capacity(nodes),
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
        self.nodes.clear();
        self.card_bands.clear();
        self.first_calls = [None; BANDS];
        self.mixed = 0;
    }

    /// Notes that the record at `index` of the draw list, `draw`, belongs to `band`.
    pub(super) fn push(&mut self, band: Band, index: usize, draw: &Draw) {
        debug_assert!(
            (index as u64) <= INDEX_MASK,
            "a frame holds fewer than 2^24 records"
        );
        let tie = record_tie(band, index as u64) << RECORD_INDEX_SHIFT;
        self.keys.push(((band as u64) << BAND_SHIFT) | tie);
        let call = record_call_key(draw);
        let first = self.first_calls[band as usize].get_or_insert(call);
        if *first != call {
            self.mixed |= 1 << band as u16;
        }
    }
}

/// Puts `frame.draws` in drawing order, sets `opaque`, `layer` and `bands`, and counts the
/// load, all but the screen areas, which `areas` measures when a gate asks for them.
pub(super) fn finish(frame: &mut Frame) {
    let (view_projection, card) = (frame.view_projection, frame.card);
    let Frame { draws, scratch, .. } = frame;
    for key in &mut scratch.keys {
        let draw = &draws[record_index(*key)];
        *key |= u64::from(depth_key(band_of(*key), draw, view_projection)) << DEPTH_SHIFT;
    }
    scratch.keys.sort_unstable();
    batch_bands(scratch, draws, (&view_projection, card));
    scratch
        .draws
        .extend(scratch.keys.iter().map(|key| draws[record_index(*key)]));
    std::mem::swap(draws, &mut scratch.draws);
    if !frame.owners.is_empty() {
        let pushed = std::mem::take(&mut frame.owners);
        frame.owners = (scratch.keys.iter())
            .map(|key| pushed[record_index(*key)])
            .collect();
    }

    frame.order_nodes();
    set_bands(frame);

    let records = super::index(frame.draws.len());
    frame.load.instances = records;
    frame.load.upload_bytes =
        records * super::index(size_of::<Draw>()) + super::index(frame.paints.bytes().len());
    frame.load.draw_calls = draw_calls(frame);
}

/// The screen areas of a finished frame's opaque and blended records. Only the gates and tests
/// read them, so the client measures them only when asked: measuring every record took about a
/// fifth of the instructions of a stress board's frame between scripts.
pub(super) fn areas(frame: &Frame) -> super::Areas {
    let opaque = frame.opaque as usize;
    let shadows_end = frame.bands[Band::TableMarks as usize] as usize;
    let moving_opaque = frame.bands[Band::MovingOpaque as usize] as usize
        ..frame.bands[Band::Moving as usize] as usize;
    let mut areas = super::Areas::default();
    for (index, draw) in frame.draws.iter().enumerate() {
        let area = screen_area(draw, &frame.view_projection, frame.card);
        if index < opaque || moving_opaque.contains(&index) || frame.opaque_layer_card(index) {
            areas.opaque += area;
        } else {
            areas.blended += area;
            if index < shadows_end {
                areas.contact_shadows += area;
            } else if index < frame.layer as usize {
                areas.scene_blended += area;
            } else {
                areas.layer_blended += area;
            }
        }
    }
    areas
}

/// Sets `frame.bands`, `opaque` and `layer` from its sort keys, which are in drawing order.
fn set_bands(frame: &mut Frame) {
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
            .partition_point(|&key| record_index(key) < backdrop.pushed as usize);
        backdrop.at = super::index(layer + before);
    }
}

impl Frame {
    pub(super) fn order_nodes(&mut self) {
        self.scratch.nodes.clear();
        self.scratch.nodes.extend(
            self.resolved
                .iter()
                .enumerate()
                .map(|(index, element)| node_key(super::index(index), element)),
        );
        self.scratch.nodes.sort_unstable();
    }

    /// Node indexes in back-to-front paint order. The tree's registration order is retained
    /// separately for walking parent and child relationships.
    pub fn paint_order(
        &self,
    ) -> impl DoubleEndedIterator<Item = u32> + ExactSizeIterator + Clone + '_ {
        self.scratch
            .nodes
            .iter()
            .map(|key| (key & INDEX_MASK) as u32)
    }

    /// Compares two registered nodes in paint order.
    #[must_use]
    pub fn compare_paint(&self, a: u32, b: u32) -> std::cmp::Ordering {
        node_key(a, &self.resolved[a as usize]).cmp(&node_key(b, &self.resolved[b as usize]))
    }

    pub(super) fn depth_card_band(&self, index: u32) -> Option<Band> {
        if self.resolved[index as usize].state.has(StateBits::RAISED) {
            return None;
        }
        self.scratch
            .card_bands
            .get(index as usize)
            .copied()
            .flatten()
            .filter(|&band| band < Band::Layer)
    }

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
        if self.backdrop.is_some_and(|backdrop| {
            backdrop.panel.is_some() && !kept.get(backdrop.at as usize).copied().unwrap_or(false)
        }) {
            self.backdrop = None;
        }
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

    /// Whether this record paints over the flat layer with blending disabled.
    #[must_use]
    pub fn opaque_layer_card(&self, index: usize) -> bool {
        self.scratch
            .keys
            .get(index)
            .is_some_and(|key| key & OPAQUE_LAYER_CARD != 0)
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

impl FrameBuilder<'_> {
    /// Ends the last element's draw records before emitting light or effects with no node.
    pub fn clear_owner(&mut self) {
        self.owner = u32::MAX;
    }

    /// Assigns a flat subtree's nodes and draw records to the requested layer band.
    pub fn with_layer_band<T>(&mut self, band: Band, write: impl FnOnce(&mut Self) -> T) -> T {
        let previous = self.layer_band;
        self.layer_band = band.max(previous);
        let result = write(self);
        self.layer_band = previous;
        result
    }

    /// The paint band of the element at `index` in the registered tree.
    #[must_use]
    pub fn node_band(&self, index: u32) -> PaintBand {
        PaintBand::of(&self.frame.resolved[index as usize])
    }

    /// Draws the card for the element just registered, using its scene band. At rest an
    /// opaque card writes depth, while a fading or dissolving one blends with its marks.
    pub fn draw_card(&mut self, at: u32, resting: Band, draw: Draw) {
        let band = self.node_band(at).records(resting);
        let bands = &mut self.frame.scratch.card_bands;
        bands.resize(bands.len().max(at as usize + 1), None);
        // The pointer uses the depth pass of the card in the fan when it swaps in the rest pose.
        let raised = self.frame.resolved[at as usize]
            .state
            .has(StateBits::RAISED);
        bands[at as usize] = Some(if raised { resting } else { band });
        self.draw(band, draw);
        // Keep the face among the raised marks in paint order. A clipped card may have
        // a soft mask, so it still uses the blended pipeline.
        if raised && band == Band::LayerCards && resting == Band::Opaque && draw.clip() == 0 {
            let index = self.frame.draws.len() - 1;
            self.frame.scratch.keys[index] |= OPAQUE_LAYER_CARD;
        }
    }
}

/// Opaque passes use a strict depth test. Submit the last registered card first at equal
/// depth so it wins the tie, just as it does when hit testing visits nodes front to back.
fn record_tie(band: Band, index: u64) -> u64 {
    match band {
        Band::Opaque | Band::MovingOpaque => INDEX_MASK ^ index,
        _ => index,
    }
}

fn record_index(key: u64) -> usize {
    record_tie(band_of(key), (key >> RECORD_INDEX_SHIFT) & INDEX_MASK) as usize
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
        6 => Band::MovingOpaque,
        7 => Band::Moving,
        8 => Band::Layer,
        9 => Band::LayerCards,
        10 => Band::LayerCover,
        11 => Band::LayerControls,
        12 => Band::Dragged,
        _ => Band::Foreground,
    }
}

/// The 32 bits that order a record within its band, smallest first.
fn depth_key(band: Band, draw: &Draw, view_projection: Mat4) -> u32 {
    match band {
        // The table comes after every card, so that a pixel a card covers is never shaded.
        Band::Opaque if draw.material == Material::Table as u8 => u32::MAX,
        Band::Opaque | Band::MovingOpaque => ordered(depth(draw, view_projection)),
        // The flat layer is drawn in the order of its tree, which is the order it was pushed.
        Band::Layer | Band::LayerCover | Band::LayerControls | Band::Foreground => 0,
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
pub(super) fn ordered(value: f32) -> u32 {
    let bits = value.to_bits();
    if bits & (1 << 31) == 0 {
        bits | (1 << 31)
    } else {
        !bits
    }
}

/// How many instanced draw calls the renderer makes for `draws`: one for each run of
/// consecutive records with the same mesh, texture array and clip. The renderer also starts a
/// new call wherever the pipeline changes.
fn draw_calls(frame: &Frame) -> u32 {
    let moving_opaque = frame.bands[Band::MovingOpaque as usize];
    let moving = frame.bands[Band::Moving as usize];
    let pass = |index: u32| {
        u8::from(index >= frame.opaque)
            + u8::from(index >= moving_opaque)
            + u8::from(index >= moving)
            + u8::from(index >= frame.layer)
    };
    let mut calls = 0;
    let mut previous = None;
    for (index, (draw, &key)) in frame.draws.iter().zip(&frame.scratch.keys).enumerate() {
        let key = (
            pass(super::index(index)),
            key & OPAQUE_LAYER_CARD,
            record_call_key(draw),
        );
        if previous != Some(key) {
            calls += 1;
            previous = Some(key);
        }
    }
    calls
}

/// Batches the records of each blended band of the table (`batch`). The flat layer's bands
/// keep the order of their tree.
fn batch_bands(scratch: &mut Scratch, draws: &[Draw], camera: (&Mat4, Vec2)) {
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
        if band != Band::MovingOpaque && *mixed & (1 << band as u16) != 0 {
            batch(&mut keys[start..start + len], draws, camera, batching);
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

/// The part of the key a draw call is cut on: the record's mesh, texture array, clip and
/// whether it uses the dedicated Enhanced VFX pipeline.
const fn record_call_key(draw: &Draw) -> u16 {
    (draw.flags as u16) | ((draw.is_vfx() as u16) << 8)
}

/// The call key of the record named by `key`.
fn call_key(key: u64, draws: &[Draw]) -> u16 {
    record_call_key(&draws[record_index(key)])
}

/// Whether every call key in `keys` comes in one unbroken run already, as in a band whose
/// records are all drawn alike, so that reordering would save no call.
fn grouped(keys: &[u64], draws: &[Draw]) -> bool {
    // A bit for each of the 512 possible call keys whose run has ended.
    let mut ended = [0_u64; 8];
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
fn batch(
    keys: &mut [u64],
    draws: &[Draw],
    (view_projection, card): (&Mat4, Vec2),
    batching: &mut Batching,
) {
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
        let draw = &draws[record_index(key)];
        rects.push(screen_rect(draw, view_projection, card));
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
