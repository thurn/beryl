// Generated from the card store by `just card header`: notes go below the end marker.
//! Ashen Rider {4}{W}{W}{B}{B}
//! Creature — Archon, 5/5
//!
//! Flying
//! When this creature enters or dies, exile target permanent.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::AshenRider
    .keywords(Keywords::FLYING)
    .abilities(&[
        when_enters(&exile(target())).target(&TargetSpec::permanent(Filter::Any)),
        Ability::triggered(&Trigger::DIES)
            .target(&TargetSpec::permanent(Filter::Any))
            .effect(&exile(target())),
    ])
    .card();
