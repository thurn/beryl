use beryl_engine::{Zone, event::Event};
use beryl_scenario::{Action, P0, P1, Permanent, Scenario, Slots};

use crate::{CARDS, names};

#[test]
fn linked_exile_reports_permanents_in_battlefield_order() {
    const LANDS: [&str; 2] = [names::PLAINS, names::ISLAND];
    const SCENARIO: Scenario = Scenario::new("Worldgorger Dragon exiles two different lands")
        .permanents(
            P0,
            &[
                Permanent::new(LANDS[0]),
                Permanent::new(LANDS[1]),
                Permanent::new(names::MOUNTAIN),
                Permanent::new(names::MOUNTAIN),
                Permanent::new(names::MOUNTAIN),
                Permanent::new(names::MOUNTAIN),
            ],
        )
        .hand(P0, &[names::WORLDGORGER_DRAGON])
        .library(P0, &[names::ISLAND])
        .library(P1, &[names::ISLAND])
        .actions(&[
            Action::cast(P0, names::WORLDGORGER_DRAGON),
            Action::Resolve,
            Action::Resolve,
        ]);
    let (table, _) = beryl_scenario::position(&CARDS, &SCENARIO).unwrap();
    for slots in Slots::ALL {
        let mut exiled = Vec::new();
        beryl_scenario::watch_with(&CARDS, &SCENARIO, slots, |state, events| {
            for event in events {
                if let Event::ZoneChange {
                    object,
                    from: Zone::Battlefield,
                    to: Zone::Exile,
                    ..
                } = event
                {
                    let name = state.definition_name(&table, object.slot);
                    if LANDS.contains(&name) {
                        exiled.push(name);
                    }
                }
            }
        })
        .unwrap_or_else(|error| panic!("{slots:?} slots: {error}"));
        assert_eq!(exiled, LANDS, "{slots:?} slots");
    }
}
