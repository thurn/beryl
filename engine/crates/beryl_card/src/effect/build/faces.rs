//! Builders for effects that turn or reveal cards.

use crate::effect::{Condition, Effect, Objects};
use crate::filter::Filter;

/// [`Effect::Transform`]: turn a transforming permanent to its other face.
pub const fn transform(objects: Objects) -> Effect {
    Effect::Transform(objects)
}

/// [`Effect::Flip`]: change a permanent's status to flipped.
pub const fn flip(objects: Objects) -> Effect {
    Effect::Flip(objects)
}

/// [`Effect::Reveal`]: show cards to every player.
pub const fn reveal(objects: Objects) -> Effect {
    Effect::Reveal(objects)
}

/// [`Effect::ReturnTransformed`]: "return [cards] to the battlefield transformed under their
/// owners' control".
pub const fn return_transformed(cards: Objects) -> Effect {
    Effect::ReturnTransformed(cards)
}

/// [`Effect::Meld`]: "exile them, then meld them into [result]", where the partner is a
/// permanent `partner` accepts.
pub const fn meld(partner: Filter) -> Effect {
    Effect::Meld(partner)
}

/// [`Condition::MeldPair`]: "if you both own and control this and a [filter] named [its
/// partner]".
pub const fn own_and_control_meld_pair(partner: Filter) -> Condition {
    Condition::MeldPair(partner)
}
