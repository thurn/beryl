//! Library ([CR 401](https://yawgatog.com/resources/magic-rules/#R401)).

use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};
use beryl_scenario_types::{ME, Resolve, THEM};

use crate::{names, props};

const ISLANDS: &[Permanent] = &[Permanent::new(names::ISLAND); 4];

/// A look at more cards than the library holds looks at them all
/// ([CR 609.3](https://yawgatog.com/resources/magic-rules/#R6093)): "If an effect attempts to
/// do something impossible, it does only as much as possible." Two of the three go to the
/// hand, and the one left goes to the bottom with nothing to arrange
/// ([CR 401.4](https://yawgatog.com/resources/magic-rules/#R4014)): "If an effect puts two
/// or more cards in a specific position in a library at the same time, the owner of those
/// cards may arrange them in any order."
pub const DIG: &[Scenario] =
    &[
        Scenario::new("Looking at the top five cards of a library of three looks at them all")
            .permanents(P0, ISLANDS)
            .hand(P0, &[names::STOCK_UP])
            .library(P0, &[names::PLAINS, props::ARTIFACT.card, names::ISLAND])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, names::STOCK_UP),
                Action::Resolve,
                Action::assert(Assertion::Decision {
                    player: P0,
                    kind: DecisionKind::ChooseObject,
                    candidates: 3,
                }),
                Action::choose(P0, DecisionKind::ChooseObject, props::ARTIFACT.card),
                Action::choose(P0, DecisionKind::ChooseObject, names::ISLAND),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Hand,
                    card: None,
                    count: 2,
                }),
                Action::assert(Assertion::Library {
                    player: P0,
                    top: &[names::PLAINS],
                }),
            ]),
    ];

const JACE: &str = names::JACE_THE_MIND_SCULPTOR;

/// Cards put on top of a library one at a time are drawn last first
/// ([CR 401.4](https://yawgatog.com/resources/magic-rules/#R4014)): "the owner of those cards
/// may arrange them in any order." The creature card, put back second, is the top card and
/// the next one drawn.
pub const PUT_BACK: &[Scenario] =
    &[
        Scenario::new("The card put back last is on top and is the next drawn")
            .permanents(P0, &[Permanent::new(JACE)])
            .library(
                P0,
                &[
                    names::ISLAND,
                    names::PLAINS,
                    props::FLIER.card,
                    names::ISLAND,
                    names::ISLAND,
                ],
            )
            .library(P1, &[names::PLAINS, names::PLAINS])
            .actions(&[
                Action::activate(P0, JACE, 1),
                Action::Resolve,
                Action::choose(P0, DecisionKind::ChooseObject, names::PLAINS),
                Action::choose(P0, DecisionKind::ChooseObject, props::FLIER.card),
                Action::assert(Assertion::Library {
                    player: P0,
                    top: &[
                        props::FLIER.card,
                        names::PLAINS,
                        names::ISLAND,
                        names::ISLAND,
                    ],
                }),
                Action::AdvanceTo {
                    turn: 3,
                    step: TurnStep::PrecombatMain,
                },
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Hand,
                    card: Some(props::FLIER.card),
                    count: 1,
                }),
            ]),
    ];

/// "Its owner puts it on their choice of the top or bottom of their library": the spell's
/// owner chooses, not the controller of the ability
/// ([CR 400.3](https://yawgatog.com/resources/magic-rules/#R4003)): "If an object would go to
/// any library, graveyard, or hand other than its owner's, it goes to its owner's
/// corresponding zone." A no is the bottom. The Plains in its owner's hand keeps their main
/// phase a decision once the stack is empty.
pub const TOP_OR_BOTTOM: &[Scenario] =
    &[
        Scenario::new("A spell's owner puts it on the bottom of their library")
            .at(2, P1, TurnStep::PrecombatMain)
            .permanents(P0, ISLANDS)
            .hand(P0, &[names::SUBTLETY])
            .permanents(
                P1,
                &[
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::PLAINS),
                ],
            )
            .hand(P1, &[props::FLIER.card, names::PLAINS])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P1, props::FLIER.card),
                Action::cast(P0, names::SUBTLETY),
                Action::Resolve,
                Action::answer(P0, DecisionKind::TargetCount, Pick::Number { n: 1 }),
                Action::Resolve,
                Action::answer(P1, DecisionKind::YesNo, Pick::No),
                Action::assert(Assertion::Library {
                    player: P1,
                    top: &[names::ISLAND, props::FLIER.card],
                }),
                Action::assert(Assertion::Stack { count: 0 }),
            ]),
    ];

/// A creature put second from the top moves there on resolution. The target remains
/// visible on the stack while the other player responds.
pub const SECOND_FROM_TOP: &[Scenario] = &[
    Scenario::new("Oust puts the creature second from the top and its controller gains 3")
        .battlefield(P0, &[names::PLAINS])
        .hand(P0, &[names::OUST, names::PLAINS])
        .battlefield(P1, &[props::FLIER.card])
        .library(P1, &[names::ISLAND, names::PLAINS])
        .actions(&[
            ME.casts(names::OUST),
            THEM.library_top(&[names::ISLAND, props::FLIER.card, names::PLAINS]),
            THEM.has(Zone::Battlefield, props::FLIER.card, 0),
            ME.has(Zone::Graveyard, names::OUST, 1),
            THEM.life(23),
        ]),
    Scenario::new("Oust beneath an opposing counterspell")
        .battlefield(P0, &[names::PLAINS, names::ISLAND, names::ISLAND])
        .hand(P0, &[names::OUST, props::INSTANT.card, names::PLAINS])
        .battlefield(P1, &[props::FLIER.card, names::ISLAND, names::ISLAND])
        .hand(P1, &[props::INSTANT.card])
        .actions(&[
            ME.casts(names::OUST),
            ME.passes(),
            THEM.casts(props::INSTANT.card),
            ME.passes(),
            THEM.controls(props::FLIER.card, 1),
        ]),
];

/// Putting selected cards back in order leaves the last selection on top.
pub const PUT_BACK_IN_ORDER: &[Scenario] =
    &[
        Scenario::new("0 draws three and puts two back in the order chosen")
            .battlefield(P0, &[JACE])
            .hand(P0, &[props::ARTIFACT.card])
            .library(
                P0,
                &[
                    names::ISLAND,
                    names::PLAINS,
                    props::FLIER.card,
                    names::ISLAND,
                ],
            )
            .library(P1, &[names::PLAINS])
            .actions(&[
                ME.activates(JACE, 1), // 0
                Resolve,
                ME.picks(props::ARTIFACT.card),
                ME.picks(names::ISLAND),
                ME.library_top(&[names::ISLAND, props::ARTIFACT.card, names::ISLAND]),
                ME.has_cards(Zone::Hand, 2),
                ME.has_loyalty(JACE, 3),
            ]),
    ];
