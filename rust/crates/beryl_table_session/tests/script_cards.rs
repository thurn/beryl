use beryl_session::protocol::ZoneName;
use beryl_session::script::{Act, Fact, Table as Stage, Tally, Verb, replay};
use beryl_table::action::{Adjustment, Destination, Position, SeenObject, Selection, SortBy};
use beryl_table::state::{Face, ObjectId, Row, Zone};
use beryl_table::{Action, Table};

mod script_common;
use script_common::{FIELD, GUEST, Game, HOST, add, card, select};

const HAND: Zone = Zone::Hand(HOST);

fn movement(selection: Selection, to: Zone, position: Position) -> Action {
    Action::Move {
        selection,
        to: Destination::Zone(to),
        position,
        face: None,
        tapped: None,
    }
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one table covers each card action in turn"
)]
fn card_changes_use_the_expected_verbs_for_both_seats() {
    let mut table = Table::new(2);
    add(&mut table, 1, "Bear One", FIELD, &[HOST, GUEST]);
    add(&mut table, 2, "Bear Two", FIELD, &[HOST, GUEST]);
    add(&mut table, 3, "Second", HAND, &[HOST]);
    add(&mut table, 4, "First", HAND, &[HOST]);
    for id in 5..9 {
        add(
            &mut table,
            id,
            &format!("Library {id}"),
            Zone::Library(HOST),
            &[],
        );
    }
    let mut game = Game::new(table);
    let creature = || select(FIELD, &[1]);
    for (action, verb) in [
        (
            Action::Reorder {
                selection: creature(),
                position: Position::Bottom,
            },
            Verb::Fly,
        ),
        (
            Action::Pin {
                selection: creature(),
                row: Row::Others,
            },
            Verb::Fly,
        ),
        (
            Action::SortHand {
                seat: HOST,
                by: SortBy::Name,
            },
            Verb::Fly,
        ),
        (
            Action::Attach {
                selection: creature(),
                host: SeenObject {
                    id: ObjectId(2),
                    zone: FIELD,
                },
            },
            Verb::Fly,
        ),
        (
            Action::Unattach {
                selection: creature(),
            },
            Verb::Fly,
        ),
        (
            Action::Tap {
                selection: creature(),
                tapped: true,
            },
            Verb::Tap,
        ),
        (
            Action::Face {
                selection: creature(),
                face: Face::Down,
            },
            Verb::Flip,
        ),
        (
            Action::Counter {
                selection: creature(),
                name: "+1/+1".into(),
                adjustment: Adjustment::Set(3),
            },
            Verb::Count,
        ),
        (
            Action::Counter {
                selection: creature(),
                name: "custom".into(),
                adjustment: Adjustment::Set(4),
            },
            Verb::Count,
        ),
        (
            Action::Stats {
                selection: creature(),
                stats: Some((3, 5)),
            },
            Verb::Count,
        ),
        (
            Action::Damage {
                selection: creature(),
                damage: 2,
            },
            Verb::Count,
        ),
        (
            Action::Life {
                seat: GUEST,
                adjustment: Adjustment::Add(-3),
            },
            Verb::Count,
        ),
        (
            Action::PlayerCounter {
                seat: HOST,
                name: "U".into(),
                adjustment: Adjustment::Set(1),
            },
            Verb::Count,
        ),
        (
            Action::Flag {
                selection: creature(),
                doesnt_untap: true,
            },
            Verb::Set,
        ),
        (
            Action::Note {
                selection: creature(),
                note: Some("remember".into()),
            },
            Verb::Set,
        ),
        (
            Action::Clone {
                selection: creature(),
            },
            Verb::Appear,
        ),
        (
            Action::Token {
                card: card("New Token"),
                count: 2,
                attached_to: None,
                note: None,
                face_down: false,
            },
            Verb::Appear,
        ),
        (
            Action::Marker {
                selection: select(FIELD, &[1, 2]),
                text: "An ability".into(),
            },
            Verb::Emerge,
        ),
        (
            movement(select(HAND, &[3, 4]), Zone::Stack, Position::Bottom),
            Verb::Fly,
        ),
        (
            Action::Draw {
                seat: HOST,
                count: 2,
            },
            Verb::Fly,
        ),
        (
            Action::Give {
                selection: creature(),
                to: GUEST,
            },
            Verb::Fly,
        ),
        (
            Action::Mulligan {
                seat: HOST,
                count: 2,
            },
            Verb::Fly,
        ),
    ] {
        game.verbs(HOST, &action, verb);
    }
    let marker = game.table.markers.keys().next().unwrap().0;
    game.verbs(
        HOST,
        &Action::Remove {
            selection: select(Zone::Stack, &[marker]),
        },
        Verb::Vanish,
    );
}

