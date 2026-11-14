use beryl_session::protocol::ZoneName;
use beryl_session::script::{Act, Fact, Verb};
use beryl_table::action::{Adjustment, Destination, Position, SeenObject, Selection, SortBy};
use beryl_table::state::{Card, Face, ObjectId, Row, Zone};
use beryl_table::testing::{Generator, Kind};
use beryl_table::{Action, Table};

mod script_common;
use script_common::{FIELD, GUEST, Game, HOST, add, card, select};

const HAND: Zone = Zone::Hand(HOST);

const KINDS: &[Kind] = &[
    Kind::Move,
    Kind::Draw,
    Kind::Give,
    Kind::Mulligan,
    Kind::Reorder,
    Kind::Pin,
    Kind::SortHand,
    Kind::Attach,
    Kind::Unattach,
    Kind::Tap,
    Kind::Face,
    Kind::Peek,
    Kind::Counter,
    Kind::Stats,
    Kind::Damage,
    Kind::Life,
    Kind::PlayerCounter,
    Kind::Token,
    Kind::Clone,
    Kind::Marker,
    Kind::Remove,
    Kind::Flag,
    Kind::Note,
    Kind::Look,
    Kind::Lend,
    Kind::Reveal,
    Kind::AlwaysLookAtTop,
    Kind::AlwaysRevealTop,
    Kind::Arrow,
    Kind::ClearArrows,
    Kind::Roll,
    Kind::Phase,
    Kind::NextTurn,
    Kind::Shuffle,
];

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
            Action::PlayerCounter {
                seat: GUEST,
                name: "energy".into(),
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
fn one_thousand_random_card_actions_replay_for_both_seats() {
    let mut table = Table::new(2);
    for i in 0..16 {
        add(
            &mut table,
            i,
            &format!("<library-{i}>"),
            Zone::Library(HOST),
            &[],
        );
    }
    let mut game = Game::new(table);
    let mut generator = Generator::new(227);
    let mut tested = 0;
    for _ in 0..2_000 {
        let (_, by, action) = generator.next(&game.table, KINDS);
        let Ok(scripts) = game.try_apply(by, &action) else {
            continue;
        };
        tested += 1;
        for seat in game.table.seats() {
            let json = serde_json::to_string(&scripts[usize::from(seat.0)]).unwrap();
            for object in game.table.objects.values() {
                let Card::Custom { name, .. } = &object.card else {
                    continue;
                };
                // Clones can share a name with a hidden card. Check the
                // record at every hidden card's current slot even then.
                if object.known_to.contains(&seat) {
                    continue;
                }
                let (view, slots) = &game.views[usize::from(seat.0)];
                assert!(
                    scripts[usize::from(seat.0)]
                        .beats
                        .iter()
                        .flatten()
                        .all(|step| {
                            step.act.object().is_none_or(|record| {
                                Some(record.slot) != slots.get(object.id) || record.name.is_none()
                            })
                        }),
                    "{action:?} named hidden slot {:?}",
                    slots.get(object.id)
                );
                if view
                    .objects
                    .iter()
                    .all(|record| record.name.as_ref() != Some(name))
                {
                    assert!(
                        !json.contains(name),
                        "{action:?} leaked {name} to {seat:?}: {json}"
                    );
                }
            }
        }
        if tested == 1_000 {
            break;
        }
    }
    assert_eq!(tested, 1_000);
}
