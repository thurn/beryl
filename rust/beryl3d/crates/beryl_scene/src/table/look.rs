//! How one card is drawn in a frame (`Look`), and what changes it beside its pose (`Dress`).

use beryl_protocol::script::Subject;

use super::{Camera, badges::Badges};
use crate::effects::EffectRun;
pub use crate::effects::card::Look;

/// What changes how the cards are drawn in a frame, beside their poses: the effects playing,
/// the marks shown, the pointer, and the time.
#[derive(Clone, Copy)]
pub struct Dress<'a> {
    pub effects: &'a [EffectRun],
    /// The numbers the badges on permanents are made of.
    pub badges: &'a Badges,
    /// Applies the marks on the card or ability at the frame's time.
    pub marks: &'a dyn Fn(Subject, &mut Look),
    /// Applies the pointer's changes to the card at the frame's time.
    pub pointer: &'a dyn Fn(usize, &Camera, &mut Look),
    pub now: u64,
}
