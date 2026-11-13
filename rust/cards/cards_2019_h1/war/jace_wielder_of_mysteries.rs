// Generated from the card store by `just card header`: notes go below the end marker.
//! Jace, Wielder of Mysteries {1}{U}{U}{U}
//! Legendary Planeswalker — Jace, loyalty 4
//!
//! If you would draw a card while your library has no cards in it, you win the game instead.
//! +1: Target player mills two cards. Draw a card.
//! −8: Draw seven cards. Then if your library has no cards in it, you win the game.
// End of the generated card header.

use crate::{names, printed};
use beryl_card::effect::build::{TARGET_PLAYER, YOU, draw, if_then, mill, sequence, win_game};
use beryl_card::{
    Ability, Amount, CardDef, Comparison, Condition, Effect, Filter, Objects, Players, Replacement,
    TargetKind, TargetSpec, Zone, card,
};

/// "While your library has no cards in it", "if your library has no cards in it".
const LIBRARY_IS_EMPTY: Condition = Condition::Compare(
    Amount::Count(&Objects::All {
        zone: Zone::Library,
        filter: Filter::Whose(&Players::You),
    }),
    Comparison::Equal,
    Amount::Fixed(0),
);

/// "You win the game".
static YOU_WIN: Effect = win_game(YOU);

pub static CARD: CardDef = card(
    names::JACE_WIELDER_OF_MYSTERIES,
    &[printed::JACE_WIELDER_OF_MYSTERIES.abilities(&[
        // A replacement effect ([CR 614.1a](https://yawgatog.com/resources/magic-rules/#R6141a)):
        // "Effects that use the word 'instead' are replacement effects." The draw
        // machinery offers it each draw, an empty library or not (CR 614.11).
        Ability::replacing(Replacement::WouldDraw)
            .only_if(&LIBRARY_IS_EMPTY)
            .effect(&YOU_WIN),
        Ability::loyalty(1)
            .targeting(&[TargetSpec::one(TargetKind::Player, Filter::Any)])
            .effect(&MILL_TWO_AND_DRAW),
        Ability::loyalty(-8).effect(&DRAW_SEVEN),
    ])],
);

/// "+1: Target player mills two cards. Draw a card." In order
/// ([CR 608.2c](https://yawgatog.com/resources/magic-rules/#R6082c)), so milling himself
/// empty, Jace draws from an empty library and his controller wins instead.
/// [CR 701.17b](https://yawgatog.com/resources/magic-rules/#R70117b): "A player can't mill a
/// number of cards greater than the number of cards in their library."
static MILL_TWO_AND_DRAW: Effect = sequence(&[mill(TARGET_PLAYER, 2), draw(YOU, 1)]);

