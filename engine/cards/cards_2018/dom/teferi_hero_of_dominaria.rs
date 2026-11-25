// Generated from the card store by `just card header`: notes go below the end marker.
//! Teferi, Hero of Dominaria {3}{W}{U}
//! Legendary Planeswalker — Teferi, loyalty 4
//!
//! +1: Draw a card. At the beginning of the next end step, untap up to two lands.
//! −3: Put target nonland permanent into its owner's library third from the top.
//! −8: You get an emblem with "Whenever you draw a card, exile target permanent an opponent
//! controls."
// End of the generated card header.

use crate::prelude::*;

/// The place of the delayed untap among the face's abilities.
const UNTAP: u8 = 3;

/// The delayed ability has Teferi as its source and can untap lands after he leaves.
/// Under [CR 603.7e], “the source of that delayed triggered ability is the same as the source
/// of that other ability.”
///
/// [CR 603.7e]: https://yawgatog.com/resources/magic-rules/#R6037e
pub static CARD: CardDef = Printed::TeferiHeroOfDominaria
    .abilities(&[
        Ability::loyalty(1).effect(&sequence(&[draw(you(), 1), delay(UNTAP)])),
        Ability::loyalty(-3)
            .target(&TargetSpec::permanent(nonland()))
            .effect(&put_into_library(target(), LibraryPosition::FromTop(3))),
        Ability::loyalty(-8).effect(&get_emblem(you(), &EMBLEM)),
        Ability::triggered(&Trigger::step(TurnStep::End, Players::EachPlayer).next())
            .effect(&untap_up_to(you(), 2, permanents(land()))),
    ])
    .card()
    .creating(&[&EMBLEM]);

pub static EMBLEM: CardDef = card(
    "Teferi, Hero of Dominaria Emblem",
    &[
        Face::EMPTY.abilities(&[Ability::triggered(&Trigger::draws(Players::You))
            .effect(&exile(target()))
            .target(&TargetSpec::permanent(Filter::Whose(
                &Players::EachOpponent,
            )))]),
    ],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const TEFERI: &str = Card::TeferiHeroOfDominaria;

    pub const SCENARIOS: &[Scenario] = &[
        // −8 from 9 leaves Teferi with 1. The emblem exiles a permanent for the draw step's card,
        // and again for the card of Teferi's own +1.
        Scenario::new("−8's emblem exiles a permanent whenever its owner draws")
            .permanents(
                P0,
                &[Permanent::new(TEFERI).counters(CounterKind::Loyalty, 9)],
            )
            .hand(P0, &[Card::Plains])
            .library(P0, &[Card::Island, Card::Island])
            .battlefield(P1, &[Prop::Artifact, Prop::Artifact, Card::Island])
            .actions(&[
                ME.activates(TEFERI, 2), // −8
                Resolve,
                ME.has_loyalty(TEFERI, 1),
                advance_to(3, TurnStep::Upkeep),
                // P1 can only pass, so the draw step follows.
                ME.passes(),
                ME.targets(Prop::Artifact),
                Resolve,
                THEM.has(Zone::Exile, Prop::Artifact, 1),
                advance_to(3, TurnStep::PrecombatMain),
                ME.activates(TEFERI, 0), // +1
                Resolve,
                // A land is a permanent too.
                ME.targets(Card::Island),
                Resolve,
                THEM.has_cards(Zone::Exile, 2),
                THEM.controls(Prop::Artifact, 1),
            ]),
        // UI regressions by-svq/by-2ml: useful colors among three lands, then undo next turn.
        Scenario::new("+1 untaps a dual land with an instant in hand")
            .battlefield(P0, &[TEFERI, Card::Plains, Card::Island, Card::Tundra])
            .hand(P0, &[Prop::Instant, Prop::Flier])
            .library(P0, &[Card::Plains, Card::Island])
            .library(P1, &[Card::Island, Card::Plains])
            .actions(&[
                // Pay for a spell so these lands are tapped in every engine, after its untap step.
                ME.casts(Prop::Flier),
                Resolve,
                ME.activates(TEFERI, 0), // +1
                Resolve,
                advance_to(1, TurnStep::End),
                Resolve,
                ME.picks_up_to(Card::Tundra),
                ME.picks_up_to(Card::Island),
                ME.tapped(Card::Tundra, 0),
                ME.tapped(Card::Island, 0),
            ]),
    ];
}
