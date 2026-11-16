//! How one card is drawn in a frame (`Look`), and what changes it beside its pose (`Dress`).

use super::badges::Badges;
use crate::effects::EffectRun;
use crate::light::Lit;
use crate::pointer::Pointer;
use crate::pose::Pose;

/// How one card is drawn in a frame: made from its pose, face and state, and then changed in
/// turn by the effects on it, the marks on it and the pointer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub pose: Pose,
    /// Multiplies the face's color.
    pub tint: [u8; 3],
    /// How far the card has broken up, from 0 to 1.
    pub dissolve: f32,
    /// How far its face is replaced by white light, from 0 to 1.
    pub whiten: f32,
    /// The board-size layer of its face, if it has one.
    pub face: Option<u16>,
    /// `StateBits`.
    pub state: u16,
    pub interactive: bool,
}

/// What changes how the cards are drawn in a frame, beside their poses: the effects playing,
/// the spans shown, the pointer, and the time.
#[derive(Clone, Copy, Debug)]
pub struct Dress<'a> {
    pub effects: &'a [EffectRun],
    /// The numbers the badges on permanents are made of.
    pub badges: &'a Badges,
    pub lit: &'a Lit,
    pub pointer: &'a Pointer,
    pub now: u64,
}
