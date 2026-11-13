// Generated from the card store by `just card header`: notes go below the end marker.
//! Jace, the Mind Sculptor {2}{U}{U}
//! Legendary Planeswalker — Jace, loyalty 3
//!
//! +2: Look at the top card of target player's library. You may put that card on the bottom of that
//! player's library.
//! 0: Draw three cards, then put two cards from your hand on top of your library in any order.
//! −1: Return target creature to its owner's hand.
//! −12: Exile all cards from target player's library, then that player shuffles their hand into
//! their library.
// End of the generated card header.

use crate::{names, printed};
use beryl_card::effect::build::{
    TARGET, TARGET_PLAYER, YOU, draw, exile, fateseal, put_back, return_to_hand, sequence,
    shuffle_into,
};
use beryl_card::{
    Ability, CardDef, Effect, Filter, Objects, TargetKind, TargetSpec, Types, Zone, card,
};

const A_PLAYER: &[TargetSpec] = &[TargetSpec::one(TargetKind::Player, Filter::Any)];

pub static CARD: CardDef = card(
    names::JACE_THE_MIND_SCULPTOR,
    &[printed::JACE_THE_MIND_SCULPTOR.abilities(&[
        Ability::loyalty(2).targeting(A_PLAYER).effect(&FATESEAL),
        Ability::loyalty(0).effect(&BRAINSTORM),
        Ability::loyalty(-1)
            .targeting(&[TargetSpec::one(
                TargetKind::Permanent,
                Filter::Types(Types::CREATURE),
            )])
            .effect(&BOUNCE),
        Ability::loyalty(-12)
            .targeting(A_PLAYER)
            .effect(&EXILE_LIBRARY),
    ])],
);

/// "+2: Look at the top card of target player's library. You may put that card on the
/// bottom of that player's library." Looking shows the card to Jace's controller alone, an
/// exception to [CR 401.2](https://yawgatog.com/resources/magic-rules/#R4012): "Players can't
/// look at or change the order of cards in a library." Moving it within the library is no
/// zone change, so it stays the same object.
static FATESEAL: Effect = fateseal(YOU, TARGET_PLAYER);

/// "0: Draw three cards, then put two cards from your hand on top of your library in any
/// order." Each card chosen goes on top of the one before, so the second is the top card.
/// [CR 121.2](https://yawgatog.com/resources/magic-rules/#R1212): "If a player is instructed to
/// draw multiple cards, that player performs that many individual card draws."
static BRAINSTORM: Effect = sequence(&[draw(YOU, 3), put_back(YOU, 2)]);

/// "−1: Return target creature to its owner's hand."
/// [CR 400.3](https://yawgatog.com/resources/magic-rules/#R4003): "If an object would go to
/// any library, graveyard, or hand other than its owner's, it goes to its owner's
/// corresponding zone."
static BOUNCE: Effect = return_to_hand(TARGET);

