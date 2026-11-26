//! Shared-deck format (`Format::SHARED_DECK`): one shared library and one shared graveyard.

use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{Action, Assertion, Ending, P0, P1, Pick, Scenario};
use beryl_scenario_types::{ME, Resolve, THEM, advance_to};

use crate::{names, props};

/// 1. A draw: the active player draws the top card of the shared library and owns it.
pub const DRAW: &[Scenario] = &[
    Scenario::new("drawing takes the top card of the shared library")
        .at(2, P1, TurnStep::Upkeep)
        .shared_library(&[names::PLAINS, names::ISLAND])
        .actions(&[
            advance_to(2, TurnStep::PrecombatMain),
            THEM.has(Zone::Hand, names::PLAINS, 1),
            THEM.has_cards(Zone::Library, 1),
            ME.has_cards(Zone::Library, 1),
        ]),
];

/// 2. A mill: milling puts cards from the top of the shared library into the shared graveyard.
pub const MILL: &[Scenario] = &[
    Scenario::new("milling puts cards into the shared graveyard")
        .battlefield(P0, &[names::ISLAND])
        .hand(P0, &[names::MENTAL_NOTE])
        .shared_library(&[names::PLAINS, names::MOUNTAIN, names::ISLAND])
        .actions(&[
            ME.casts(names::MENTAL_NOTE),
            ME.has(Zone::Hand, names::ISLAND, 1),
            ME.has(Zone::Graveyard, names::MENTAL_NOTE, 1),
            ME.has(Zone::Graveyard, names::PLAINS, 1),
            ME.has(Zone::Graveyard, names::MOUNTAIN, 1),
            THEM.has_cards(Zone::Graveyard, 3),
            ME.has_cards(Zone::Library, 0),
        ]),
];

/// 3. A card returned to the top of the shared library that the other player draws and owns.
pub const RETURN_TO_TOP: &[Scenario] = &[Scenario::new(
    "a permanent put on top of the shared library is drawn and owned by the other player",
)
.battlefield(P0, &[names::PLAINS, props::VANILLA.card])
.battlefield(P1, &[names::ISLAND, names::ISLAND])
.hand(P0, &[names::OUST])
.hand(P1, &[names::ISLAND])
.shared_library(&[])
.actions(&[
    ME.casts(names::OUST),
    advance_to(2, TurnStep::PrecombatMain),
    THEM.has(Zone::Hand, props::VANILLA.card, 1),
    THEM.casts(props::VANILLA.card),
    THEM.controls(props::VANILLA.card, 1),
])];

/// 4. A shuffle: both hands and the shared graveyard are shuffled into the shared library once.
pub const SHUFFLE: &[Scenario] =
    &[
        Scenario::new("shuffling both hands and the shared graveyard into the shared library")
            .seed(0)
            .battlefield(P0, &[names::ISLAND; 3])
            .hand(P0, &[names::TIMETWISTER, names::PLAINS])
            .hand(P1, &[names::MOUNTAIN])
            .shared_graveyard(&[props::SORCERY.card])
            .shared_library(&[names::ISLAND; 12])
            .actions(&[
                ME.casts(names::TIMETWISTER),
                ME.has_cards(Zone::Hand, 7),
                THEM.has_cards(Zone::Hand, 7),
                ME.has(Zone::Graveyard, names::TIMETWISTER, 1),
                ME.has_cards(Zone::Graveyard, 1),
                ME.has_cards(Zone::Library, 1),
            ]),
    ];

/// 5. "Each player draws" alternates one at a time starting with the active player.
pub const SIMULTANEOUS_DRAW: &[Scenario] =
    &[
        Scenario::new("simultaneous draws alternate from the active player")
            .seed(0)
            .battlefield(P0, &[names::ISLAND; 3])
            .hand(P0, &[names::TIMETWISTER])
            .shared_library(&[names::ISLAND; 13])
            .actions(&[
                ME.casts(names::TIMETWISTER),
                Action::assert(Assertion::Outcome {
                    outcome: Ending::Win { player: P0 },
                }),
            ]),
    ];

