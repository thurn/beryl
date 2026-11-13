//! An Equipment comes off a permanent that stops being a creature
//! ([CR 704.5n](https://yawgatog.com/resources/magic-rules/#R7045n)): "If an Equipment or
//! Fortification is attached to an illegal permanent or to a player, it becomes unattached from
//! that permanent or player. It remains on the battlefield."
//!
//! No scenario assertion says what a permanent is attached to, so this test watches the state.

use beryl_card::{Castings, FaceIx};
use beryl_scenario::{Action, P0, Permanent, Scenario};

use crate::{CARDS, names, props};

/// P0 equips Leonin Scimitar to Elite Spellbinder, then makes the Spellbinder a noncreature
/// Vehicle artifact with Swift Reconfiguration.
const RECONFIGURED: Scenario =
    Scenario::new("An Equipment falls off a creature that stops being one")
        .interaction(&[names::SWIFT_RECONFIGURATION])
        .hand(P0, &[names::SWIFT_RECONFIGURATION])
        .battlefield(
            P0,
            &[
                Permanent::new(props::EQUIPMENT),
                Permanent::new(props::CREATURE),
                Permanent::new(names::PLAINS),
                Permanent::new(names::PLAINS),
            ],
        )
        .library(P0, &[names::PLAINS])
        .library(beryl_scenario::P1, &[names::PLAINS])
        .actions(&[
            Action::activate(P0, props::EQUIPMENT, 1),
            Action::Resolve,
            Action::Cast {
                player: P0,
                card: names::SWIFT_RECONFIGURATION,
                face: FaceIx::FRONT,
                costs: Castings::NONE,
            },
            Action::Resolve,
        ]);

#[test]
fn an_equipment_on_a_permanent_that_stops_being_a_creature_comes_off() {
    let (t, _) = beryl_scenario::position(&CARDS, &RECONFIGURED).unwrap_or_else(|e| panic!("{e}"));
    // What the Scimitar was attached to after each step, as long as it was on the battlefield.
    let mut attached = Vec::new();
    beryl_scenario::watch_with(
        &CARDS,
        &RECONFIGURED,
        beryl_scenario::Slots::Dealt,
        |s, _| {
            let scimitar = s
                .permanents()
                .map(|(slot, _)| slot)
                .find(|&slot| s.definition_name(&t, slot) == props::EQUIPMENT);
            let on = scimitar.map(|slot| {
                s.object(slot)
                    .attached_to()
                    .map(|host| s.definition_name(&t, host))
            });
            if attached.last() != Some(&on) {
                attached.push(on);
            }
        },
    )
    .unwrap_or_else(|e| panic!("{e}"));
    // Unattached, then on the Spellbinder, then unattached and still on the battlefield.
    assert_eq!(
        attached,
        [None, Some(props::CREATURE), None].map(Some),
        "the Scimitar's host after each step"
    );
}
