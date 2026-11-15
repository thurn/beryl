//! The frame: what the client core hands the renderer and the checks for one moment in time
//! (docs/beryl-3d.md, "The frame"). It has two parts, built together: the resolved elements,
//! which the interface checks and agent tools read, and the draw list, which the renderer reads.

mod draw;

use beryl_session::protocol::{RowName, Side, ZoneName};
use glam::{Mat4, Vec2};

pub use draw::{Draw, Material, Mesh, Sprite, TextureArray};

use crate::input::Rect;

/// A card's element id is its object's slot in the view. A flat-layer element's is given when
/// it first enters the tree, and kept while it stays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ElementId(pub u32);

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
            Self::List => "list",
            Self::MenuItem => "menuitem",
            Self::Arrow => "arrow",
        }
    }

    /// Every role, in declaration order.
    pub const ALL: [Self; 13] = [
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

/// Where an element is: in the flat layer, or for a card its zone, whose side of the table it
/// is on, and its battlefield row where it has one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Layer,
    Card {
        zone: ZoneName,
        side: Side,
        row: Option<RowName>,
    },
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

    /// Whether every bit of `bits` is set.
    #[must_use]
    pub fn has(self, bits: u16) -> bool {
        self.0 & bits == bits
    }
}

/// A sound that begins in a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundId(pub u16);

/// Numbers that predict the GPU's load, added up while the frame is built.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Load {
    pub draw_calls: u32,
    pub instances: u32,
    pub upload_bytes: u32,
    /// The screen area of the opaque records, in screens.
    pub opaque_area: f32,
    /// The screen area of the blended records, in screens.
    pub blended_area: f32,
}

/// The work the client core did, counted so that the gates can hold it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Work {
    /// Elements visited.
    pub elements: u32,
    /// Animation segments evaluated.
    pub segments: u32,
    /// Layout nodes computed, in `handle`.
    pub layout_nodes: u32,
    /// Runs of text shaped, in `handle`.
    pub text_runs: u32,
    /// Draw records written.
    pub records: u32,
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

/// A group of records drawn together. The opaque pass comes first, then the seven blended
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
    /// The flat layer, in the order of its tree.
    Layer,
    /// Cards shown inside flat-layer elements, such as a card browser, with their marks.
    LayerCards,
    /// Flat-layer elements that must cover those cards, such as a tooltip.
    LayerCover,
}

/// How many entries `Frame::clips` has; entry 0 means no clip.
pub const CLIPS: usize = 16;

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
    /// The camera's view and projection: from the table's space to the renderer's clip space,
    /// with depth from 0 at the near plane to 1 at the far one. It serves every record,
    /// because a flat-layer record is placed on a plane just in front of the camera.
    pub view_projection: Mat4,
    /// Clip rectangles in physical pixels; entry 0 is not used.
    pub clips: [Rect; CLIPS],
    /// How far the whole picture is shaken, in physical pixels.
    pub shake: Vec2,
    /// The sounds that begin in this frame.
    pub sounds: Vec<SoundId>,
    pub load: Load,
    pub work: Work,
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
            view_projection: Mat4::IDENTITY,
            clips: [Rect::default(); CLIPS],
            shake: Vec2::ZERO,
            sounds: Vec::new(),
            load: Load::default(),
            work: Work::default(),
        }
    }

    /// The name of a resolved element.
    #[must_use]
    pub fn name(&self, name: NameRef) -> &str {
        let start = name.start as usize;
        &self.names[start..start + name.len as usize]
    }
}

/// What `Client::frame` writes a frame into. Making one clears the frame, keeping its capacity.
pub struct FrameBuilder<'a> {
    frame: &'a mut Frame,
    clips: u8,
    /// Whether every record pushed so far is opaque.
    opaque_run: bool,
    /// Whether a record of the flat layer has been pushed.
    in_layer: bool,
}

impl<'a> FrameBuilder<'a> {
    pub fn new(frame: &'a mut Frame) -> Self {
        frame.resolved.clear();
        frame.names.clear();
        frame.draws.clear();
        frame.opaque = 0;
        frame.layer = 0;
        frame.view_projection = Mat4::IDENTITY;
        frame.clips = [Rect::default(); CLIPS];
        frame.shake = Vec2::ZERO;
        frame.sounds.clear();
        frame.load = Load::default();
        frame.work = Work::default();
        Self {
            frame,
            clips: 0,
            opaque_run: true,
            in_layer: false,
        }
    }

    /// Adds `element` named `name`, and returns its index. The builder sets `element.name`.
    pub fn resolved(&mut self, name: &str, mut element: Resolved) -> u32 {
        element.name = NameRef {
            start: index(self.frame.names.len()),
            len: index(name.len()),
        };
        self.frame.names.push_str(name);
        self.frame.resolved.push(element);
        self.frame.work.elements += 1;
        index(self.frame.resolved.len() - 1)
    }

    /// Sets the camera's view and projection, `Frame::view_projection`.
    pub fn camera(&mut self, view_projection: Mat4) {
        self.frame.view_projection = view_projection;
    }

    /// Adds a record to `band`. The records stay in the order they were pushed, so `opaque`
    /// counts only the opaque records pushed before any blended one, and `layer` is where the
    /// first record of the flat layer's bands was pushed.
    pub fn draw(&mut self, band: Band, draw: Draw) {
        self.opaque_run &= band == Band::Opaque;
        if self.opaque_run {
            self.frame.opaque += 1;
        }
        if !self.in_layer && band >= Band::Layer {
            self.in_layer = true;
            self.frame.layer = index(self.frame.draws.len());
        }
        self.frame.draws.push(draw);
        self.frame.work.records += 1;
        self.frame.load.instances += 1;
    }

    /// Adds a clip rectangle in physical pixels and returns its entry, for `Draw::with_clip`.
    /// Returns `None` once all 15 entries are taken, which is a fault the checks report.
    pub fn clip(&mut self, rect: Rect) -> Option<u8> {
        if usize::from(self.clips) + 1 >= CLIPS {
            return None;
        }
        self.clips += 1;
        self.frame.clips[usize::from(self.clips)] = rect;
        Some(self.clips)
    }

    pub fn finish(self) -> &'a Frame {
        if !self.in_layer {
            self.frame.layer = index(self.frame.draws.len());
        }
        self.frame
    }
}

/// A length as a `u32` index. A frame never holds four billion of anything.
fn index(length: usize) -> u32 {
    u32::try_from(length).expect("a frame's arrays fit u32 indexes")
}
