//! Builders for effects that turn or reveal cards.

use crate::effect::{Effect, Objects};

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
