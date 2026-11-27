//! Replacement effects ([CR 614](https://yawgatog.com/resources/magic-rules/#R614)).

use beryl_engine::{DecisionKind, TurnStep, Zone, state::CounterKind};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};
use beryl_scenario_types::{ME, Player, Resolve, SnapshotRef, THEM};

use crate::names;

/// Effects that say a permanent "enters with" counters are replacement effects ([CR 614.1c]).
///
/// [CR 614.1c]: https://yawgatog.com/resources/magic-rules/#R6141c
pub const ENTERS_WITH_COUNTERS: &[Scenario] = &[Scenario::new("it enters with ten ice counters")
    .hand(P0, &[names::DARK_DEPTHS])
    .actions(&[
        ME.plays(names::DARK_DEPTHS),
        ME.counters(names::DARK_DEPTHS, CounterKind::Ice, 10),
    ])];

const JACE: &str = names::JACE_VRYNS_PRODIGY;
const TELEPATH: &str = "Jace, Telepath Unbound";
const IMPERSONATOR: &str = names::CLEVER_IMPERSONATOR;

const fn mana_value(player: Player, value: u8) -> Action {
    Action::assert(Assertion::ManaValue {
        object: SnapshotRef {
            zone: Zone::Battlefield,
            player: player.0,
            name: Some(TELEPATH),
            index: 0,
        },
        value,
    })
}

/// `XMage`'s [CleverImpersonatorTest.testCopyPlaneswalker] copies transformed Liliana
/// and checks loyalty. [TokenCopyTest.testCopyTransformedDFC] checks mana value for
/// token copies. Here the original is transformed Jace and the copy is a card.
///
/// [CleverImpersonatorTest.testCopyPlaneswalker]: https://github.com/magefree/mage/blob/987fd4e9c73e9f113e235f8d8f98628f2d391667/Mage.Tests/src/test/java/org/mage/test/cards/copy/CleverImpersonatorTest.java#L54-L83
/// [TokenCopyTest.testCopyTransformedDFC]: https://github.com/magefree/mage/blob/987fd4e9c73e9f113e235f8d8f98628f2d391667/Mage.Tests/src/test/java/org/mage/test/cards/copy/TokenCopyTest.java#L100-L115
pub const COPIES_TRANSFORMED_PERMANENT: &[Scenario] =
    &[Scenario::new("Clever Impersonator copies transformed Jace")
        .interaction(&[JACE])
        .battlefield(P0, &[names::ISLAND; 4])
        .permanents(P1, &[Permanent::new(JACE).transformed()])
        .hand(P0, &[IMPERSONATOR])
        .actions(&[
            ME.casts(IMPERSONATOR),
            Resolve,
            ME.picks_up_to(TELEPATH),
            THEM.controls(TELEPATH, 1),
            ME.controls(TELEPATH, 1),
            mana_value(THEM, 2),
            mana_value(ME, 0),
        ])];

const DOUBLING_SEASON: &str = names::DOUBLING_SEASON;
const HARDENED_SCALES: &str = names::HARDENED_SCALES;
const MIND_SCULPTOR: &str = crate::props::PLANESWALKER.card;
const EMPEROR: &str = names::THE_WANDERING_EMPEROR;
const SAGA: &str = names::THE_ELDEST_REBORN;
const STUDENT: &str = names::STUDENT_OF_WARFARE;
const SHIMMERER: &str = names::ADAPTIVE_SHIMMERER;
const VANILLA: &str = crate::props::VANILLA.card;