/// "−12: Exile all cards from target player's library, then that player shuffles their hand
/// into their library." [CR 701.13a](https://yawgatog.com/resources/magic-rules/#R70113a):
/// "To exile an object, move it to the exile zone from wherever it is." Then
/// [CR 701.24a](https://yawgatog.com/resources/magic-rules/#R70124a): "To shuffle a library
/// ... randomize the cards within it so that no player knows their order."
static EXILE_LIBRARY: Effect = sequence(&[
    exile(Objects::InZoneOf {
        zone: Zone::Library,
        players: &TARGET_PLAYER,
        filter: Filter::Any,
    }),
    shuffle_into(
        TARGET_PLAYER,
        Objects::InZoneOf {
            zone: Zone::Hand,
            players: &TARGET_PLAYER,
            filter: Filter::Any,
        },
    ),
]);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_card::CounterKind;
    use beryl_card::{DecisionKind, TurnStep, Zone};

    use beryl_scenario_types::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

    use crate::{names, props};

    const JACE: &str = names::JACE_THE_MIND_SCULPTOR;

    const ISLAND: &str = names::ISLAND;

    const PLAINS: &str = names::PLAINS;

    const ARTIFACT: &str = props::ARTIFACT;

    const CREATURE: &str = props::CREATURE;

    const fn loyalty(count: u8) -> Action {
        Action::assert(Assertion::Counters {
            player: P0,
            card: JACE,
            kind: CounterKind::Loyalty,
            count,
        })
    }

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("+2 puts the top of an opponent's library on the bottom")
            .permanents(P0, &[Permanent::new(JACE)])
            .library(P0, &[ISLAND, PLAINS])
            .library(P1, &[ARTIFACT, PLAINS, ISLAND])
            .actions(&[
                Action::activate(P0, JACE, 0),
                Action::answer(P0, DecisionKind::Target, Pick::Player { player: P1 }),
                Action::Resolve,
                Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
                // Assertions read the next open decision: by then, the opponent has drawn the
                // Plains that the artifact left on top.
                Action::assert(Assertion::Library {
                    player: P1,
                    top: &[ISLAND, ARTIFACT],
                }),
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Hand,
                    card: Some(PLAINS),
                    count: 1,
                }),
                loyalty(5),
                // The next turn, Jace looks at his own top card and keeps it there.
                Action::AdvanceTo {
                    turn: 3,
                    step: TurnStep::PrecombatMain,
                },
                Action::activate(P0, JACE, 0),
                Action::answer(P0, DecisionKind::Target, Pick::Player { player: P0 }),
                Action::answer(P0, DecisionKind::YesNo, Pick::No),
                Action::assert(Assertion::Library {
                    player: P0,
                    top: &[PLAINS],
                }),
                loyalty(7),
            ]),
        Scenario::new("0 draws three and puts two back in the order chosen")
            .permanents(P0, &[Permanent::new(JACE)])
            .hand(P0, &[ARTIFACT])
            .library(P0, &[ISLAND, PLAINS, CREATURE, ISLAND])
            .library(P1, &[PLAINS])
            .actions(&[
                Action::activate(P0, JACE, 1),
                Action::Resolve,
                Action::choose(P0, DecisionKind::ChooseObject, ARTIFACT),
                Action::choose(P0, DecisionKind::ChooseObject, ISLAND),
                Action::assert(Assertion::Library {
                    player: P0,
                    top: &[ISLAND, ARTIFACT, ISLAND],
                }),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Hand,
                    card: None,
                    count: 2,
                }),
                loyalty(3),
            ]),
        Scenario::new("−1 returns a creature; −12 exiles a library and shuffles in the hand")
            .permanents(
                P0,
                &[Permanent::new(JACE).counters(CounterKind::Loyalty, 13)],
            )
            .permanents(P1, &[Permanent::new(CREATURE)])
            .hand(P1, &[PLAINS])
            .library(P0, &[ISLAND, ISLAND])
            .library(P1, &[ISLAND, ISLAND, ISLAND])
            .actions(&[
                Action::activate(P0, JACE, 2),
                Action::Resolve,
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Hand,
                    card: Some(CREATURE),
                    count: 1,
                }),
                loyalty(12),
                Action::AdvanceTo {
                    turn: 3,
                    step: TurnStep::PrecombatMain,
                },
                Action::activate(P0, JACE, 3),
                Action::answer(P0, DecisionKind::Target, Pick::Player { player: P1 }),
                // Turn 2's draw left two Islands, now exiled; the hand of three is the library.
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Exile,
                    card: Some(ISLAND),
                    count: 2,
                }),
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Library,
                    card: None,
                    count: 3,
                }),
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Hand,
                    card: None,
                    count: 0,
                }),
                // Loyalty 0: put into its owner's graveyard (CR 704.5i).
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Graveyard,
                    card: Some(JACE),
                    count: 1,
                }),
            ]),
    ];
}
