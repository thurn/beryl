// Generated from the card store by `just card header`: notes go below the end marker.
//! Village Rites {B}
//! Instant
//!
//! As an additional cost to cast this spell, sacrifice a creature.
//! Draw two cards.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::VillageRites
    .additional_cost(&[CostPart::sacrifice(Filter::Types(Types::CREATURE))])
    .abilities(&[Ability::spell().effect(&draw(you(), 2))])
    .card();
