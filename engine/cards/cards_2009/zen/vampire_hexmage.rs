// Generated from the card store by `just card header`: notes go below the end marker.
//! Vampire Hexmage {B}{B}
//! Creature — Vampire Shaman, 2/1
//!
//! First strike
//! Sacrifice this creature: Remove all counters from target permanent.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::VampireHexmage
    .keywords(Keywords::FIRST_STRIKE)
    .abilities(&[Ability::activated(Cost::part(&CostPart::SACRIFICE))
        .target(&TargetSpec::permanent(Filter::Any))
        .effect(&remove_all_counters(target()))])
    .card();
