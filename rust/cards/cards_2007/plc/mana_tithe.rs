// Generated from the card store by `just card header`: notes go below the end marker.
//! Mana Tithe {W}
//! Instant
//!
//! Counter target spell unless its controller pays {1}.
// End of the generated card header.

use crate::prelude::*;

/// [CR 118.12a]: "[Do something] unless [a player does something else]" means "[A player may do
/// something else]. If [that player doesn't], [do something]." A controller who can't pay is never
/// asked.
///
/// [CR 118.12a]: https://yawgatog.com/resources/magic-rules/#R11812a
pub static CARD: CardDef = Printed::ManaTithe
    .abilities(&[Ability::spell()
        .target(&TargetSpec::spell())
        .effect(&unless_pays(
            &counter(target()),
            its_controller(),
            ManaCost::generic(1),
        ))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    /// P1 has a card to draw, so the game waits in their turn 2 once the stack is empty. The
    /// artifact is the spell to counter: it costs {1}, so one of its controller's two lands
    /// stays free to pay the tithe.
    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Mana Tithe: the controller pays {1}")
            .battlefield(P0, &[Card::Plains, Card::Plains])
            .hand(P0, &[Prop::Artifact])
            .battlefield(P1, &[Card::Plains])
            .hand(P1, &[Card::ManaTithe])
            .actions(&[
                ME.casts(Prop::Artifact),
                THEM.casts(Card::ManaTithe),
                ME.yes(),
                ME.has(Zone::Battlefield, Prop::Artifact, 1),
                ME.tapped(Card::Plains, 2),
                THEM.has(Zone::Graveyard, Card::ManaTithe, 1),
            ]),
        // The planeswalker ({2}{U}{U}) taps four of P0's six lands, and the two left pay the
        // tithe. (A game once showed it countered unasked from six lands: Elite Spellbinder's
        // {2} tax had tapped them all.)
        // The six lands are four for the planeswalker and two to pay with.
        Scenario::new("Mana Tithe: lands left after paying for the spell pay {1}")
            .battlefield(
                P0,
                &[
                    Card::Island,
                    Prop::NonbasicLand,
                    Card::Plains,
                    Card::Plains,
                    Card::Island,
                    Card::Plains,
                ],
            )
            .hand(P0, &[Prop::Planeswalker])
            .battlefield(P1, &[Card::Plains])
            .hand(P1, &[Card::ManaTithe])
            .actions(&[
                ME.casts(Prop::Planeswalker),
                THEM.casts(Card::ManaTithe),
                ME.yes(),
                advance_to(1, TurnStep::End),
                ME.controls(Prop::Planeswalker, 1),
                THEM.has(Zone::Graveyard, Card::ManaTithe, 1),
            ]),
        Scenario::new("Mana Tithe: a controller who can't pay isn't asked")
            .battlefield(P0, &[Card::Plains])
            .hand(P0, &[Prop::Artifact])
            .battlefield(P1, &[Card::Plains])
            .hand(P1, &[Card::ManaTithe])
            .actions(&[
                ME.casts(Prop::Artifact),
                THEM.casts(Card::ManaTithe),
                ME.has(Zone::Graveyard, Prop::Artifact, 1),
                ME.has(Zone::Battlefield, Prop::Artifact, 0),
            ]),
    ];
}
