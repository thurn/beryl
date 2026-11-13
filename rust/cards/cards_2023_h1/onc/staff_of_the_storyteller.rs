// Generated from the card store by `just card header`: notes go below the end marker.
//! Staff of the Storyteller {1}{W}
//! Artifact
//!
//! When this artifact enters, create a 1/1 white Spirit creature token with flying.
//! Whenever you create one or more creature tokens, put a story counter on this artifact.
//! {W}, {T}, Remove a story counter from this artifact: Draw a card.
// End of the generated card header.

use crate::prelude::*;

/// The Spirit token's name ([CR 111.4]): its subtype plus the word "Token".
///
/// [CR 111.4]: https://yawgatog.com/resources/magic-rules/#R1114
pub const SPIRIT_TOKEN: &str = "Spirit Token";

/// "A 1/1 white Spirit creature token with flying"
pub static SPIRIT: CardDef = Token::SpiritToken.keywords(Keywords::FLYING).card();

/// "When this artifact enters, create a 1/1 white Spirit creature token with flying." [CR 111.2]:
/// "The player who creates a token is its owner. The token enters the battlefield under that
/// player's control."
///
/// [CR 111.2]: https://yawgatog.com/resources/magic-rules/#R1112
pub static CARD: CardDef = Printed::StaffOfTheStoryteller
    .abilities(&[
        Ability::triggered(&Trigger::ENTERS).effect(&create_tokens(you(), &SPIRIT, 1)),
        Ability::triggered(&Trigger::creates_tokens(
            Players::You,
            Filter::Types(Types::CREATURE),
        ))
        .effect(&add_counters(this(), CounterKind::Story, 1)),
        Ability::activated(Cost::of(
            mana("{W}"),
            &[CostPart::TAP, CostPart::remove(CounterKind::Story)],
        ))
        .effect(&draw(you(), 1)),
    ])
    .card()
    .creating(&[&SPIRIT]);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    use super::SPIRIT_TOKEN;

    const STAFF: &str = Card::StaffOfTheStoryteller;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("its own Spirit and a Samurai each add a story counter, spent to draw")
            .interaction(&[Card::TheWanderingEmperor])
            .hand(P0, &[STAFF])
            .battlefield(
                P0,
                &[
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::TheWanderingEmperor,
                ],
            )
            .library(P0, &[Card::Island, Card::Island, Card::Island])
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.casts(STAFF),
                Resolve,
                // The enters trigger: the Spirit, and its story-counter trigger.
                Resolve,
                Resolve,
                ME.counters(STAFF, CounterKind::Story, 1),
                ME.controls(SPIRIT_TOKEN, 1),
                ME.activates(Card::TheWanderingEmperor, 2), // -1
                Resolve,
                Resolve,
                ME.counters(STAFF, CounterKind::Story, 2),
                ME.activates(STAFF, 2), // {W}, {T}, remove a story counter
                ME.counters(STAFF, CounterKind::Story, 1),
                ME.has(Zone::Hand, Card::Island, 1),
                ME.tapped(STAFF, 1),
                // Tapped, it can't draw again this turn: passing, or playing the Island drawn.
                ME.deciding(DecisionKind::Priority, 2),
            ]),
        // CR 603.2c: Overlord's attack makes two Insects in one event, so one story counter.
        Scenario::new("two tokens created at once add one story counter")
            .interaction(&[Card::OverlordOfTheMistmoors])
            .battlefield(P0, &[STAFF, Card::OverlordOfTheMistmoors])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Plains])
            .actions(&[
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(Card::OverlordOfTheMistmoors),
                ME.controls(crate::tokens::INSECT_TOKEN, 2),
                ME.counters(STAFF, CounterKind::Story, 1),
            ]),
        // UI regression by-xd4: resolving a response must not demand a pass on one's own spell.
        Scenario::new("Staff resolves after an opposing flash creature")
            .battlefield(P0, &[Card::Plains, Card::Plains])
            .hand(P0, &[STAFF, Card::Plains])
            .battlefield(P1, &[Card::Island, Card::Island])
            .hand(P1, &[Card::SnapcasterMage])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.casts(STAFF),
                THEM.casts(Card::SnapcasterMage),
                ME.counters(STAFF, CounterKind::Story, 1),
            ]),
    ];
}
