// Generated from the card store by `just card header`: notes go below the end marker.
//! Phyrexian Altar {3}
//! Artifact
//!
//! Sacrifice a creature: Add one mana of any color.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::PhyrexianAltar
    .abilities(&[Ability::mana(
        Cost::of_parts(&[CostPart::sacrifice(Filter::Types(Types::CREATURE))]),
        ManaSet::any_color(1),
    )])
    .card();
