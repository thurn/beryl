// Generated from the card store by `just card header`: notes go below the end marker.
//! Tarfire {R}
//! Kindred Instant — Goblin
//!
//! Tarfire deals 2 damage to any target.
// End of the generated card header.

use crate::prelude::*;

/// Implemented for scenarios that count card types in graveyards. This one card
/// contributes both kindred and instant.
pub static CARD: CardDef = Printed::Tarfire
    .abilities(&[Ability::spell()
        .target(&TargetSpec::any_target())
        .effect(&deal_damage_to_any_target(2))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const TARFIRE: &str = Card::Tarfire;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Tarfire deals 2 damage to a player")
            .hand(P0, &[TARFIRE])
            .battlefield(P0, &[Card::Mountain])
            .actions(&[
                ME.casts(TARFIRE),
                ME.targets_player(THEM),
                Resolve,
                THEM.life(18),
            ]),
        Scenario::new("Tarfire deals 2 damage to a creature")
            .hand(P0, &[TARFIRE])
            .battlefield(P0, &[Card::Mountain])
            .battlefield(P1, &[Prop::Vanilla])
            .actions(&[
                ME.casts(TARFIRE),
                ME.targets(Prop::Vanilla),
                Resolve,
                THEM.has(Zone::Graveyard, Prop::Vanilla, 1),
            ]),
    ];
}
