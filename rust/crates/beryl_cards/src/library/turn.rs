//! Turn structure ([CR 500](https://yawgatog.com/resources/magic-rules/#R500)).

use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{P0, P1, Scenario};
use beryl_scenario_types::{ME, Resolve, THEM, advance_to};

use crate::{names, props};

/// "The affected player is controlled during the entire turn" ([CR 724.1]).
///
/// [CR 724.1]: https://yawgatog.com/resources/magic-rules/#R7241
pub const CONTROLLED_TURN: &[Scenario] = &[Scenario::new("the control ends with the turn")
    .at(1, P0, TurnStep::PostcombatMain)
    .battlefield(
        P0,
        &[
            names::MINDSLAVER,
            names::ISLAND,
            names::ISLAND,
            names::ISLAND,
            names::ISLAND,
            props::VANILLA.card,
        ],
    )
    .battlefield(P1, &[names::PLAINS, props::FLIER.card])
    .hand(P1, &[props::SORCERY.card])
    .library(P0, &[names::ISLAND, names::ISLAND])
    .library(P1, &[names::ISLAND, names::ISLAND])
    .actions(&[
        ME.activates(names::MINDSLAVER, 0),
        ME.targets_player(THEM),
        Resolve,
        advance_to(2, TurnStep::PrecombatMain),
        // Passing, playing P1's Island, or casting P1's Oust.
        ME.deciding(DecisionKind::Priority, 3),
        advance_to(2, TurnStep::BeginningOfCombat),
        ME.passes(),
        ME.done_attacking(),
        // In their own turn P0 decides for themself, as ever.
        advance_to(3, TurnStep::BeginningOfCombat),
        ME.passes(),
        ME.done_attacking(),
        advance_to(4, TurnStep::PrecombatMain),
        THEM.casts(props::SORCERY.card),
        THEM.targets(props::VANILLA.card),
        Resolve,
        ME.has(Zone::Library, props::VANILLA.card, 1),
        ME.life(23),
    ])];

/// The starting player skips their first draw, so with equally sized libraries the player
/// on the draw runs out first.
pub const FIRST_DRAW: &[Scenario] = &[Scenario::new("the player on the draw decks first")
    .at(1, P0, TurnStep::Upkeep)
    .hand(P0, &[names::ISLAND])
    .hand(P1, &[names::PLAINS])
    .library(P0, &[names::ISLAND])
    .library(P1, &[names::PLAINS])
    .actions(&[
        advance_to(1, TurnStep::PrecombatMain),
        ME.has_cards(Zone::Hand, 1),
        advance_to(2, TurnStep::PrecombatMain),
        THEM.has_cards(Zone::Hand, 2),
        advance_to(3, TurnStep::PrecombatMain),
        ME.has_cards(Zone::Hand, 2),
        advance_to(3, TurnStep::End),
        ME.passes(),
        beryl_scenario_types::wins(ME),
    ])];

/// Only the active player can play a land, only in their main phase with an empty stack,
/// and by default only once per turn.
pub const LAND_PLAY: &[Scenario] = &[Scenario::new("players get one land play on their own turn")
    .interaction(&[names::LIGHTNING_BOLT])
    .hand(P0, &[names::ISLAND, names::PLAINS])
    .hand(P1, &[names::ISLAND, names::PLAINS, names::LIGHTNING_BOLT])
    .battlefield(P1, &[names::MOUNTAIN])
    .library(P1, &[names::ISLAND])
    .actions(&[
        ME.deciding(DecisionKind::Priority, 3),
        ME.plays(names::ISLAND),
        advance_to(1, TurnStep::PostcombatMain),
        ME.deciding(DecisionKind::Priority, 1),
        ME.has(Zone::Hand, names::PLAINS, 1),
        ME.passes(),
        // The opponent can cast Lightning Bolt, which exposes their priority during this
        // main phase. Their lands are unavailable.
        THEM.deciding(DecisionKind::Priority, 2),
        THEM.passes(),
        advance_to(2, TurnStep::Upkeep),
        THEM.deciding(DecisionKind::Priority, 2),
        advance_to(2, TurnStep::PrecombatMain),
        THEM.plays(names::PLAINS),
        advance_to(2, TurnStep::PostcombatMain),
        THEM.deciding(DecisionKind::Priority, 2),
        ME.controls(names::ISLAND, 1),
        THEM.controls(names::PLAINS, 1),
    ])];

/// In cleanup the active player chooses cards to discard until their hand has seven.
pub const CLEANUP: &[Scenario] = &[
    Scenario::new("only the active player discards down to seven")
        .at(1, P0, TurnStep::End)
        .hand(P0, &[names::ISLAND; 9])
        .hand(P1, &[names::PLAINS; 8])
        .library(P1, &[names::PLAINS])
        .actions(&[
            advance_to(2, TurnStep::Upkeep),
            ME.has_cards(Zone::Hand, 7),
            ME.has_cards(Zone::Graveyard, 2),
            THEM.has_cards(Zone::Hand, 8),
            THEM.has_cards(Zone::Graveyard, 0),
        ]),
];
