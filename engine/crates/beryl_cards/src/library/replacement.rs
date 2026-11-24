//! Replacement effects ([CR 614](https://yawgatog.com/resources/magic-rules/#R614)).

use beryl_engine::{Zone, state::CounterKind};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};
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
