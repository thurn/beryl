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

/// "+1: Draw a card. At the beginning of the next end step, untap up to two lands." The second
/// sentence creates a delayed triggered ability
/// ([CR 603.7a](https://yawgatog.com/resources/magic-rules/#R6037a)), whose source is Teferi
/// ([CR 603.7e](https://yawgatog.com/resources/magic-rules/#R6037e)): "If an activated or
/// triggered ability creates a delayed triggered ability, the source of that delayed
/// triggered ability is the same as the source of that other ability." It triggers whether or
/// not Teferi is still on the battlefield, since it refers to no object.
///
/// "−3: Put target nonland permanent into its owner's library third from the top." Under two
/// cards, or on the bottom of a library with fewer
/// ([CR 401.7](https://yawgatog.com/resources/magic-rules/#R4017)): "If an effect causes a
/// player to put a card into a library 'Nth from the top,' and that library has fewer than N
/// cards in it, the player puts that card on the bottom of that library." It goes to its
/// owner's library ([CR 400.3](https://yawgatog.com/resources/magic-rules/#R4003)).
///
/// "Untap up to two lands": any lands, whoever controls them, chosen as it resolves.
pub static CARD: CardDef = Printed::TeferiHeroOfDominaria
    .abilities(&[
        Ability::loyalty(1).effect(&sequence(&[draw(you(), 1), delay(UNTAP)])),
        Ability::loyalty(-3)
            .target(&TargetSpec::permanent(Filter::Not(&Filter::Types(
                Types::LAND,
            ))))
            .effect(&put_into_library(target(), LibraryPosition::FromTop(3))),
        Ability::loyalty(-8).effect(&get_emblem(you(), &EMBLEM)),
        Ability::triggered(&Trigger::step(TurnStep::End, Players::EachPlayer).next()).effect(
            &untap_up_to(
                you(),
                2,
                Objects::All {
                    zone: Zone::Battlefield,
                    filter: Filter::Types(Types::LAND),
                },
            ),
        ),
    ])
    .card()
    .creating(&[&EMBLEM]);

