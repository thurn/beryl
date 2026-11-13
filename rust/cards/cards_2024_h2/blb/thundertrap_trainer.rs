// Generated from the card store by `just card header`: notes go below the end marker.
//! Thundertrap Trainer {1}{U}
//! Creature — Otter Wizard, 1/2
//!
//! Offspring {4} (You may pay an additional {4} as you cast this spell. If you do, when this
//! creature enters, create a 1/1 token copy of it.)
//! When this creature enters, look at the top four cards of your library. You may reveal a
//! noncreature, nonland card from among them and put it into your hand. Put the rest on the bottom
//! of your library in a random order.
// End of the generated card header.

use crate::prelude::*;

/// "Look at the top four cards of your library. You may reveal a noncreature, nonland card from
/// among them and put it into your hand. Put the rest on the bottom of your library in a random
/// order." The look, the optional reveal ([CR 701.20a]): "To reveal a card, show that card to all
/// players for a brief time", and the rest to the bottom in a random order ([CR 401.4]): "That
/// library's owner doesn't reveal the order in which the cards go into the library."
///
/// [CR 701.20a]: https://yawgatog.com/resources/magic-rules/#R70120a
/// [CR 401.4]: https://yawgatog.com/resources/magic-rules/#R4014
pub static CARD: CardDef = Printed::ThundertrapTrainer
    .abilities(&[
        Ability::offspring(),
        Ability::triggered(&Trigger::ENTERS).effect(&dig(
            you(),
            4,
            Taken::MayReveal(Filter::Not(&Filter::Types(
                Types::CREATURE.union(Types::LAND),
            ))),
            LibraryOrder::Random,
        )),
    ])
    .casting(&[CastingOption::Offspring(ManaCost::generic(4))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const TRAINER: &str = Card::ThundertrapTrainer;

    /// Eight lands: no card among the top four qualifies, so nothing is asked.
    const LANDS: &[&str] = &[Card::Plains; 8];

    pub const SCENARIOS: &[Scenario] = &[
        // Counterspell and Oust qualify among the top four; the Trainer takes Oust.
        Scenario::new("Thundertrap Trainer takes a spell")
            .hand(P0, &[TRAINER])
            .battlefield(P0, &[Card::Island; 2])
            .library(
                P0,
                &[
                    Card::Plains,
                    Prop::Instant,
                    Prop::Creature,
                    Card::Oust,
                    Card::Island,
                ],
            )
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.casts(TRAINER),
                Resolve,
                Resolve,
                ME.yes(),
                ME.deciding(DecisionKind::ChooseObject, 2),
                ME.picks(Card::Oust),
                ME.has(Zone::Hand, Card::Oust, 1),
                ME.has_cards(Zone::Library, 4),
                // The fifth card, below the four, is on top now.
                advance_to(3, TurnStep::PrecombatMain),
                ME.has(Zone::Hand, Card::Island, 1),
            ]),
        Scenario::new("Thundertrap Trainer finds no spell")
            .hand(P0, &[TRAINER])
            .battlefield(P0, &[Card::Island; 2])
            .library(P0, LANDS)
            .actions(&[
                ME.casts(TRAINER),
                Resolve,
                stack(1),
                Resolve,
                ME.has_cards(Zone::Library, 8),
                ME.has_cards(Zone::Hand, 0),
                ME.controls(TRAINER, 1),
            ]),
        // Offspring paid: two enters triggers, ordered by their controller; the 1/1 token copy
        // has the look of its own, but not offspring's.
        Scenario::new("Thundertrap Trainer with offspring")
            .hand(P0, &[TRAINER])
            .battlefield(P0, &[Card::Island; 6])
            .library(P0, LANDS)
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: TRAINER,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                Resolve,
                ME.chooses(DecisionKind::TriggerOrder, TRAINER),
                stack(2),
                Resolve,
                Resolve,
                // The token's own enters trigger, if offspring's resolved last; otherwise the look.
                Resolve,
                stack(0),
                ME.token_pt(TRAINER, 1, 1),
                ME.pt(TRAINER, 1, 2),
                ME.has_cards(Zone::Library, 8),
                ME.has_cards(Zone::Hand, 0),
                advance_to(1, TurnStep::End),
            ]),
    ];
}
