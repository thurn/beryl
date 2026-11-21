use beryl_engine::{Zone, event::Event};
use beryl_scenario::{Action, P0, P1, Permanent, Scenario, Slots};

use crate::{CARDS, names};

#[test]
fn simultaneous_entry_choices_and_events_match_in_both_layouts() {
    use beryl_card_shared::Prop;
    use beryl_engine::state::Pending;
    use beryl_engine::{DecisionKind, Relayout, Replay, Scratch, Step, apply, next};
    use beryl_scenario::answer;

    let scenario = super::fixtures::SHOW_AND_TELL_AURAS;
    let (table, _) = beryl_scenario::position(&CARDS, scenario).unwrap();
    for slots in Slots::ALL {
        let mut checked = false;
        beryl_scenario::watch_with(&CARDS, scenario, slots, |state, _| {
            let Pending::Resolving(resolving) = state.pending() else {
                return;
            };
            if checked || resolving.question.kind != DecisionKind::ChooseObject {
                return;
            }
            // Start both layouts at the first attachment question, after the hand picks.
            let mut small = *state;
            let mut large = small.widen();
            assert_eq!(large.try_relayout(&table), Ok(small));
            let (mut small_scratch, mut large_scratch) =
                (Scratch::recording(), Scratch::recording());
            for (player, host) in [(P0, Prop::Flier), (P1, Prop::Vanilla)] {
                let step = next(&mut small, &table, &mut small_scratch, &mut Replay);
                assert_eq!(
                    next(&mut large, &table, &mut large_scratch, &mut Replay),
                    step.relayout()
                );
                let Step::Decide(decision) = step else {
                    panic!("expected an Aura attachment choice");
                };
                assert_eq!(
                    (decision.player, decision.kind, decision.len()),
                    (player, DecisionKind::ChooseObject, 3)
                );
                let action = Action::choose(player, DecisionKind::ChooseObject, host);
                let choice = answer(&table, &small, &decision, &action).unwrap();
                apply(&mut small, &table, &mut small_scratch, &mut Replay, choice);
                apply(&mut large, &table, &mut large_scratch, &mut Replay, choice);
                assert_eq!(
                    small.widen(),
                    large,
                    "{slots:?} slots after {player:?} chose"
                );
            }
            let entries: Vec<_> = small_scratch
                .events
                .events()
                .iter()
                .zip(small_scratch.events.tags())
                .filter_map(|(event, tag)| match event {
                    Event::ZoneChange {
                        object,
                        from: Zone::Hand,
                        to: Zone::Battlefield,
                        ..
                    } => Some((small.definition_name(&table, object.slot), tag.together)),
                    _ => None,
                })
                .collect();
            assert_eq!(
                entries,
                [
                    (names::HOLY_STRENGTH, false),
                    (names::SWIFT_RECONFIGURATION, true)
                ]
            );
            assert!(
                small_scratch
                    .events
                    .events()
                    .iter()
                    .map(Relayout::relayout)
                    .eq(large_scratch.events.events().iter().cloned())
            );
            assert!(
                small_scratch
                    .events
                    .tags()
                    .iter()
                    .map(Relayout::relayout)
                    .eq(large_scratch.events.tags().iter().copied())
            );
            checked = true;
        })
        .unwrap_or_else(|error| panic!("{slots:?} slots: {error}"));
        assert!(checked, "{slots:?} slots: no attachment question");
    }
}

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