/// The emblem of the −8 ([CR 114.1](https://yawgatog.com/resources/magic-rules/#R1141)): "An
/// emblem is a marker used to represent an object that has one or more abilities, but usually
/// no other characteristics." Teferi's controller gets it
/// ([CR 114.2](https://yawgatog.com/resources/magic-rules/#R1142)): "This means that [player]
/// puts an emblem with [ability] into the command zone. The emblem is both owned and
/// controlled by that player."
///
/// "Whenever you draw a card, exile target permanent an opponent controls."
/// [CR 701.13a](https://yawgatog.com/resources/magic-rules/#R70113a): "To exile an object,
/// move it to the exile zone from wherever it is."
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

    /// −3 on P1's creature, whose library is `before`; `then` checks where it went. The Island
    /// is a land and can't be chosen; Teferi himself could.
    const fn tuck(
        name: &'static str,
        before: &'static [&'static str],
        then: &'static [Action],
    ) -> Scenario {
        Scenario::new(name)
            .battlefield(P0, &[TEFERI])
            .hand(P0, &[Card::Plains])
            .battlefield(P1, &[Prop::Creature, Card::Island])
            .library(P1, before)
            .actions(then)
    }

    pub const SCENARIOS: &[Scenario] = &[
        // P0 taps both Plains for two Portable Holes, which find nothing to exile; P1's Island is
        // tapped too. The delayed ability may untap either player's lands, and P0 takes their own.
        Scenario::new("+1 draws, and at the end step untaps up to two lands")
            .battlefield(P0, &[TEFERI, Card::Plains, Card::Plains])
            .hand(P0, &[Prop::Artifact, Prop::Artifact])
            .library(P0, &[Card::Island, Card::Plains])
            .permanents(P1, &[Permanent::new(Card::Island).tapped()])
            .actions(&[
                ME.casts(Prop::Artifact),
                Resolve,
                ME.casts(Prop::Artifact),
                Resolve,
                ME.activates(TEFERI, 0), // +1
                Resolve,
                ME.has(Zone::Hand, Card::Island, 1),
                ME.has_loyalty(TEFERI, 5),
                // Nothing untaps before the end step.
                ME.tapped(Card::Plains, 2),
                THEM.tapped(Card::Island, 1),
                advance_to(1, TurnStep::End),
                Resolve,
                ME.picks_up_to(Card::Plains),
                ME.picks_up_to(Card::Plains),
                // Read at the next decision, in turn 2, after P1's own untap step.
                ME.tapped(Card::Plains, 0),
            ]),
        // The delayed ability refers to no object, so it untaps lands though Teferi is gone: P0
        // taps both Plains for two Portable Holes, and P1 returns Teferi to P0's hand with Sink
        // into Stupor. P0 untaps one Plains and stops.
        Scenario::new("+1's untap comes at the end step after Teferi has left")
            .battlefield(P0, &[TEFERI, Card::Plains, Card::Plains])
            .hand(P0, &[Prop::Artifact, Prop::Artifact])
            .battlefield(P1, &[Card::Island, Card::Island, Card::Island])
            .hand(P1, &[Prop::Removal])
            .actions(&[
                ME.casts(Prop::Artifact),
                Resolve,
                ME.casts(Prop::Artifact),
                Resolve,
                ME.activates(TEFERI, 0), // +1
                Resolve,
                ME.passes(),
                THEM.casts(Prop::Removal),
                THEM.targets(TEFERI),
                ME.has(Zone::Hand, TEFERI, 1),
                ME.tapped(Card::Plains, 2),
                advance_to(1, TurnStep::End),
                Resolve,
                ME.picks_up_to(Card::Plains),
                ME.done_picking(),
                ME.tapped(Card::Plains, 1),
            ]),
        tuck(
            "−3 puts a nonland permanent third from the top of its owner's library",
            &[Card::Plains, Card::Plains, Card::Island],
            &[
                ME.activates(TEFERI, 1), // −3
                ME.targets(Prop::Creature),
                Resolve,
                THEM.library_top(&[
                    Card::Plains,
                    Card::Plains,
                    Prop::Creature,
                    Card::Island,
                ]),
                THEM.has(Zone::Battlefield, Prop::Creature, 0),
                ME.has_loyalty(TEFERI, 1),
            ],
        ),
        tuck(
            "−3 into a library of fewer than two cards puts it on the bottom",
            &[Card::Plains],
            &[
                ME.activates(TEFERI, 1), // −3
                ME.targets(Prop::Creature),
                Resolve,
                THEM.library_top(&[Card::Plains, Prop::Creature]),
                ME.has_loyalty(TEFERI, 1),
            ],
        ),
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
        // The Mind Sculptor's 0 draws three cards, three draws: the emblem triggers three
        // times (CR 603.2c), each trigger targeting as it goes on the stack.
        Scenario::new("−8's emblem triggers once for each card of a multiple draw")
            .permanents(
                P0,
                &[
                    Permanent::new(TEFERI).counters(CounterKind::Loyalty, 9),
                    Permanent::new(Prop::Planeswalker),
                ],
            )
            .library(P0, &[Card::Island, Card::Island, Card::Plains, Card::Plains])
            .battlefield(
                P1,
                &[
                    Prop::Creature,
                    Prop::Artifact,
                    Prop::SmallCreature,
                    Card::Island,
                ],
            )
            .actions(&[
                ME.activates(TEFERI, 2), // −8
                Resolve,
                ME.activates(Prop::Planeswalker, 1), // 0
                Resolve,
                ME.picks(Card::Island),
                ME.picks(Card::Plains),
                ME.targets(Prop::Creature),
                ME.targets(Prop::Artifact),
                ME.targets(Prop::SmallCreature),
                stack(3),
                Resolve,
                Resolve,
                Resolve,
                THEM.has_cards(Zone::Exile, 3),
                THEM.controls(Card::Island, 1),
            ]),
        // UI regressions by-svq/by-2ml: useful colors among three lands, then undo next turn.
        Scenario::new("+1 untaps a dual land with an instant in hand")
            .battlefield(
                P0,
                &[TEFERI, Card::Plains, Card::Island, Card::Tundra],
            )
            .hand(P0, &[Prop::Instant, Prop::Creature])
            .library(P0, &[Card::Plains, Card::Island])
            .library(P1, &[Card::Island, Card::Plains])
            .actions(&[
                // Pay for a spell so these lands are tapped in every engine, after its untap step.
                // The creature, then its enters trigger, which sees an empty hand.
                ME.casts(Prop::Creature),
                Resolve,
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
