// Generated from the card store by `just card header`: notes go below the end marker.
//! Lure {1}{G}{G}
//! Enchantment — Aura
//!
//! Enchant creature
//! All creatures able to block enchanted creature do so.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::Lure
    .abilities(&[
        Ability::spell().target(&TargetSpec::creature()),
        Ability::rule_change(&RuleChange {
            players: Players::You,
            what: Governed::MustBlock(Objects::AttachedTo),
            when: When::AsUsual,
        }),
    ])
    .card();
