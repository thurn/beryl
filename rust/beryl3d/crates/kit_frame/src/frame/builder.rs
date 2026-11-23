//! Building a frame (`FrameBuilder`) without allocating once the frame's arrays have their
//! capacity.

use glam::{Mat4, Vec2};
use kit_base::atlas::Tier;
use kit_base::geometry::Rect;
use kit_base::paint::Shadow;

use super::{
    BANDS, Band, CLIPS, Draw, ElementId, ElementLook, Frame, Load, Material, Missing, NameRef,
    PARTICLES, Paints, RINGS, Resolved, Ring, Sound, StateBits, TEXT_RECORDS, TextLine, Undrawn,
    Work, index, order,
};

/// What `Client::frame` writes a frame into. Making one clears the frame, keeping its capacity.
/// Records may be pushed in any order: `finish` puts them in drawing order.
pub struct FrameBuilder<'a> {
    pub(super) frame: &'a mut Frame,
    pub(super) clips: u8,
    /// The element added last, which the records pushed after it belong to (`Frame::owners`).
    pub(super) owner: u32,
    pub(super) layer_band: Band,
}

impl<'a> FrameBuilder<'a> {
    /// Starts a frame drawn in `tier`, whose card mesh is `card` millimetres wide and high.
    pub fn new(frame: &'a mut Frame, tier: Tier, card: Vec2) -> Self {
        frame.resolved.clear();
        frame.shows.clear();
        frame.anchors.clear();
        frame.anchor_nodes.clear();
        frame.drop_regions.clear();
        frame.names.clear();
        frame.draws.clear();
        frame.opaque = 0;
        frame.layer = 0;
        frame.bands = [0; BANDS + 1];
        frame.tier = tier;
        frame.ambient_motion = false;
        frame.view_projection = Mat4::IDENTITY;
        frame.card = card;
        frame.clips = [Rect::default(); CLIPS];
        frame.clips_full = false;
        frame.paints = Paints::default();
        frame.backdrop = None;
        frame.shake = Vec2::ZERO;
        frame.sounds.clear();
        frame.lines.clear();
        frame.looks.clear();
        frame.owners.clear();
        frame.undrawn.clear();
        frame.lifted.clear();
        frame.decorations.clear();
        frame.rings.clear();
        frame.load = Load::default();
        frame.work = Work::default();
        frame.text_room = TEXT_RECORDS;
        frame.scratch.clear();
        Self {
            frame,
            clips: 0,
            owner: u32::MAX,
            layer_band: Band::Layer,
        }
    }

    /// The visual tier the frame is drawn in, which decides the records only one tier writes:
    /// contact shadows in Baseline, particles and procedural VFX in Enhanced.
    #[must_use]
    pub fn tier(&self) -> Tier {
        self.frame.tier
    }

    /// Adds `element` named `name`, and returns its index. The builder sets `element.name`.
    ///
    /// # Panics
    ///
    /// If the parent is neither the root sentinel nor an already registered node.
    pub fn resolved(&mut self, name: &str, mut element: Resolved) -> u32 {
        let at = index(self.frame.resolved.len());
        assert!(
            element.parent == u32::MAX || element.parent < at,
            "a scene parent must be registered before its child"
        );
        element.state.0 |= match self.layer_band {
            Band::LayerControls => StateBits::ABOVE_CARDS,
            Band::Foreground => StateBits::FOREGROUND,
            _ => 0,
        };
        element.name = NameRef {
            start: index(self.frame.names.len()),
            len: index(name.len()),
        };
        self.frame.names.push_str(name);
        self.frame.resolved.push(element);
        self.owner = at;
        at
    }

    /// The centre of the table's element `id` as written so far (`Frame::centre`).
    #[must_use]
    pub fn centre(&self, id: ElementId) -> Option<Vec2> {
        self.frame.centre(id)
    }

    /// The corners of the table's element `id` as written so far (`Frame::corners`).
    #[must_use]
    pub fn corners(&self, id: ElementId) -> Option<[Vec2; 4]> {
        self.frame.corners(id)
    }

    /// Gives the resolved element at `at` the text of one annotation.
    ///
    /// # Panics
    ///
    /// If `at` or `annotation` is out of bounds.
    pub fn annotate(&mut self, at: u32, annotation: usize, text: &str) {
        let name = NameRef {
            start: index(self.frame.names.len()),
            len: index(text.len()),
        };
        self.frame.names.push_str(text);
        self.frame.resolved[at as usize].annotations[annotation] = name;
    }

    /// Takes `records` of the frame's `TEXT_RECORDS` for the lines of text on a card's face,
    /// and says whether there were that many left. A line that does not fit is left out.
    pub fn take_text_room(&mut self, records: usize) -> bool {
        let fits = records <= self.frame.text_room;
        if fits {
            self.frame.text_room -= records;
        }
        fits
    }

    /// Counts an element visited: an object slot of the table or a node of the flat layer.
    /// Every walk over elements calls it once for each, so that a second walk shows in
    /// `Work::elements`.
    pub fn visit(&mut self) {
        self.frame.work.elements += 1;
    }

    /// Counts an animation segment evaluated for an object's pose.
    pub fn segment(&mut self) {
        self.frame.work.segments += 1;
    }

