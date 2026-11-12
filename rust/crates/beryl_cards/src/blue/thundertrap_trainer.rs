//! Thundertrap Trainer. {1}{U} Creature — Otter Wizard, 1/2. "Offspring {4} (You may pay an
//! additional {4} as you cast this spell. If you do, when this creature enters, create a 1/1
//! token copy of it.) When this creature enters, look at the top four cards of your library.
//! You may reveal a noncreature, nonland card from among them and put it into your hand. Put
//! the rest on the bottom of your library in a random order."

use crate::names;
use beryl_card::effect::build::{YOU, dig};
use beryl_card::{
    Ability, CardDef, CastingOption, Colors, Effect, Face, Filter, LibraryOrder, ManaCost, Subtype,
    Subtypes, Taken, Trigger, Types, card, mana,
};

const NONCREATURE_NONLAND: Filter = Filter::Not(&Filter::Types(Types::CREATURE.union(Types::LAND)));

/// The look, the optional reveal
/// ([CR 701.20a](https://yawgatog.com/resources/magic-rules/#R70120a)): "To reveal a card,
/// show that card to all players for a brief time", and the rest to the bottom in a random
/// order ([CR 401.4](https://yawgatog.com/resources/magic-rules/#R4014)): "That library's
/// owner doesn't reveal the order in which the cards go into the library."
static DIG: Effect = dig(
    YOU,
    4,
    Taken::MayReveal(NONCREATURE_NONLAND),
    LibraryOrder::Random,
);

pub static CARD: CardDef = card(
    names::THUNDERTRAP_TRAINER,
    &[Face::EMPTY
        .cost(mana("{1}{U}"))
        .types(Types::CREATURE)
        .subtypes(Subtypes::of(&[Subtype::Otter, Subtype::Wizard]))
        .colors(Colors::BLUE)
        .power_toughness(1, 2)
        .abilities(&[
            Ability::offspring(),
            Ability::triggered(&Trigger::ENTERS).effect(&DIG),
        ])
        .casting(&[CastingOption::Offspring(ManaCost::generic(4))])],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_engine::card::{Castings, FaceIx};

    use beryl_engine::{DecisionKind, TurnStep, Zone};

    use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

    use crate::{names, props};

    const TRAINER: &str = names::THUNDERTRAP_TRAINER;

    const ISLANDS: &[Permanent] = &[Permanent::new(names::ISLAND); 6];

    const fn library(count: u8) -> Action {
        Action::Assert {
            assertion: Assertion::Count {
                player: P0,
                zone: Zone::Library,
                card: None,
                count,
            },
        }
    }

    const fn hand(count: u8) -> Action {
        Action::Assert {
            assertion: Assertion::Count {
                player: P0,
                zone: Zone::Hand,
                card: None,
                count,
            },
        }
    }

    /// Eight lands: no card among the top four qualifies, so nothing is asked.
    const LANDS: &[&str] = &[names::PLAINS; 8];

    pub const SCENARIOS: &[Scenario] = &[
        // Counterspell and Oust qualify among the top four; the Trainer takes Oust.
        Scenario::new("Thundertrap Trainer takes a spell")
            .hand(P0, &[TRAINER])
            .battlefield(P0, ISLANDS.split_at(2).0)
            .library(
                P0,
                &[
                    names::PLAINS,
                    props::INSTANT,
                    props::CREATURE,
                    names::OUST,
                    names::ISLAND,
                ],
            )
            .library(P1, &[names::PLAINS])
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: TRAINER,
                    face: FaceIx::FRONT,
                    costs: Castings::NONE,
                },
                Action::Resolve,
                Action::Resolve,
                Action::Answer {
                    player: P0,
                    kind: DecisionKind::YesNo,
                    pick: Pick::Yes,
                },
                Action::Assert {
                    assertion: Assertion::Decision {
                        player: P0,
                        kind: DecisionKind::ChooseObject,
                        candidates: 2,
                    },
                },
                Action::Answer {
                    player: P0,
                    kind: DecisionKind::ChooseObject,
                    pick: Pick::Card { card: names::OUST },
                },
                Action::Assert {
                    assertion: Assertion::Count {
                        player: P0,
                        zone: Zone::Hand,
                        card: Some(names::OUST),
                        count: 1,
                    },
                },
                library(4),
                // The fifth card, below the four, is on top now.
                Action::AdvanceTo {
                    turn: 3,
                    step: TurnStep::PrecombatMain,
                },
                Action::Assert {
                    assertion: Assertion::Count {
                        player: P0,
                        zone: Zone::Hand,
                        card: Some(names::ISLAND),
                        count: 1,
                    },
                },
            ]),
        Scenario::new("Thundertrap Trainer finds no spell")
            .hand(P0, &[TRAINER])
            .battlefield(P0, ISLANDS.split_at(2).0)
            .library(P0, LANDS)
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: TRAINER,
                    face: FaceIx::FRONT,
                    costs: Castings::NONE,
                },
                Action::Resolve,
                Action::Assert {
                    assertion: Assertion::Stack { count: 1 },
                },
                Action::Resolve,
                library(8),
                hand(0),
                Action::Assert {
                    assertion: Assertion::Controls {
                        player: P0,
                        card: TRAINER,
                        count: 1,
                    },
                },
            ]),
        // Offspring paid: two enters triggers, ordered by their controller; the 1/1 token copy
        // has the look of its own, but not offspring's.
        Scenario::new("Thundertrap Trainer with offspring")
            .hand(P0, &[TRAINER])
            .battlefield(P0, ISLANDS)
            .library(P0, LANDS)
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: TRAINER,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                Action::Resolve,
                Action::Answer {
                    player: P0,
                    kind: DecisionKind::TriggerOrder,
                    pick: Pick::Card { card: TRAINER },
                },
                Action::Assert {
                    assertion: Assertion::Stack { count: 2 },
                },
                Action::Resolve,
                Action::Resolve,
                // The token's own enters trigger, if offspring's resolved last; otherwise the look.
                Action::Resolve,
                Action::Assert {
                    assertion: Assertion::Stack { count: 0 },
                },
                Action::Assert {
                    assertion: Assertion::Pt {
                        player: P0,
                        card: TRAINER,
                        token: true,
                        power: 1,
                        toughness: 1,
                    },
                },
                Action::Assert {
                    assertion: Assertion::Pt {
                        player: P0,
                        card: TRAINER,
                        token: false,
                        power: 1,
                        toughness: 2,
                    },
                },
                library(8),
                hand(0),
                Action::AdvanceTo {
                    turn: 1,
                    step: TurnStep::End,
                },
            ]),
    ];
}
