//! What the frame records for the checks and the pointer beyond its records: the outlines of
//! marks with the things they ring, and the hand cards up out of their place in the fan.

use glam::Vec2;

use super::{ABILITIES, ElementId, Frame, OBJECTS};
use beryl_protocol::protocol::Side;
use beryl_protocol::script::Mark;
use kit_base::paint::Shadow;

/// The most hand cards the pointer can have up out of the fan at once. One is raised at a time,
/// and a pointer swept across a hand leaves each card it crossed falling for a moment; past this
/// many, the card that started falling first lands at once.
pub const LIFTED: usize = 32;

/// The most mark outlines a frame records in `Frame::rings`: two on each card, ability and
/// player. An outline past that many is drawn but not checked.
pub const RINGS: usize = 2 * (OBJECTS + ABILITIES + 2);

/// What a mark's outline rings: a card's or an ability's element of the table, or a player's
/// life total.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ringed {
    Element(ElementId),
    Player(Side),
}

/// A mark's outline as the frame draws it: the thing it rings, which mark it is, and where it
/// lies. `model` places a unit quad (-0.5 to 0.5 across) in the table's space, as a record's
/// `Draw::model` does: the box the outline is drawn round, the card's own for a card, as the web
/// interface's `.stage-mark` element is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ring {
    pub on: Ringed,
    /// Which mark the outline draws, which `just b3d match` names its record by.
    pub mark: Mark,
    pub model: [f32; 12],
    /// Half the width and height, in the units of `model`'s quad, of the rectangle the
    /// outline's line runs along.
    pub line: [f32; 2],
    /// Half the width and height, in the same units, of the rectangle outside which the
    /// outline and its glow draw nothing.
    pub outer: [f32; 2],
    /// The outline's color and opacity, the opacity falling as the mark comes and goes.
    pub tint: [u8; 4],
    /// The CSS box shadows the outline is drawn with, their lengths in frames (the web
    /// interface's `--frame`, a card's width), their colors white at each shadow's own opacity,
    /// which `tint` colors. Empty for an outline drawn as a sprite (a player's).
    pub shadows: &'static [Shadow],
    /// How many frames wide the box `model` places is.
    pub frames: f32,
    /// The box's corner radius, in frames.
    pub radius: f32,
}

/// A hand card up out of its place in the fan, and the box it has resting there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lifted {
    /// The index of the card's element in `Frame::resolved`.
    pub element: u32,
    /// Its corners on screen resting in the fan, in logical points.
    pub corners: [Vec2; 4],
    /// Its depth resting in the fan.
    pub depth: f32,
}

impl Frame {
    /// Exchanges each lifted card's drawn box in `resolved` with its resting box in `lifted`.
    /// Called twice, it leaves the frame as it was.
    pub fn swap_lifted(&mut self) {
        for lifted in &mut self.lifted {
            let element = &mut self.resolved[lifted.element as usize];
            std::mem::swap(&mut element.corners, &mut lifted.corners);
            std::mem::swap(&mut element.depth, &mut lifted.depth);
        }
    }
}
