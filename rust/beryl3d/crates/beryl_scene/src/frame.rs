//! The frame: what the client core hands the renderer and the checks for one moment in time
//! (docs/beryl-3d.md, "The frame"). It has two parts, built together: the resolved elements,
//! which the interface checks and agent tools read, and the draw list, which the renderer reads.

mod backdrop;
mod draw;
mod look;
mod order;
mod paint;
mod rings;
mod undrawn;

use beryl_protocol::protocol::{RowName, Side, ZoneName};
use glam::{Mat4, Vec2};

pub use backdrop::Backdrop;
pub use draw::{
    Draw, Material, Mesh, RADIAL, SIDE_BOTTOM, SIDE_LEFT, SIDE_RIGHT, SIDE_TOP, Sprite,
    TextureArray, box_shadow_reach, fixed, gradient_reach, sprites,
};
pub use look::{EffectLook, ElementLook, LookPaint, LookShadow, Looks};
pub use paint::{GRADIENTS, GradientEntry, MaskEntry, PaintTable, Paints};
pub use rings::{LIFTED, Lifted, RINGS, Ring, Ringed};
pub use undrawn::{Missing, Undrawn};

use crate::element::Shadow;
use crate::input::{Rect, Tier};

/// A card's element id is its object's slot in the view, and the face of that card in the
/// Revealed Cards pile's is `REVEALED_FACE` more. A flat-layer element's is given when it first
/// enters the tree, and kept while it stays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ElementId(pub u32);

/// What a card's element id is raised by for its face in the Revealed Cards pile: the second
/// element of a card held up to be read, beside the card itself.
pub const REVEALED_FACE: u32 = 1 << 21;

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
    /// The size of a battlefield pile on its top card, zero on buried copies, and one elsewhere.
    pub copies: u16,
    /// For a card that is a copy of another card, the name of the card it really is in
    /// `Frame::names` ([CR 707.2]), while `name` is the copied card's. Empty for every other
    /// element.
    ///
    /// [CR 707.2]: https://yawgatog.com/resources/magic-rules/#R7072
    pub really: NameRef,
    /// For a permanent, the line of text drawn on its face: its manual table note, or the
    /// value it chose as it entered, in `Frame::names`. Empty when its face shows none.
    pub note: NameRef,
}

/// What kind of thing an element is. Closed: a new role is added here with the word the tree
/// printout uses for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Card,
    Ability,
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
}

impl Role {
    /// The word the tree printout uses.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Card => "card",
            Self::Ability => "ability",
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
        }
    }

    /// Every role, in declaration order.
    pub const ALL: [Self; 14] = [
        Self::Card,
        Self::Ability,
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
    ];

    /// The role whose tree word is `word`.
    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|role| role.word() == word)
    }
}

/// Where an element is: in the flat layer, for a card its zone, whose side of the table it is
/// on and its battlefield row where it has one, or the Revealed Cards pile for the second card
/// element that shows a revealed card's face there (`table::revealed`). The face is no game
/// object of its own, so it names no zone: the card itself lies where the table has it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Layer,
    Card {
        zone: ZoneName,
        side: Side,
        row: Option<RowName>,
    },
    Revealed,
}

/// An element's states, one bit each.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StateBits(pub u16);

impl StateBits {
    pub const TAPPED: u16 = 1 << 0;
    pub const FACE_DOWN: u16 = 1 << 1;
    pub const MOVING: u16 = 1 << 2;
    pub const HOVERED: u16 = 1 << 3;
    /// Held under the pointer; the checks on cards at rest skip it.
    pub const HELD: u16 = 1 << 4;
    pub const ELIGIBLE: u16 = 1 << 5;
    pub const SELECTED: u16 = 1 << 6;
    pub const TARGET: u16 = 1 << 7;
    pub const DISABLED: u16 = 1 << 8;
    pub const FOCUSED: u16 = 1 << 9;
    /// A phased-out permanent, which stays where it lay, faded.
    pub const PHASED_OUT: u16 = 1 << 10;
    /// A card its player picked in a hidden pick, which lies apart from their hand.
    pub const PICKED: u16 = 1 << 11;
    /// A flat-layer element the pointer goes through (`Style::passes`), which never catches a
    /// press. The tree printout leaves it out.
    pub const PASSES: u16 = 1 << 12;
    /// The button a prompt or screen expects to be pressed, drawn amber.
    pub const PRIMARY: u16 = 1 << 13;
    /// A flat-layer element drawn on a panel of its own, a fill or a rim, which covers what the
    /// layer drew before it. The "reachable" check reads it to find a panel the pointer goes
    /// through drawn over a control, such as a scrim drawn over a prompt's buttons.
    pub const PAINTED: u16 = 1 << 14;

    /// Whether every bit of `bits` is set.
    #[must_use]
    pub fn has(self, bits: u16) -> bool {
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

/// Numbers that predict the GPU's load, added up while the frame is built. The screen areas,
/// which take longer to measure, are `Frame::areas`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Load {
    pub draw_calls: u32,
    pub instances: u32,
    pub upload_bytes: u32,
    /// The particle records of the frame, which only the Enhanced tier writes.
    pub particles: u32,
}

/// The screen area of a frame's records, in screens (`Frame::areas`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Areas {
    /// The screen area of the opaque records.
    pub opaque: f32,
    /// The screen area of the blended records.
    pub blended: f32,
}

