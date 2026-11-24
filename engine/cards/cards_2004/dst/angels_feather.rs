// Generated from the card store by `just card header`: notes go below the end marker.
//! Angel's Feather {2}
//! Artifact
//!
//! Whenever a player casts a white spell, you may gain 1 life.
// End of the generated card header.

use crate::prelude::*;

/// Implemented for scenarios that change the color of a spell.
pub static CARD: CardDef = Printed::AngelsFeather
    .abilities(&[Ability::triggered(&Trigger::casts(
        Players::EachPlayer,
        Filter::Colors(Colors::WHITE),
    ))
    .effect(&may(you(), &gain_life(you(), 1)))])
    .card();
