//! The frame: what the client core hands the renderer and the checks for one moment in time
//! (docs/beryl-3d.md, "The frame"). It has two parts, built together: the resolved elements,
//! which the interface checks and agent tools read, and the draw list, which the renderer reads.

mod backdrop;
mod draw;
mod labels;
mod look;
mod order;
mod paint;
mod place;
pub use place::Place;
pub mod picking;
mod relations;
mod rings;
pub use labels::Labels;
pub use relations::{
    Anchor, AnchorBox, Anchored, Clearance, Decoration, Endpoint, LineEnd, NodeRef, ShapeAnchor,
    Show, ShowPlacement,
};
pub mod text;

pub use order::PaintBand;
mod undrawn;
mod work;

use std::sync::Arc;

use glam::{Mat4, Vec2};

pub use backdrop::{Backdrop, BackdropPanel};
pub use draw::{
    Draw, Material, Mesh, RADIAL, SIDE_BOTTOM, SIDE_LEFT, SIDE_RIGHT, SIDE_TOP, Sprite,
    TextureArray, box_shadow_reach, fixed, gradient_reach, rows, sprites,
};
pub use look::{EffectLook, EffectTarget, ElementLook, LookPaint, LookShadow, Looks};
pub use paint::{GRADIENTS, GradientEntry, MaskEntry, PaintTable, Paints, ScreenClip};
pub use rings::{LIFTED, Lifted, RINGS, Ring, outline_corners};
pub use undrawn::{Missing, Undrawn};
pub use work::{Areas, Load, Work};

use kit_base::atlas::Tier;
use kit_base::geometry::{Rect, Size};
use kit_base::paint::Shadow;
use kit_base::pose::Pose;

pub use kit_base::ids::ElementId;

/// The first reserved id in the range used for stack abilities, above all card and UI ids.
pub const ABILITY_IDS: u32 = 1 << 20;

/// The first element id of the arrows: an arrow's id is this plus its index among the open
/// spans.
pub const ARROW_IDS: u32 = 1 << 22;

/// What a card's element id is raised by for its face in the Revealed Cards pile: the second
/// element of a card held up to be read, beside the card itself.
pub const REVEALED_FACE: u32 = 1 << 21;

/// The first mark id. Each active mark adds its index among the open spans.
pub const MARK_IDS: u32 = 1 << 24;

/// The first tray grid id. The following range begins at `TRAY_CARD_BOX_IDS`.
pub const TRAY_GRID_IDS: u32 = 1 << 25;

/// The first id of the tray boxes that show cards, indexed by card slot.
pub const TRAY_CARD_BOX_IDS: u32 = TRAY_GRID_IDS + (1 << 16);

/// A range of `Frame::names`, so that a resolved element owns no string.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NameRef {
    pub start: u32,
    pub len: u32,
}

/// One named thing on screen, with the numbers a check or a locator needs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Resolved {
    pub id: ElementId,
    /// The index of its parent in `Frame::resolved`, or `u32::MAX` at the root. Parents come
    /// before their children.
    pub parent: u32,
    pub role: Role,
    /// Its name ("Counterspell", "Auto Pay") in `Frame::names`.
    pub name: NameRef,
    pub place: Place,
    /// The table pose before camera projection. Screen-space elements have no pose.
    pub pose: Option<Pose>,
    /// Its corners on screen, in logical points.
    pub corners: [Vec2; 4],
    /// Nearer the camera is smaller.
    pub depth: f32,
    pub opacity: f32,
    /// The rectangle it is clipped to, in logical points.
    pub clip: Option<Rect>,
    pub interactive: bool,
    pub state: StateBits,
    /// For a card, the layer of the face-texture array its face is drawn from, or `None` when
    /// it has no face of its own yet and is drawn blank. `None` for every other role.
    pub face: Option<u16>,
    /// The number represented by this element, when its owner supplies a count label.
    pub count: u16,
    /// Optional strings reported under the scene owner's annotation labels.
    pub annotations: [NameRef; labels::ANNOTATIONS],
}

/// What kind of thing an element is, as named in the tree printout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Card,
    Pile,
    Player,
    Region,
    Button,
    Text,
    Icon,
    Field,
    Toggle,
    /// A control that sets a level by a drag along it.
    Slider,
    List,
    MenuItem,
    /// An arrow or line on the table. Its `corners` are not a box but the four control points
    /// of the cubic Bezier curve it is drawn along: its start, two control points and its end.
    Arrow,
    /// An outline or symbol drawn against another node.
    Mark,
    /// A role named by the owner of the scene.
    Custom(&'static str),
}

