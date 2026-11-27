// Generated from the card store by `just card header`: notes go below the end marker.
//! Bloodrock Cyclops {2}{R}
//! Creature — Cyclops, 3/3
//!
//! This creature attacks each combat if able.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::BloodrockCyclops
    .abilities(&[Ability::rule_change(&RuleChange {
        players: Players::You,
        what: Governed::MustAttack(Objects::This),
        when: When::AsUsual,
    })])
    .card();
