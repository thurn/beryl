//! What the frame records for the checks and the pointer beyond its records: the outlines of
//! marks with the things they ring, and the hand cards up out of their place in the fan.

use glam::Vec2;

use super::{ABILITIES, ElementId, Frame, OBJECTS};
use beryl_protocol::protocol::Side;
use beryl_protocol::script::Mark;

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

/// A mark's outline as the frame draws it: the thing it rings, which mark it is, and the
/// transform of its record (`Draw::model`), which places the `sprites::OUTLINE` quad in the
/// table's space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ring {
    pub on: Ringed,
    /// Which mark the outline draws, which `just b3d match` names its record by.
    pub mark: Mark,
    pub model: [f32; 12],
    /// The outline's color and opacity (`Draw::tint`), the opacity falling as the mark comes
    /// and goes.
    pub tint: [u8; 4],
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