impl Role {
    /// The word the tree printout uses.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Card => "card",
            Self::Pile => "pile",
            Self::Player => "player",
            Self::Region => "region",
            Self::Button => "button",
            Self::Text => "text",
            Self::Icon => "icon",
            Self::Field => "field",
            Self::Toggle => "toggle",
            Self::Slider => "slider",
            Self::List => "list",
            Self::MenuItem => "menuitem",
            Self::Arrow => "arrow",
            Self::Mark => "mark",
            Self::Custom(word) => word,
        }
    }

    /// Every role, in declaration order.
    pub const ALL: [Self; 14] = [
        Self::Card,
        Self::Pile,
        Self::Player,
        Self::Region,
        Self::Button,
        Self::Text,
        Self::Icon,
        Self::Field,
        Self::Toggle,
        Self::Slider,
        Self::List,
        Self::MenuItem,
        Self::Arrow,
        Self::Mark,
    ];

    /// The role whose tree word is `word`.
    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|role| role.word() == word)
    }
}

/// An element's states, one bit each.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StateBits(pub u32);

impl StateBits {
    pub const FACE_DOWN: u32 = 1 << 1;
    pub const MOVING: u32 = 1 << 2;
    pub const HOVERED: u32 = 1 << 3;
    /// Held under the pointer; the checks on cards at rest skip it.
    pub const HELD: u32 = 1 << 4;
    pub const ELIGIBLE: u32 = 1 << 5;
    pub const SELECTED: u32 = 1 << 6;
    pub const TARGET: u32 = 1 << 7;
    pub const DISABLED: u32 = 1 << 8;
    pub const FOCUSED: u32 = 1 << 9;
    /// A flat-layer element the pointer goes through (`Style::passes`), which never catches a
    /// press. The tree printout leaves it out.
    pub const PASSES: u32 = 1 << 12;
    /// The button a prompt or screen expects to be pressed, drawn amber.
    pub const PRIMARY: u32 = 1 << 13;
    /// A flat-layer element drawn on a panel of its own, a fill or a rim, which covers what the
    /// layer drew before it. The "reachable" check reads it to find a panel the pointer goes
    /// through drawn over a control, such as a scrim drawn over a prompt's buttons.
    pub const PAINTED: u32 = 1 << 14;
    /// A table object drawn over the flat layer in a box or tray.
    pub const SHOWN: u32 = 1 << 15;

    /// A card moving within its resting arrangement, such as a hand closing a gap.
    pub const LAYOUT_MOTION: u32 = 1 << 16;

    /// A hand card rising out of the fan, held up to read, or easing back into the fan.
    pub const RAISED: u32 = 1 << 17;

    /// Whether every bit of `bits` is set.
    #[must_use]
    pub fn has(self, bits: u32) -> bool {
        self.0 & bits == bits
    }
}

/// One of the sounds of the list both clients play from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundId(pub u16);

/// A sound that begins in a frame. `theirs` says it is the opponent's, which the web interface
/// plays 4 dB quieter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sound {
    pub id: SoundId,
    pub theirs: bool,
}

/// The most sounds a frame holds without allocating.
const SOUNDS: usize = 16;

/// When the client next needs a frame, as of the last call to `Client::frame`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wake {
    /// Something is animating.
    Now,
    /// At this time, in nanoseconds since the client started.
    At(u64),
    /// Not until an input arrives.
    Never,
}

impl Wake {
    /// The earlier of this wake and one at `at`, if there is one.
    #[must_use]
    pub fn or_at(self, at: Option<u64>) -> Self {
        match (self, at) {
            (Self::At(wake), Some(at)) => Self::At(wake.min(at)),
            (Self::Never, Some(at)) => Self::At(at),
            (wake, _) => wake,
        }
    }
}