/// The work the client core did, counted so that the gates can hold it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Work {
    /// Elements visited: each object slot of the table and each node of the flat layer that
    /// the frame looks at, whether or not it draws it.
    pub elements: u32,
    /// Animation segments evaluated: one for each object whose pose comes from a track.
    pub segments: u32,
    /// Layout nodes computed, in `handle`.
    pub layout_nodes: u32,
    /// Runs of text shaped, in `handle`.
    pub text_runs: u32,
    /// Draw records written.
    pub records: u32,
    /// Objects placed: each object of `Placement::rests` whose resting place the table worked
    /// out, whether in `handle`, compiling a script step by step, or in the frame. A group whose
    /// resting places are copied from a placement already made counts nothing, and neither do
    /// the abilities on the stack, which are few.
    pub placed: u32,
    /// Script steps compiled, in `handle` or in the frame: each step of each script the show
    /// was given or compiled again. A step costs work even where it places nothing and writes
    /// no segment, such as an arrow that opens.
    pub steps: u32,
}

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

/// A group of records drawn together. The opaque pass comes first, then the eight blended
/// bands in this order.
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
    /// Glows and streaks.
    Glows,
    /// Particles, which only the Enhanced tier draws: blended, tested against the depth
    /// buffer like the other bands of the table, farthest first.
    Particles,
    /// The flat layer, in the order of its tree.
    Layer,
    /// Cards shown inside flat-layer elements, such as a card browser, with their marks.
    LayerCards,
    /// Flat-layer elements that must cover those cards, such as a tooltip.
    LayerCover,
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
pub const BANDS: usize = 9;
/// The most elements of the flat layer a frame holds.
pub const LAYER_ELEMENTS: usize = 3072;
/// The most resolved elements a frame holds: one for each card and each ability on the stack,
/// one for the cost of each card in a hand, and the flat layer's.
pub const ELEMENTS: usize = 2 * OBJECTS + ABILITIES + LAYER_ELEMENTS;
/// The most bytes of names a frame holds: 32 bytes for each element, which a card's name
/// rarely exceeds.
pub const NAME_BYTES: usize = ELEMENTS * 32;

/// What the client core hands the renderer and the checks for one moment.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// Capacity set at startup, cleared each frame.
    pub resolved: Vec<Resolved>,
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
    /// Each mark's outline the frame draws, with the thing it rings, for the "marks on their
    /// things" check.
    pub rings: Vec<Ring>,
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
            resolved: Vec::with_capacity(elements),
            names: String::with_capacity(name_bytes),
            draws: Vec::with_capacity(draws),
            opaque: 0,
            layer: 0,
            bands: [0; BANDS + 1],
            tier: Tier::Baseline,
            view_projection: Mat4::IDENTITY,
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
            rings: Vec::with_capacity(RINGS),
            load: Load::default(),
            work: Work::default(),
            text_room: TEXT_RECORDS,
            scratch: order::Scratch::with_capacity(draws),
        }
    }

    /// The centre of the box of the table's element `id` (a card or an ability, not an element
    /// of the flat layer), on the window in logical points, if the frame holds it.
    #[must_use]
    pub fn centre(&self, id: ElementId) -> Option<Vec2> {
        let corners = self.corners(id)?;
        Some(corners.iter().copied().sum::<Vec2>() / 4.0)
    }

    /// The corners of the box of the table's element `id` (a card or an ability, not an
    /// element of the flat layer), on the window in logical points, if the frame holds it.
    #[must_use]
    pub fn corners(&self, id: ElementId) -> Option<[Vec2; 4]> {
        let element = (self.resolved.iter())
            .find(|element| element.id == id && element.place != Place::Layer)?;
        Some(element.corners)
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
    /// Starts a frame drawn in `tier`.
    pub fn new(frame: &'a mut Frame, tier: Tier) -> Self {
        frame.resolved.clear();
        frame.names.clear();
        frame.draws.clear();
        frame.opaque = 0;
        frame.layer = 0;
        frame.bands = [0; BANDS + 1];
        frame.tier = tier;
        frame.view_projection = Mat4::IDENTITY;
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
    pub fn resolved(&mut self, name: &str, mut element: Resolved) -> u32 {
        element.name = NameRef {
            start: index(self.frame.names.len()),
            len: index(name.len()),
        };
        self.frame.names.push_str(name);
        self.frame.resolved.push(element);
        self.owner = index(self.frame.resolved.len() - 1);
        self.owner
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

    /// Names the card the element at `at` in `Frame::resolved` really is, for a copy
    /// (`Resolved::really`).
    pub fn really(&mut self, at: u32, name: &str) {
        let really = NameRef {
            start: index(self.frame.names.len()),
            len: index(name.len()),
        };
        self.frame.names.push_str(name);
        self.frame.resolved[at as usize].really = really;
    }

    /// Names the line of text drawn on the face of the element at `at` in `Frame::resolved`
    /// (`Resolved::note`).
    pub fn note(&mut self, at: u32, text: &str) {
        let note = NameRef {
            start: index(self.frame.names.len()),
            len: index(text.len()),
        };
        self.frame.names.push_str(text);
        self.frame.resolved[at as usize].note = note;
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

    /// Adds the record of the outline of `mark` that rings `on` to `band`, as `draw` does, and
    /// records it in `Frame::rings` for the checks and the element records.
    pub fn ring(
        &mut self,
        band: Band,
        on: Ringed,
        mark: beryl_protocol::script::Mark,
        outline: Draw,
    ) {
        if self.frame.rings.len() < RINGS {
            self.frame.rings.push(Ring {
                on,
                mark,
                model: outline.model,
                tint: outline.tint,
            });
        }
        self.draw(band, outline);
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
