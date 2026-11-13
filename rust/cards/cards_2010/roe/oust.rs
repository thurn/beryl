// Generated from the card store by `just card header`: notes go below the end marker.
//! Oust {W}
//! Sorcery
//!
//! Put target creature into its owner's library second from the top. Its controller gains 3 life.
// End of the generated card header.

use crate::prelude::*;

/// "Second from the top" is under one card, or the top of an empty library. "Its controller"
/// is the creature's as it was on the battlefield: in a library it has no controller
/// ([CR 108.4a](https://yawgatog.com/resources/magic-rules/#R1084a)): "If anything asks for
/// the controller of a card that doesn't have one (because it's not a permanent or spell),
/// use its owner instead." Owner and controller differ only for a stolen creature, which
/// goes to its owner's library while its controller gains the life.
pub static CARD: CardDef = Printed::Oust
    .abilities(&[Ability::spell()
        .target(&TargetSpec::creature())
        .effect(&sequence(&[
            put_into_library(target(), LibraryPosition::FromTop(2)),
            gain_life(its_controller(), 3),
        ]))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const SPELLBINDER: &str = Card::EliteSpellbinder;

    /// P0 casts Oust on P1's Elite Spellbinder, whose library is `top`. The Plains left in P0's
    /// hand keeps the game in P0's main phase once Oust has resolved.
    const fn oust_onto(name: &'static str, top: &'static [&'static str]) -> Scenario {
        Scenario::new(name)
            .battlefield(P0, &[Card::Plains])
            .hand(P0, &[Card::Oust, Card::Plains])
            .battlefield(P1, &[SPELLBINDER])
            .library(P1, top)
    }

    pub const SCENARIOS: &[Scenario] = &[
        // With no legal target it can't be cast
        // ([CR 601.2c](https://yawgatog.com/resources/magic-rules/#R6012c)): P1's only
        // permanent is a noncreature artifact, so passing is P0's only option.
        Scenario::new("Oust can't be cast without a target")
            .battlefield(P0, &[Card::Plains; 3])
            .hand(P0, &[Card::Oust])
            .battlefield(P1, &[Prop::Artifact])
            .library(P1, &[Card::Island])
            .actions(&[
                advance_to(1, TurnStep::PrecombatMain),
                ME.deciding(DecisionKind::Priority, 1),
            ]),
        // Second from the top of an empty library is its top: the creature is its only card.
        oust_onto(
            "Oust on an empty library makes the creature its only card",
            &[],
        )
        .actions(&[
            ME.casts(Card::Oust),
            THEM.library_top(&[SPELLBINDER]),
            THEM.life(23),
            ME.life(20),
        ]),
        oust_onto(
            "Oust on a library of one puts the creature under it",
            &[Card::Island],
        )
        .actions(&[
            ME.casts(Card::Oust),
            THEM.library_top(&[Card::Island, SPELLBINDER]),
        ]),
        oust_onto(
            "Oust puts the creature second from the top and its controller gains 3",
            &[Card::Island, Card::Plains],
        )
        .actions(&[
            ME.casts(Card::Oust),
            THEM.library_top(&[Card::Island, SPELLBINDER, Card::Plains]),
            THEM.has(Zone::Battlefield, SPELLBINDER, 0),
            ME.has(Zone::Graveyard, Card::Oust, 1),
            THEM.life(23),
        ]),
        // UI regression by-97z: the lower spell's target stays legible beneath a response. P0's
        // own Counterspell keeps each of P0's priorities a decision.
        Scenario::new("Oust beneath an opposing counterspell")
            .battlefield(P0, &[Card::Plains, Card::Island, Card::Island])
            .hand(P0, &[Card::Oust, Prop::Instant, Card::Plains])
            .battlefield(P1, &[SPELLBINDER, Card::Island, Card::Island])
            .hand(P1, &[Prop::Instant])
            .actions(&[
                ME.casts(Card::Oust),
                ME.passes(),
                THEM.casts(Prop::Instant),
                ME.passes(),
                THEM.controls(SPELLBINDER, 1),
            ]),
    ];
}