/// The renderer's record bands, including the passes for depth, shadows and blending.
/// `PaintBand` orders scene nodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Band {
    /// Cards and the table, nearest first, with a depth buffer.
    Opaque,
    /// Contact shadows of cards above the table.
    ContactShadows,
    /// The soft edges of cards, and the marks on cards lying on the table.
    TableMarks,
    /// Marks on cards above the table: in flight, in a hand, on the stack, held up to be read.
    RaisedMarks,
    /// Decorations in front of raised objects and their marks, below objects in flight.
    TableOverlays,
    /// Glows and streaks.
    Glows,
    /// Particles, which only the Enhanced tier draws: blended, tested against the depth
    /// buffer like the other bands of the table, farthest first.
    Particles,
    /// Objects in motion above the resting table, unaffected by its depth buffer.
    MovingOpaque,
    /// Translucent moving cards and the decorations of cards in motion.
    Moving,
    /// The flat layer, in the order of its tree.
    Layer,
    /// Raised hand cards and cards shown inside flat-layer elements, with their marks.
    LayerCards,
    /// Flat-layer elements that must cover those cards, such as a tooltip.
    LayerCover,
    /// Objects held by the pointer above the scene.
    Dragged,
}

/// How many entries `Frame::clips` has; entry 0 means no clip.
pub const CLIPS: usize = 16;

/// The most objects the engine's largest board holds, and so the most cards a frame draws.
pub const OBJECTS: usize = 1024;
/// The most records one card writes: the card, its soft edge, its contact shadow and a mark, and
/// its badges (a plaque and its digits, the manual table's lock and a named counter with a
/// six-letter name) or, in a hand, its cost (`table::badges`).
pub const RECORDS_PER_OBJECT: usize = 18;
/// The most records the flat layer writes: the glyphs and panels of a prompt, a card browser and
/// the text around the table.
pub const LAYER_RECORDS: usize = 4096;
/// The most abilities on the stack a frame draws as cards.
pub const ABILITIES: usize = 64;
/// The most particle records a frame holds (`Band::Particles`), which only the Enhanced tier
/// writes.
pub const PARTICLES: usize = 2000;
/// The most records all the cards of a frame write for the lines of text on their faces (a
/// note or a chosen value, and a copy's label), beyond `RECORDS_PER_OBJECT`: few permanents
/// have one, and a line is too long to fit in every card's share.
pub const TEXT_RECORDS: usize = 2048;
/// The most records a frame holds, so that ordering the largest board allocates nothing.
pub const DRAWS: usize =
    (OBJECTS + ABILITIES) * RECORDS_PER_OBJECT + LAYER_RECORDS + PARTICLES + TEXT_RECORDS;
/// How many bands there are (`Band`).
pub const BANDS: usize = Band::Dragged as usize + 1;
/// The most elements of the flat layer a frame holds.
pub const LAYER_ELEMENTS: usize = 3072;
/// The most resolved elements a frame holds: one for each card and each ability on the stack,
/// one for the cost of each card in a hand, room for their marks, and the flat layer's.
pub const ELEMENTS: usize = 2 * OBJECTS + ABILITIES + RINGS + LAYER_ELEMENTS;
/// The most bytes of names a frame holds: 32 bytes for each element, which a card's name
/// rarely exceeds.
pub const NAME_BYTES: usize = ELEMENTS * 32;

/// A scene with its own camera and logical window, drawn within a physical pixel rectangle.
#[derive(Clone, Debug, PartialEq)]
pub struct Inset {
    pub frame: Frame,
    pub size: Size,
    pub scale: f32,
    pub viewport: Rect,
}

