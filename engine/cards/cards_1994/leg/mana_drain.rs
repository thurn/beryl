// Generated from the card store by `just card header`: notes go below the end marker.
//! Mana Drain {U}{U}
//! Instant
//!
//! Counter target spell. At the beginning of your next main phase, add an amount of {C} equal to
//! that spell's mana value.
// End of the generated card header.

use crate::prelude::*;

/// The delayed ability uses the countered spell's mana value as it was on the stack,
/// including the chosen X ([CR 202.3e]): "X is treated as the number chosen for it while
/// the object is on the stack."
///
/// [CR 202.3e]: https://yawgatog.com/resources/magic-rules/#R2023e
pub static CARD: CardDef =
    Printed::ManaDrain
        .abilities(&[
            Ability::spell()
                .target(&TargetSpec::spell())
                .effect(&sequence(&[
                    counter(target()),
                    delay_carrying(1, Amount::ManaValue(target())),
                ])),
            Ability::triggered(&Trigger::main_phase(Players::You).next())
                .effect(&add_mana_of_kind(you(), ManaColor::Colorless, carried())),
        ])
        .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    /// On the caster's turn, the delayed trigger can fire in that turn's postcombat main phase.
    pub const SCENARIOS: &[Scenario] = &[
        // P1 counters P0's artifact with Counterspell, and P0 drains the Counterspell.
        Scenario::new("Mana Drain on the caster's own turn: mana at that turn's postcombat main")
            .battlefield(P0, &[Card::Island, Card::Island, Card::Plains])
            .hand(P0, &[Prop::Artifact, Card::ManaDrain, Card::Plains])
            .battlefield(P1, &[Card::Island, Card::Island])
            .hand(P1, &[Prop::Instant])
            .actions(&[
                ME.casts(Prop::Artifact),
                ME.passes(),
                THEM.casts(Prop::Instant),
                ME.casts(Card::ManaDrain),
                ME.targets(Prop::Instant),
                THEM.has(Zone::Graveyard, Prop::Instant, 1),
                ME.has(Zone::Battlefield, Prop::Artifact, 1),
                ME.mana(ManaColor::Colorless, 0),
                advance_to(1, TurnStep::PostcombatMain),
                ME.passes(),
                ME.mana(ManaColor::Colorless, 2),
            ]),
    ];
}
