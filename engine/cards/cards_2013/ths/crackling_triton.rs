// Generated from the card store by `just card header`: notes go below the end marker.
//! Crackling Triton {2}{U}
//! Creature — Merfolk Wizard, 2/3
//!
//! {2}{R}, Sacrifice this creature: It deals 2 damage to any target.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::CracklingTriton
    .abilities(&[
        Ability::activated(Cost::of(mana("{2}{R}"), &[CostPart::SACRIFICE]))
            .target(&TargetSpec::any_target())
            .effect(&deal_damage_to_any_target(2)),
    ])
    .card();