/// What the client core hands the renderer and the checks for one moment.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// The labels its owner uses for states and reported counts.
    pub labels: &'static Labels,
    /// Capacity set at startup, cleared each frame.
    pub resolved: Vec<Resolved>,
    /// Screen element indexes with a Show relation, in registration order, and their targets.
    shows: Vec<(u32, ElementId)>,
    /// The anchored placements written in this frame.
    pub anchors: Vec<Anchored>,
    /// The boxes of layout nodes that paint nothing, such as an empty row's container.
    anchor_nodes: Vec<(ElementId, Rect)>,
    drop_regions: Vec<picking::DropRegion>,
    /// Every resolved element's name, end to end.
    pub names: String,
    /// In drawing order.
    pub draws: Vec<Draw>,
    /// `draws[..opaque]` are the opaque records.
    pub opaque: u32,
    /// `draws[layer..]` are the flat layer's records, drawn with no depth test and not shaken.
    pub layer: u32,
    /// The index in `draws` where each band starts, by `Band` value, and the end of `draws`
    /// as the last entry, so that `draws[bands[b]..bands[b + 1]]` are band `b`'s records.
    pub bands: [u32; BANDS + 1],
    /// The visual tier the frame is drawn in (docs/beryl-3d.md, "Choosing a tier"). The
    /// renderer chooses its passes from this alone.
    pub tier: Tier,
    /// The camera's view and projection: from the table's space to the renderer's clip space,
    /// with depth from 0 at the near plane to 1 at the far one. It serves every record,
    /// because a flat-layer record is placed on a plane just in front of the camera.
    pub view_projection: Mat4,
    /// A separate scene composited into this frame, if its viewport is visible.
    pub inset: Option<Box<Inset>>,
    /// A card's printed width and height in millimetres, the size of the card mesh. The
    /// renderer builds its card mesh at this size, and `finish` bounds card records by it.
    pub card: Vec2,
    /// Clip rectangles in physical pixels; entry 0 is not used.
    pub clips: [Rect; CLIPS],
    /// Whether a clip was asked for after all 15 entries were taken. The records that wanted it
    /// are drawn unclipped, and the checks report it as a fault.
    pub clips_full: bool,
    /// The stops of the frame's gradients and the masks of its clips (`paint`).
    pub paints: Paints,
    /// The blur and color filters drawn over what comes before `Backdrop::at`, if any.
    pub backdrop: Option<Backdrop>,
    /// How far the whole picture is shaken, in physical pixels.
    pub shake: Vec2,
    /// The sounds that begin in this frame.
    pub sounds: Vec<Sound>,
    /// Each laid-out line of text, for the "text fits" check.
    pub lines: Vec<TextLine>,
    /// Whether the frame records `looks` and each line's words (`TextLine::words`), for the
    /// tools that compare Beryl 3D with the web interface. Building a frame keeps it as it is.
    pub looking: bool,
    /// How each flat-layer element was drawn, while `looking`.
    pub looks: Looks,
    /// While `looking`, the index in `resolved` of the element each record of `draws` belongs
    /// to, in drawing order, so that a tool can draw one element alone (`keep_only`). A record
    /// belongs to the element added last before it was pushed, or to none (`u32::MAX`), as the
    /// table's cloth does.
    pub owners: Vec<u32>,
    /// Each element whose text or icon the frame could not draw in full because no atlas cell
    /// holds a glyph or the icon at its own size, for the "drawn" check. A host delivers what
    /// the client asks for soon after it is asked, so this is empty once delivery has caught up.
    pub undrawn: Vec<Undrawn>,
    /// The hand cards the pointer has up out of their place in the fan (rising, raised or
    /// falling), each with the box it has resting in the fan. The pointer's hit test and the
    /// "reachable" check put these boxes in place of the drawn ones, so that a pointer over the
    /// part of the fan a raised card covers still finds the card that rests there.
    pub lifted: Vec<Lifted>,
    /// The scene relations for outlines, symbols and lines, in element registration order.
    pub decorations: Vec<Decoration>,
    /// Each mark's outline, with its drawn geometry and styling.
    pub rings: Vec<Ring>,
    /// The handle-time placements and the resting boxes they must avoid.
    pub clearances: Arc<[Clearance]>,
    pub load: Load,
    pub work: Work,
    /// How many of `TEXT_RECORDS` the frame's cards have not yet written.
    text_room: usize,
    /// The sort keys and reordered records of `FrameBuilder::finish`, kept so that ordering
    /// allocates nothing.
    scratch: order::Scratch,
}

/// One laid-out line of an element's text.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextLine {
    /// The index in `Frame::resolved` of the element whose text it is.
    pub element: u32,
    /// The line's box on screen, in logical points: as wide as its glyphs and as tall as the
    /// line.
    pub rect: Rect,
    /// The type size of its text, in logical points: the text's own size, which a text that
    /// shrinks to fit has shrunk.
    pub size: f32,
    /// The y of its baseline on screen, in logical points.
    pub baseline: f32,
    /// Its words in `Frame::names`, while `Frame::looking`, and empty otherwise.
    pub words: NameRef,
}

