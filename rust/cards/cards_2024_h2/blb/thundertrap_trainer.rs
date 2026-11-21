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

pub static CARD: CardDef = Printed::ThundertrapTrainer
    .abilities(&[
        Ability::offspring(),
        when_enters(&dig(
            you(),
            4,
            Taken::MayReveal(Filter::Not(&Filter::Types(
                Types::CREATURE.union(Types::LAND),
            ))),
            LibraryOrder::Random,
        )),
    ])
    .casting(&[CastingOption::Offspring(ManaCost::generic(4))])
    .excepts(&[CopyException::ONE_ONE])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const TRAINER: &str = Card::ThundertrapTrainer;

    /// Eight lands: no card among the top four qualifies, so nothing is asked.
    const LANDS: &[&str] = &[Card::Plains; 8];

    pub const SCENARIOS: &[Scenario] = &[
        // Counterspell and Oust qualify among the top four; the Trainer takes Oust. Portable
        // Hole, fifth, would qualify too, but the Trainer looks at four.
        Scenario::new("Thundertrap Trainer takes a spell")
            .hand(P0, &[TRAINER])
            .battlefield(P0, &[Card::Island; 2])
            .library(
                P0,
                &[
                    Card::Plains,
                    Prop::Instant,
                    Prop::Flier,
                    Card::Oust,
                    Prop::Artifact,
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
                ME.has(Zone::Hand, Prop::Artifact, 1),
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