/// "−8: Draw seven cards. Then if your library has no cards in it, you win the game." Jace
/// with exactly eight loyalty is gone by then (CR 704.5i), and his controller draws what is
/// left and wins on the second sentence; the attempt to draw from the empty library loses
/// only when state-based actions are next checked
/// ([CR 704.5b](https://yawgatog.com/resources/magic-rules/#R7045b)), by when the game is
/// over ([CR 104.2b](https://yawgatog.com/resources/magic-rules/#R1042b)): "An effect may
/// state that a player wins the game."
static DRAW_SEVEN: Effect = sequence(&[draw(YOU, 7), if_then(LIBRARY_IS_EMPTY, &YOU_WIN)]);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_card::CounterKind;
    use beryl_card::{DecisionKind, Seat, TurnStep, Zone};

    use beryl_scenario_types::{Action, Assertion, Ending, P0, P1, Permanent, Pick, Scenario};

    use crate::{names, props};

    const JACE: &str = names::JACE_WIELDER_OF_MYSTERIES;

    const ISLAND: &str = names::ISLAND;

    const PLAINS: &str = names::PLAINS;

    /// The Islands to flash in the small creature with.
    const ISLANDS: &[Permanent] = &[Permanent::new(ISLAND), Permanent::new(ISLAND)];

    const ISLANDS_AND_JACE: &[Permanent] = &[
        Permanent::new(JACE),
        Permanent::new(ISLAND),
        Permanent::new(ISLAND),
    ];

    const fn outcome(outcome: Ending) -> Action {
        Action::assert(Assertion::Outcome { outcome })
    }

    const fn count(player: Seat, zone: Zone, count: u8) -> Action {
        Action::assert(Assertion::Count {
            player,
            zone,
            card: None,
            count,
        })
    }

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("+1 mills the target player two, then draws")
            .permanents(P0, &[Permanent::new(JACE)])
            .hand(P0, &[PLAINS])
            .library(P0, &[ISLAND, ISLAND])
            .library(P1, &[PLAINS, PLAINS, ISLAND])
            .actions(&[
                Action::activate(P0, JACE, 1),
                Action::answer(P0, DecisionKind::Target, Pick::Player { player: P1 }),
                Action::Resolve,
                count(P1, Zone::Graveyard, 2),
                Action::assert(Assertion::Library {
                    player: P1,
                    top: &[ISLAND],
                }),
                count(P0, Zone::Hand, 2),
                Action::assert(Assertion::Counters {
                    player: P0,
                    card: JACE,
                    kind: CounterKind::Loyalty,
                    count: 5,
                }),
            ]),
        // Milling his own last two cards, Jace draws from the empty library: a win, not a loss.
        Scenario::new("+1 milling his controller empty wins on the draw")
            .permanents(P0, &[Permanent::new(JACE)])
            .library(P0, &[ISLAND, PLAINS])
            .library(P1, &[ISLAND])
            .actions(&[
                Action::activate(P0, JACE, 1),
                // P0 has nothing else to do, so the ability resolves at once.
                Action::answer(P0, DecisionKind::Target, Pick::Player { player: P0 }),
                count(P0, Zone::Graveyard, 2),
                outcome(Ending::Win { player: P0 }),
            ]),
        // A creature with flash gives P0 something to pass in the upkeep.
        Scenario::new("the draw step's draw from an empty library wins")
            .at(3, P0, TurnStep::Upkeep)
            .permanents(P0, ISLANDS_AND_JACE)
            .hand(P0, &[props::SMALL_CREATURE])
            .library(P0, &[])
            .library(P1, &[ISLAND])
            .actions(&[Action::pass(P0), outcome(Ending::Win { player: P0 })]),
        // The replacement is Jace's controller's alone: the opponent drawing from an empty
        // library still loses (CR 704.5b).
        Scenario::new("the opponent's draw from an empty library still loses")
            .at(2, P1, TurnStep::Upkeep)
            .permanents(P0, &[Permanent::new(JACE)])
            .library(P0, &[ISLAND])
            .permanents(P1, ISLANDS)
            .hand(P1, &[props::SMALL_CREATURE])
            .library(P1, &[])
            .actions(&[Action::pass(P1), outcome(Ending::Win { player: P0 })]),
        // At exactly eight loyalty Jace dies before the ability resolves: no replacement. Five
        // cards are drawn, two draws fail, and the library is empty: a win.
        Scenario::new("−8 draws the library empty and wins")
            .permanents(
                P0,
                &[Permanent::new(JACE).counters(CounterKind::Loyalty, 8)],
            )
            .library(P0, &[ISLAND, PLAINS, ISLAND, PLAINS, ISLAND])
            .library(P1, &[ISLAND])
            .actions(&[
                Action::activate(P0, JACE, 2),
                outcome(Ending::Win { player: P0 }),
                count(P0, Zone::Hand, 5),
            ]),
        // With cards left after the seven, there is no win.
        Scenario::new("−8 with cards left after the seven doesn't win")
            .permanents(
                P0,
                &[Permanent::new(JACE).counters(CounterKind::Loyalty, 9)],
            )
            .library(
                P0,
                &[
                    ISLAND, PLAINS, ISLAND, PLAINS, ISLAND, PLAINS, ISLAND, PLAINS,
                ],
            )
            .library(P1, &[ISLAND])
            .actions(&[
                Action::activate(P0, JACE, 2),
                Action::Resolve,
                count(P0, Zone::Hand, 7),
                count(P0, Zone::Library, 1),
                Action::assert(Assertion::Counters {
                    player: P0,
                    card: JACE,
                    kind: CounterKind::Loyalty,
                    count: 1,
                }),
            ]),
    ];
}
