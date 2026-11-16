// Generated from the card store by `just card header`: notes go below the end marker.
//! Mana Drain {U}{U}
//! Instant
//!
//! Counter target spell. At the beginning of your next main phase, add an amount of {C} equal to
//! that spell's mana value.
// End of the generated card header.

use crate::prelude::*;

/// The mana value is the spell's as it was on the stack, X included ([CR 202.3e]): "X is treated as
/// the number chosen for it while the object is on the stack." The second sentence creates a
/// delayed triggered ability ([CR 603.7a]): "Delayed triggered abilities are created during the
/// resolution of spells or abilities".
///
/// The delayed ability is the face's ability 1.
///
/// [CR 202.3e]: https://yawgatog.com/resources/magic-rules/#R2023e
/// [CR 603.7a]: https://yawgatog.com/resources/magic-rules/#R6037a
pub static CARD: CardDef =
    Printed::ManaDrain
        .abilities(&[
            Ability::spell()
                .target(&TargetSpec::spell())
                .effect(&sequence(&[
                    counter(target()),
                    delay_carrying(1, Amount::ManaValue(target())),
                ])),
            // CR 106.4: "When an effect instructs a player to add mana, that mana goes into
            // a player's mana pool."
            Ability::triggered(&Trigger::main_phase(Players::You).next())
                .effect(&add_mana_of_kind(you(), ManaColor::Colorless, carried())),
        ])
        .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    /// P0 counters P1's Timetwister (mana value 3) in P1's precombat main phase. The mana waits
    /// for P0's own next main phase: not P1's postcombat main, but P0's precombat main of turn 3,
    /// where the delayed trigger goes on the stack as the phase begins.
    ///
    /// Cast in P0's own precombat main phase, against the Mana Tithe P1 aims at P0's Mana Crypt,
    /// the next main phase is the postcombat main of the same turn. The Plains in P0's hand
    /// keeps P0's priority a decision there.
    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new(
            "Mana Drain: the countered spell's mana value at the caster's next main phase",
        )
        .at(2, P1, TurnStep::PrecombatMain)
        .battlefield(P0, &[Card::Island, Card::Island])
        .hand(P0, &[Card::ManaDrain])
        .library(P0, &[Card::Plains])
        .battlefield(P1, &[Card::Island; 3])
        .hand(P1, &[Card::Timetwister])
        .actions(&[
            THEM.casts(Card::Timetwister),
            ME.casts(Card::ManaDrain),
            THEM.has(Zone::Graveyard, Card::Timetwister, 1),
            advance_to(2, TurnStep::PostcombatMain),
            ME.mana(ManaColor::Colorless, 0),
            advance_to(3, TurnStep::PrecombatMain),
            ME.passes(),
            ME.mana(ManaColor::Colorless, 3),
        ]),
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
