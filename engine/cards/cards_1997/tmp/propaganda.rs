// Generated from the card store by `just card header`: notes go below the end marker.
//! Propaganda {2}{U}
//! Enchantment
//!
//! Creatures can't attack you unless their controller pays {2} for each creature they control
//! that's attacking you.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::Propaganda
    .abilities(&[Ability::rule_change(&RuleChange {
        players: Players::You,
        what: Governed::AttackCost(ManaCost::generic(2)),
        when: When::AsUsual,
    })])
    .card();
