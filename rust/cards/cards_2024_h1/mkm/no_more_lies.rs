// Generated from the card store by `just card header`: notes go below the end marker.
//! No More Lies {W}{U}
//! Instant
//!
//! Counter target spell unless its controller pays {3}. If that spell is countered this way, exile
//! it instead of putting it into its owner's graveyard.
// End of the generated card header.

use crate::prelude::*;

/// "Unless" as [CR 118.12a](https://yawgatog.com/resources/magic-rules/#R11812a) reads it. The
/// second sentence modifies the first
/// ([CR 608.2c](https://yawgatog.com/resources/magic-rules/#R6082c)): "later text on the card
/// may modify the meaning of earlier text (for example, ... 'Counter target spell. If that
/// spell is countered this way, put it on top of its owner's library instead of into its
/// owner's graveyard.')", so the countered spell goes straight to exile.
pub static CARD: CardDef = Printed::NoMoreLies
    .abilities(&[Ability::spell()
        .target(&TargetSpec::spell())
        .effect(&unless_pays(
            &counter_and_exile(target()),
            its_controller(),
            ManaCost::generic(3),
        ))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    /// P1 has a card to draw, so the game waits in their turn 2 once the stack is empty. The
    /// artifact is the spell to counter: it costs {1}, so three of P0's four lands stay free to
    /// pay.
    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("No More Lies: the controller pays {3}")
            // One land for the artifact, three for the tax.
            .battlefield(P0, &[Card::Plains; 4])
            .hand(P0, &[Prop::Artifact])
            .battlefield(P1, &[Card::Plains, Card::Island])
            .hand(P1, &[Card::NoMoreLies])
            .actions(&[
                ME.casts(Prop::Artifact),
                THEM.casts(Card::NoMoreLies),
                ME.yes(),
                ME.has(Zone::Battlefield, Prop::Artifact, 1),
                ME.tapped(Card::Plains, 4),
                THEM.has(Zone::Graveyard, Card::NoMoreLies, 1),
            ]),
        Scenario::new("No More Lies: declining to pay exiles the spell")
            .battlefield(P0, &[Card::Plains; 4])
            .hand(P0, &[Prop::Artifact])
            .battlefield(P1, &[Card::Plains, Card::Island])
            .hand(P1, &[Card::NoMoreLies])
            .actions(&[
                ME.casts(Prop::Artifact),
                THEM.casts(Card::NoMoreLies),
                ME.no(),
                ME.has(Zone::Exile, Prop::Artifact, 1),
                ME.has(Zone::Graveyard, Prop::Artifact, 0),
                ME.tapped(Card::Plains, 1),
            ]),
        // The planeswalker ({2}{U}{U}) taps four of P0's seven lands, and the three left pay
        // {3}.
        Scenario::new("No More Lies: lands left after paying for the spell pay {3}")
            // Seven lands: four for the planeswalker, three for the tax.
            .battlefield(
                P0,
                &[
                    Card::Island,
                    Prop::NonbasicLand,
                    Card::Plains,
                    Card::Plains,
                    Card::Island,
                    Card::Plains,
                    Card::Plains,
                ],
            )
            .hand(P0, &[Prop::Planeswalker])
            .battlefield(P1, &[Card::Plains, Card::Island])
            .hand(P1, &[Card::NoMoreLies])
            .actions(&[
                ME.casts(Prop::Planeswalker),
                THEM.casts(Card::NoMoreLies),
                ME.yes(),
                advance_to(1, TurnStep::End),
                ME.controls(Prop::Planeswalker, 1),
                THEM.has(Zone::Graveyard, Card::NoMoreLies, 1),
            ]),
        Scenario::new("No More Lies: a controller who can't pay isn't asked")
            .battlefield(P0, &[Card::Plains])
            .hand(P0, &[Prop::Artifact])
            .battlefield(P1, &[Card::Plains, Card::Island])
            .hand(P1, &[Card::NoMoreLies])
            .actions(&[
                ME.casts(Prop::Artifact),
                THEM.casts(Card::NoMoreLies),
                ME.has(Zone::Exile, Prop::Artifact, 1),
                ME.has(Zone::Graveyard, Prop::Artifact, 0),
            ]),
    ];
}