/// 6. The free mulligan: a hand with fewer than two lands or spells takes a free mulligan
///    without reducing hand size or incrementing mulligans taken.
pub const FREE_MULLIGAN: &[Scenario] =
    &[
        Scenario::new("a hand with fewer than two nonland cards takes a free mulligan")
            .pregame(P0)
            .shared_library(&[
                // P0 gets 0, 2, 4, 6, 8, 10, 12 (7 Islands -> qualifies for free mulligan).
                // P1 gets 1, 3, 5, 7, 9, 11, 13 (3 Islands, 4 Vanilla -> keeps).
                names::ISLAND,
                names::ISLAND,
                names::ISLAND,
                props::VANILLA.card,
                names::ISLAND,
                names::ISLAND,
                names::ISLAND,
                props::VANILLA.card,
                names::ISLAND,
                names::ISLAND,
                names::ISLAND,
                props::VANILLA.card,
                names::ISLAND,
                props::VANILLA.card,
                // Next 7 cards for P0's free mulligan redraw (3 Islands, 4 Vanilla -> keeps).
                names::ISLAND,
                names::ISLAND,
                names::ISLAND,
                props::VANILLA.card,
                props::VANILLA.card,
                props::VANILLA.card,
                props::VANILLA.card,
            ])
            .actions(&[
                Action::answer(P0, DecisionKind::KeepOrMulligan, Pick::FreeMulligan),
                Action::answer(P1, DecisionKind::KeepOrMulligan, Pick::Keep),
                Action::answer(P0, DecisionKind::KeepOrMulligan, Pick::Keep),
                ME.has_cards(Zone::Hand, 7),
                THEM.has_cards(Zone::Hand, 7),
                ME.has_cards(Zone::Library, 7),
            ]),
    ];

/// 7. Decking: the player who must draw from the empty shared library loses.
pub const DECKING: &[Scenario] =
    &[
        Scenario::new("the player who draws from an empty shared library loses")
            .at(1, P0, TurnStep::Upkeep)
            .shared_library(&[names::ISLAND])
            .actions(&[
                advance_to(1, TurnStep::PrecombatMain),
                ME.has_cards(Zone::Library, 1),
                advance_to(2, TurnStep::End),
                THEM.has(Zone::Hand, names::ISLAND, 1),
                THEM.passes(),
                Action::assert(Assertion::Outcome {
                    outcome: Ending::Win { player: P1 },
                }),
            ]),
    ];

/// 8. A card with flashback cast from the shared graveyard by the player who did not put it there.
pub const FLASHBACK: &[Scenario] = &[Scenario::new(
    "a player casts a card with flashback from the shared graveyard put there by the opponent",
)
.interaction(&[names::LIGHTNING_BOLT])
.battlefield(P0, &[names::MOUNTAIN])
.hand(P0, &[names::LIGHTNING_BOLT])
.battlefield(
    P1,
    &[
        names::ISLAND,
        names::ISLAND,
        names::MOUNTAIN,
        names::MOUNTAIN,
    ],
)
.hand(P1, &[names::SNAPCASTER_MAGE])
.shared_library(&[names::ISLAND, names::ISLAND])
.actions(&[
    ME.casts(names::LIGHTNING_BOLT),
    ME.targets_player(THEM),
    Resolve,
    THEM.has(Zone::Graveyard, names::LIGHTNING_BOLT, 1),
    advance_to(2, TurnStep::PrecombatMain),
    THEM.casts(names::SNAPCASTER_MAGE),
    Resolve,
    Resolve,
    THEM.casts(names::LIGHTNING_BOLT),
    THEM.targets_player(ME),
    ME.life(17),
    THEM.has(Zone::Exile, names::LIGHTNING_BOLT, 1),
    THEM.has(Zone::Graveyard, names::LIGHTNING_BOLT, 0),
])];

/// 9. Playing an Island in a shared game and tapping it for blue mana.
pub const ISLAND: &[Scenario] =
    &[
        Scenario::new("Island played in a shared game taps for blue mana")
            .shared_library(&[names::ISLAND; 3])
            .interaction(&[names::MENTAL_NOTE])
            .hand(P0, &[names::ISLAND, names::MENTAL_NOTE])
            .actions(&[
                ME.plays(names::ISLAND),
                ME.controls(names::ISLAND, 1),
                ME.casts(names::MENTAL_NOTE),
                Resolve,
                ME.tapped(names::ISLAND, 1),
            ]),
    ];