/// A replacement of the counters an effect puts on a permanent ("If an effect would put one or
/// more counters on a permanent you control"). A planeswalker's printed loyalty and the
/// counters a permanent enters with (a Saga's first lore counter among them) are put by an
/// effect, so they are doubled; a loyalty ability's cost and counters on an opponent's
/// permanent are not.
pub const DOUBLED_COUNTERS: &[Scenario] = &[
    Scenario::new("Doubling Season doubles a planeswalker's loyalty but not its loyalty cost")
        .battlefield(
            P0,
            &[
                DOUBLING_SEASON,
                names::ISLAND,
                names::ISLAND,
                names::ISLAND,
                names::ISLAND,
            ],
        )
        .hand(P0, &[MIND_SCULPTOR])
        .library(P0, &[names::ISLAND])
        .library(P1, &[names::ISLAND, names::PLAINS])
        .actions(&[
            ME.casts(MIND_SCULPTOR),
            Resolve,
            ME.has_loyalty(MIND_SCULPTOR, 6),
            ME.activates(MIND_SCULPTOR, 0),
            Action::answer(P0, DecisionKind::Target, Pick::Player { player: P1 }),
            ME.has_loyalty(MIND_SCULPTOR, 8),
            Resolve,
            Action::answer(P0, DecisionKind::YesNo, Pick::No),
            ME.has_loyalty(MIND_SCULPTOR, 8),
        ]),
    Scenario::new("Doubling Season doubles the counters a permanent enters with")
        .interaction(&[names::DARK_DEPTHS])
        .battlefield(P0, &[DOUBLING_SEASON])
        .hand(P0, &[names::DARK_DEPTHS])
        .actions(&[
            ME.plays(names::DARK_DEPTHS),
            ME.counters(names::DARK_DEPTHS, CounterKind::Ice, 20),
        ]),
    Scenario::new("Doubling Season doubles counters an opponent's effect puts on your creature")
        .interaction(&[EMPEROR])
        .battlefield(P0, &[DOUBLING_SEASON, VANILLA])
        .battlefield(P1, &[EMPEROR])
        .library(P0, &[names::PLAINS, names::PLAINS])
        .library(P1, &[names::PLAINS, names::PLAINS])
        .at(2, P1, TurnStep::PrecombatMain)
        .actions(&[
            THEM.activates(EMPEROR, 1),
            THEM.target_count(1),
            Resolve,
            ME.counters(VANILLA, CounterKind::PlusOne, 2),
        ]),
    Scenario::new("Doubling Season does not double counters put on an opponent's creature")
        .interaction(&[EMPEROR])
        .battlefield(
            P0,
            &[
                DOUBLING_SEASON,
                names::PLAINS,
                names::PLAINS,
                names::PLAINS,
                names::PLAINS,
            ],
        )
        .hand(P0, &[EMPEROR])
        .battlefield(P1, &[VANILLA])
        .library(P0, &[names::PLAINS, names::PLAINS])
        .library(P1, &[names::PLAINS, names::PLAINS])
        .actions(&[
            ME.casts(EMPEROR),
            Resolve,
            ME.has_loyalty(EMPEROR, 6),
            ME.activates(EMPEROR, 1),
            ME.target_count(1),
            Resolve,
            THEM.counters(VANILLA, CounterKind::PlusOne, 1),
            ME.has_loyalty(EMPEROR, 7),
        ]),
    // A Saga's first lore counter comes from an intrinsic replacement effect, as a
    // planeswalker's loyalty does (CR 715.3a, CR 306.5b), so it is doubled and chapters I and II
    // both trigger.
    Scenario::new("Doubling Season doubles a Saga's first lore counter")
        .interaction(&[SAGA])
        .battlefield(
            P0,
            &[
                DOUBLING_SEASON,
                names::SWAMP,
                names::SWAMP,
                names::SWAMP,
                names::SWAMP,
                names::SWAMP,
            ],
        )
        .hand(P0, &[SAGA])
        .library(P0, &[names::SWAMP; 3])
        .library(P1, &[names::ISLAND; 3])
        .hand(P1, &[names::PLAINS])
        .actions(&[
            ME.casts(SAGA),
            Resolve,
            ME.chooses(DecisionKind::TriggerOrder, SAGA),
            ME.counters(SAGA, CounterKind::Lore, 2),
            Resolve,
            Resolve,
            THEM.has(Zone::Graveyard, names::PLAINS, 1),
        ]),
];

/// A replacement that adds to the +1/+1 counters put on a creature, whatever puts them.
pub const PLUS_ONE_COUNTER: &[Scenario] = &[
    Scenario::new("Hardened Scales makes one +1/+1 counter two")
        .interaction(&[EMPEROR])
        .battlefield(P0, &[HARDENED_SCALES, EMPEROR, VANILLA])
        .library(P0, &[names::PLAINS, names::PLAINS])
        .library(P1, &[names::PLAINS, names::PLAINS])
        .actions(&[
            ME.activates(EMPEROR, 1),
            ME.target_count(1),
            Resolve,
            ME.counters(VANILLA, CounterKind::PlusOne, 2),
        ]),
    Scenario::new("Hardened Scales leaves counters of another kind unchanged")
        .interaction(&[STUDENT])
        .battlefield(P0, &[HARDENED_SCALES, STUDENT, names::PLAINS])
        .actions(&[
            ME.activates(STUDENT, 0),
            Resolve,
            ME.counters(STUDENT, CounterKind::Level, 1),
            ME.counters(STUDENT, CounterKind::PlusOne, 0),
        ]),
    Scenario::new("Hardened Scales adds one to the +1/+1 counters a creature enters with")
        .interaction(&[SHIMMERER])
        .battlefield(
            P0,
            &[
                HARDENED_SCALES,
                names::FOREST,
                names::FOREST,
                names::FOREST,
                names::FOREST,
                names::FOREST,
            ],
        )
        .hand(P0, &[SHIMMERER])
        .actions(&[
            ME.casts(SHIMMERER),
            Resolve,
            ME.counters(SHIMMERER, CounterKind::PlusOne, 4),
            ME.pt(SHIMMERER, 4, 4),
        ]),
];
