// Generated from the card store by `just card header`: notes go below the end marker.
//! Night's Whisper {1}{B}
//! Sorcery
//!
//! You draw two cards and lose 2 life.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::NightsWhisper
    .abilities(&[Ability::spell().effect(&sequence(&[draw(you(), 2), lose_life(you(), 2)]))])
    .card();
