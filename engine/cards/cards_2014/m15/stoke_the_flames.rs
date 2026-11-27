// Generated from the card store by `just card header`: notes go below the end marker.
//! Stoke the Flames {2}{R}{R}
//! Instant
//!
//! Convoke (Your creatures can help cast this spell. Each creature you tap while casting this spell
//! pays for {1} or one mana of that creature's color.)
//! Stoke the Flames deals 4 damage to any target.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::StokeTheFlames
    .keywords(Keywords::CONVOKE)
    .abilities(&[Ability::spell()
        .target(&TargetSpec::any_target())
        .effect(&deal_damage_to_any_target(4))])
    .card();