#[test]
fn peeking_turns_the_card_without_disclosing_its_face_to_the_other_seat() {
    let mut table = Table::new(2);
    add(&mut table, 1, "Secret Face", FIELD, &[]);
    table.objects.get_mut(&ObjectId(1)).unwrap().face = Face::Down;
    let mut game = Game::new(table);
    let [owner, guest] = game.apply(
        HOST,
        &Action::Peek {
            selection: select(FIELD, &[1]),
        },
    );
    for script in [&owner, &guest] {
        assert!(matches!(script.beats[0][0].act, Act::Flip { .. }));
    }
    assert!(
        serde_json::to_string(&owner)
            .unwrap()
            .contains("Secret Face")
    );
    assert!(
        !serde_json::to_string(&guest)
            .unwrap()
            .contains("Secret Face")
    );
}

#[test]
fn a_draw_moves_anonymous_backs_as_one_group() {
    let mut table = Table::new(2);
    add(
        &mut table,
        1,
        "Top Card",
        Zone::Library(HOST),
        &[HOST, GUEST],
    );
    add(&mut table, 2, "Second Card", Zone::Library(HOST), &[]);
    let mut game = Game::new(table);
    let old = game.views[1].1.get(ObjectId(1)).unwrap();
    let [owner, guest] = game.apply(
        HOST,
        &Action::Draw {
            seat: HOST,
            count: 2,
        },
    );
    assert_eq!(owner.beats.len(), 1);
    assert_eq!(guest.beats.len(), 1);
    assert_eq!(
        guest.beats[0]
            .iter()
            .filter(|step| step.act.verb() == Verb::Fly)
            .count(),
        2
    );
    assert!(matches!(
        guest.beats[0][0].act,
        Act::Set {
            fact: Fact::Forget { .. }
        }
    ));
    for step in guest.beats[0].iter().skip(1) {
        let Act::Fly {
            origin: Some(origin),
            object,
            ..
        } = &step.act
        else {
            panic!("{step}")
        };
        assert_eq!(origin.zone, ZoneName::Library);
        assert!(object.name.is_none());
        assert_ne!(object.slot, old);
    }
    assert!(!serde_json::to_string(&guest).unwrap().contains("Top Card"));
}

#[test]
fn undo_of_bounce_restores_a_token_without_flying_it_from_hand() {
    let mut table = Table::new(2);
    add(&mut table, 1, "Card", FIELD, &[HOST, GUEST]);
    add(&mut table, 2, "Token", FIELD, &[HOST, GUEST]);
    table.objects.get_mut(&ObjectId(2)).unwrap().token = true;
    let mut game = Game::new(table);
    game.apply(HOST, &movement(select(FIELD, &[1, 2]), HAND, Position::Top));

    let scripts = game.apply(HOST, &Action::Undo { entry: None });
    for (index, script) in scripts.iter().enumerate() {
        let slots = &game.views[index].1;
        let card = slots.get(ObjectId(1)).unwrap();
        let token = slots.get(ObjectId(2)).unwrap();
        let arrivals = script
            .beats
            .iter()
            .flatten()
            .filter_map(|step| match &step.act {
                Act::Fly { object, .. } | Act::Appear { object, .. } => {
                    Some((step.act.verb(), object.slot))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(arrivals.len(), 2, "{script}");
        assert!(arrivals.contains(&(Verb::Fly, card)), "{script}");
        assert!(arrivals.contains(&(Verb::Appear, token)), "{script}");
    }
}

#[test]
fn player_counters_are_part_of_script_replay() {
    let mut game = Game::new(Table::new(2));
    let before = game.views.each_ref().map(|(view, _)| Stage::of(view));
    let scripts = game.apply(
        GUEST,
        &Action::PlayerCounter {
            seat: GUEST,
            name: "energy".into(),
            adjustment: Adjustment::Set(3),
        },
    );
    for (index, mut script) in scripts.into_iter().enumerate() {
        for beat in &mut script.beats {
            beat.retain(|step| {
                !matches!(
                    step.act,
                    Act::Count {
                        tally: Tally::PlayerCounters { .. }
                    }
                )
            });
        }
        script.beats.retain(|beat| !beat.is_empty());
        let (after, _) = &game.views[index];
        assert!(replay::failure(&before[index], &script, after).is_some());
    }
}