impl Frame {
    /// An empty frame with room for `elements` resolved elements, `draws` records and
    /// `name_bytes` bytes of names, so that building a frame within those sizes allocates
    /// nothing.
    #[must_use]
    pub fn with_capacity(elements: usize, draws: usize, name_bytes: usize) -> Self {
        Self {
            labels: &Labels::DEFAULT,
            resolved: Vec::with_capacity(elements),
            shows: Vec::with_capacity(elements),
            anchors: Vec::with_capacity(elements),
            anchor_nodes: Vec::with_capacity(elements.max(32)),
            drop_regions: Vec::with_capacity(8),
            names: String::with_capacity(name_bytes),
            draws: Vec::with_capacity(draws),
            opaque: 0,
            layer: 0,
            bands: [0; BANDS + 1],
            tier: Tier::Baseline,
            view_projection: Mat4::IDENTITY,
            inset: None,
            card: Vec2::ONE,
            clips: [Rect::default(); CLIPS],
            clips_full: false,
            paints: Paints::default(),
            backdrop: None,
            shake: Vec2::ZERO,
            sounds: Vec::with_capacity(SOUNDS),
            lines: Vec::with_capacity(elements),
            looking: false,
            looks: Looks::default(),
            owners: Vec::new(),
            undrawn: Vec::with_capacity(elements),
            lifted: Vec::with_capacity(LIFTED),
            decorations: Vec::with_capacity(elements),
            rings: Vec::with_capacity(RINGS),
            clearances: Arc::default(),
            load: Load::default(),
            work: Work::default(),
            text_room: TEXT_RECORDS,
            scratch: order::Scratch::with_capacity(draws, elements),
        }
    }

    /// The name of a resolved element.
    #[must_use]
    pub fn name(&self, name: NameRef) -> &str {
        let start = name.start as usize;
        &self.names[start..start + name.len as usize]
    }

    /// How many named face-up cards this frame draws blank, with no face layer of their own: a
    /// host that prepares faces leaves none once a match's faces have been uploaded.
    #[must_use]
    pub fn faceless_cards(&self) -> usize {
        (self.resolved.iter())
            .filter(|element| {
                element.role == Role::Card
                    && element.face.is_none()
                    && element.name.len > 0
                    && element.state.0 & StateBits::FACE_DOWN == 0
            })
            .count()
    }
}

/// What `Client::frame` writes a frame into. Making one clears the frame, keeping its capacity.
/// Records may be pushed in any order: `finish` puts them in drawing order.
pub struct FrameBuilder<'a> {
    frame: &'a mut Frame,
    clips: u8,
    /// The element added last, which the records pushed after it belong to (`Frame::owners`).
    owner: u32,
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
        }
    }

    /// The visual tier the frame is drawn in, which decides the records only one tier writes:
    /// contact shadows in Baseline, particles in Enhanced.
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
        element.name = NameRef {
            start: index(self.frame.names.len()),
            len: index(name.len()),
        };
        self.frame.names.push_str(name);
        self.frame.resolved.push(element);
        self.owner = at;
        at
    }

    /// Records that only the modal at `root` and its descendants accept input.
    /// Resolved screen elements are written in tree order, after the table.
    ///
    /// # Panics
    ///
    /// If `root` is not an index of a resolved element.
    pub fn restrict_input(&mut self, root: u32) {
        let (before, modal) = self.frame.resolved.split_at_mut(root as usize);
        let after = modal[1..]
            .iter()
            .position(|node| node.parent < root || node.parent == u32::MAX)
            .map_or(modal.len(), |position| position + 1);
        for node in before.iter_mut().chain(&mut modal[after..]) {
            node.interactive = false;
        }
    }

    /// Ends the last element's draw records before emitting light or effects with no node.
    pub fn clear_owner(&mut self) {
        self.owner = u32::MAX;
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

    /// Adds a record to `band`. Within the flat layer's band, records are drawn in the order
    /// they are pushed, which is the order of its tree.
    pub fn draw(&mut self, band: Band, draw: Draw) {
        let particle = draw.material == Material::Particle as u8;
        debug_assert!(
            !particle || self.frame.tier == Tier::Enhanced,
            "a particle record pushed in the Baseline tier"
        );
        self.frame.load.particles += u32::from(particle);
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

/// A length as a `u32` index. A frame never holds four billion of anything.
fn index(length: usize) -> u32 {
    u32::try_from(length).expect("a frame's arrays fit u32 indexes")
}