    /// A card's printed width and height in millimetres, the size of the card mesh records are
    /// drawn with (`Frame::card`).
    #[must_use]
    pub fn card(&self) -> Vec2 {
        self.frame.card
    }

    /// Sets the camera's view and projection, `Frame::view_projection`, which `finish` also uses
    /// to order the records by depth and to measure their screen area.
    pub fn camera(&mut self, view_projection: Mat4) {
        self.frame.view_projection = view_projection;
    }

    /// How many particle records remain within the frame's `PARTICLES` ceiling.
    #[must_use]
    pub fn particle_room(&self) -> usize {
        PARTICLES.saturating_sub(self.frame.load.particles as usize)
    }

    /// Adds a record to `band`. Within the flat layer's band, records are drawn in the order
    /// they are pushed, which is the order of its tree.
    pub fn draw(&mut self, band: Band, draw: Draw) {
        let particle = draw.material == Material::Particle as u8;
        let vfx = draw.is_vfx();
        debug_assert!(
            !particle || self.frame.tier == Tier::Enhanced,
            "a particle record pushed in the Baseline tier"
        );
        debug_assert!(
            !vfx || self.frame.tier == Tier::Enhanced,
            "a vfx record pushed in the Baseline tier"
        );
        self.frame.load.particles += u32::from(particle);
        if vfx {
            self.frame.ambient_motion = true;
        }
        let band = if band == Band::Layer {
            self.layer_band
        } else {
            band
        };
        self.frame.scratch.push(band, self.frame.draws.len(), &draw);
        self.frame.draws.push(draw);
        if self.frame.looking {
            self.frame.owners.push(self.owner);
        }
        self.frame.work.records += 1;
    }

    /// Records the outline `ring` in `Frame::rings` for the checks and the element records. Its
    /// records are drawn apart, with `draw`.
    pub fn ring(&mut self, ring: Ring) {
        if self.frame.rings.len() < RINGS {
            self.frame.rings.push(ring);
        }
    }

    /// Starts `sound` in this frame.
    pub fn sound(&mut self, sound: Sound) {
        self.frame.sounds.push(sound);
    }

    /// Starts each of `sounds` in this frame.
    pub fn sounds(&mut self, sounds: impl IntoIterator<Item = Sound>) {
        self.frame.sounds.extend(sounds);
    }

    /// Shakes the whole picture, but for the flat layer, by `offset` physical pixels.
    pub fn shake(&mut self, offset: Vec2) {
        self.frame.shake = offset;
    }

    /// Adds a line of the text of the resolved element at `element`, whose type is `size`
    /// points, whose baseline is at `baseline` on screen and which shows `words`. The words
    /// are kept only while the frame is `looking`.
    pub fn line(&mut self, element: u32, rect: Rect, size: f32, baseline: f32, words: &str) {
        let words = if self.frame.looking {
            let start = index(self.frame.names.len());
            self.frame.names.push_str(words);
            NameRef {
                start,
                len: index(words.len()),
            }
        } else {
            NameRef::default()
        };
        self.frame.lines.push(TextLine {
            element,
            rect,
            size,
            baseline,
            words,
        });
    }

    /// Whether the frame records how its elements look (`Frame::looking`).
    #[must_use]
    pub fn looking(&self) -> bool {
        self.frame.looking
    }

    /// Records how a flat-layer element was drawn, with the shadows of its style, while the
    /// frame is `looking`.
    pub fn look(&mut self, look: ElementLook, box_shadow: &[Shadow], drop_shadow: &[Shadow]) {
        if self.frame.looking {
            self.frame.looks.push(look, box_shadow, drop_shadow);
        }
    }

    /// Notes that the resolved element at `element` lacked atlas cells for `missing`. Past the
    /// capacity the frame was made with, which is one entry per element, a note is dropped, so
    /// that building a frame never allocates.
    pub fn undrawn(&mut self, element: u32, missing: Missing) {
        if self.frame.undrawn.len() < self.frame.undrawn.capacity() {
            self.frame.undrawn.push(Undrawn { element, missing });
        }
    }

    /// Records the layout work `Client::handle` did since the last frame: layout nodes
    /// computed and runs of text shaped.
    pub fn layout_work(&mut self, layout_nodes: u32, text_runs: u32) {
        self.frame.work.layout_nodes += layout_nodes;
        self.frame.work.text_runs += text_runs;
    }

    /// Records the objects the table placed and the script steps the show compiled since the
    /// last frame.
    pub fn placement_work(&mut self, placed: u32, steps: u32) {
        self.frame.work.placed += placed;
        self.frame.work.steps += steps;
    }

    /// Adds a clip rectangle in physical pixels and returns its entry, for `Draw::with_clip`.
    /// Returns `None` once all 15 entries are taken, and sets `Frame::clips_full`, which the
    /// checks report as a fault.
    pub fn clip(&mut self, rect: Rect) -> Option<u8> {
        if usize::from(self.clips) + 1 >= CLIPS {
            self.frame.clips_full = true;
            return None;
        }
        self.clips += 1;
        self.frame.clips[usize::from(self.clips)] = rect;
        Some(self.clips)
    }

    /// Puts the records in drawing order, sets `Frame::opaque`, `Frame::layer` and
    /// `Frame::bands`, and counts the frame's `Load` (see `order`).
    pub fn finish(self) -> &'a Frame {
        order::finish(self.frame);
        self.frame
    }
}
