use std::collections::BTreeSet;

use beryl_table::action::{Adjustment, Destination, Portion, Position, Selection};
use beryl_table::log::line;
use beryl_table::redact::{entry_seen_by, seen_by};
use beryl_table::state::{
    Assist, Card, Face, Line, LineVisibility, Object, ObjectId, Proposal, Zone,
};
use beryl_table::{Action, Chance, Change, Seat, Table, apply};

mod common;
use common::random::{ASSIST_AND_LOG, CORE, Generator, Kind};

const HOST: Seat = Seat(0);
const GUEST: Seat = Seat(1);
const LIBRARY: Zone = Zone::Library(HOST);
const HAND: Zone = Zone::Hand(HOST);

fn add(table: &mut Table, id: ObjectId, name: &str, zone: Zone, known_to: &[Seat]) {
    let index = table.zone(zone).unwrap().len();
    let mut object = Object::new(id, Card::Named(name.into()), HOST, zone);
    object.known_to = known_to.to_vec();
    Change::Create { object, index }.perform(table);
}

#[test]
fn a_drawn_card_has_the_same_id_in_both_views_and_only_the_drawer_sees_its_name() {
    let mut table = Table::new(2);
    let name = "Brainstorm";
    add(&mut table, ObjectId(76), name, LIBRARY, &[GUEST]);
    let before = table.clone();
    let entry = apply(
        &mut table,
        HOST,
        &Action::Draw {
            seat: HOST,
            count: 1,
        },
        &mut Chance::fresh(1),
    )
    .unwrap();

    let host = seen_by(&table, HOST);
    let guest = seen_by(&table, GUEST);
    assert_eq!(host.zone(HAND).unwrap(), &[ObjectId(77)]);
    assert_eq!(host.zone(HAND), guest.zone(HAND));
    assert_eq!(host.objects[&ObjectId(77)].card, Card::Named(name.into()));
    assert_eq!(guest.objects[&ObjectId(77)].card, Card::Hidden);
    let guest_entry = entry_seen_by(&entry, &before, GUEST);
    assert!(!guest_entry.changes.iter().any(|change| matches!(
        change,
        Change::Move {
            id: ObjectId(76),
            ..
        } | Change::ReplaceId {
            from: ObjectId(76),
            ..
        }
    )));
    assert!(
        !serde_json::to_string(&(guest, guest_entry))
            .unwrap()
            .contains(name)
    );
}

#[test]
fn a_shuffle_preserves_the_new_order_without_showing_how_the_cards_were_reordered() {
    let mut table = Table::new(2);
    for (index, name) in ["First Card", "Second Card", "Third Card"]
        .into_iter()
        .enumerate()
    {
        add(
            &mut table,
            ObjectId(u32::try_from(index).unwrap() + 1),
            name,
            LIBRARY,
            &[HOST, GUEST],
        );
    }
    let before = table.clone();
    let entry = apply(
        &mut table,
        HOST,
        &Action::Shuffle {
            seat: HOST,
            portion: Portion::Top(2),
        },
        &mut Chance::replaying(vec![1]),
    )
    .unwrap();

    assert!(
        entry
            .changes
            .iter()
            .any(|change| matches!(change, Change::Random { .. }))
    );
    for seat in table.seats() {
        let view = seen_by(&table, seat);
        assert_eq!(view.zone(LIBRARY), table.zone(LIBRARY));
        let order = view.zone(LIBRARY).unwrap();
        assert_eq!(view.objects[&order[0]].card, Card::Hidden);
        assert_eq!(view.objects[&order[1]].card, Card::Hidden);
        assert_eq!(
            view.objects[&order[2]].card,
            Card::Named("Third Card".into())
        );
        assert!(entry_seen_by(&entry, &before, seat).changes.is_empty());
    }
}

#[test]
fn a_look_projects_the_new_knowledge_only_for_the_seat_that_learned_it() {
    let mut table = Table::new(2);
    let id = ObjectId(1);
    add(&mut table, id, "First Card", LIBRARY, &[]);
    let before = table.clone();
    let entry = apply(
        &mut table,
        HOST,
        &Action::Look {
            seat: HOST,
            portion: Portion::Top(1),
        },
        &mut Chance::fresh(1),
    )
    .unwrap();

    assert_eq!(
        entry_seen_by(&entry, &before, HOST).changes,
        vec![Change::Learn { id, seat: HOST }]
    );
    assert!(entry_seen_by(&entry, &before, GUEST).changes.is_empty());
    assert_eq!(seen_by(&table, GUEST).objects[&id].card, Card::Hidden);
}

#[test]
fn proposal_text_and_actions_obey_the_same_visibility_as_the_table() {
    let mut table = Table::new(2);
    let name = "A Private Card";
    let id = ObjectId(1);
    add(&mut table, id, name, HAND, &[HOST]);
    table.proposals.push(Proposal {
        assist: Assist::TurnUpkeep,
        lines: [
            LineVisibility::Unstructured,
            LineVisibility::KnownObject(id),
        ]
        .into_iter()
        .map(|visibility| Line {
            text: format!("Remember {}", name.to_uppercase()),
            visibility,
            whose: HOST,
            on: true,
            actions: vec![Action::Batch {
                actions: vec![Action::Marker {
                    selection: Selection {
                        objects: vec![id],
                        zone: HAND,
                    },
                    text: "Private ability text".into(),
                }],
            }],
        })
        .collect(),
    });
    let host = seen_by(&table, HOST);
    assert_eq!(host.proposals, table.proposals);
    let guest = seen_by(&table, GUEST);
    assert!(
        guest.proposals[0]
            .lines
            .iter()
            .all(|line| line.text.is_empty())
    );
    let serialized = serde_json::to_string(&guest).unwrap();
    assert!(!serialized.contains(name));
    assert!(!serialized.contains("Private ability text"));
}

#[test]
fn public_labels_survive_a_hidden_card_with_the_same_name() {
    let mut table = Table::new(2);
    let id = ObjectId(1);
    let hidden = ObjectId(2);
    add(
        &mut table,
        id,
        "Public Card",
        Zone::Battlefield,
        &[HOST, GUEST],
    );
    add(&mut table, hidden, "Charge", HAND, &[HOST]);
    table.game_match.seats[0].deck.push(("Charge".into(), 1));
    let selection = Selection {
        objects: vec![id],
        zone: Zone::Battlefield,
    };
    let counter = Action::Counter {
        selection: selection.clone(),
        name: "charge".into(),
        adjustment: Adjustment::Add(1),
    };
    let note = Action::Note {
        selection,
        note: Some("Add a charge counter".into()),
    };
    let player_counter = Action::PlayerCounter {
        seat: HOST,
        name: "charge".into(),
        adjustment: Adjustment::Add(1),
    };
    table.proposals.push(Proposal {
        assist: Assist::TurnUpkeep,
        lines: vec![Line {
            text: "Add a charge counter".into(),
            visibility: LineVisibility::Public,
            whose: HOST,
            on: true,
            actions: vec![counter.clone()],
        }],
    });
    let mut chance = Chance::fresh(1);
    for action in [counter, note, player_counter] {
        let before = table.clone();
        let entry = apply(&mut table, HOST, &action, &mut chance).unwrap();
        for seat in table.seats() {
            let projected = entry_seen_by(&entry, &before, seat);
            assert_eq!(projected.action, action);
            assert_eq!(projected.changes, entry.changes);
            if matches!(
                action,
                Action::Counter { .. } | Action::PlayerCounter { .. }
            ) {
                assert!(line(&projected, &seen_by(&table, seat), seat).contains("charge counter"));
            }
        }
    }
    let guest = seen_by(&table, GUEST);
    assert_eq!(guest.proposals, table.proposals);
    assert_eq!(guest.objects[&id], table.objects[&id]);
    assert_eq!(guest.players[0].counters, vec![("charge".into(), 1)]);
    assert_eq!(guest.objects[&hidden].card, Card::Hidden);
    assert!(guest.game_match.seats[0].deck.is_empty());
}

#[test]
fn opponent_deck_names_are_hidden_in_the_lobby_and_in_both_sides_of_an_edit() {
    let mut table = Table::new(2);
    let mut chance = Chance::fresh(1);
    let mut check_load = |name: &str, side: &str, previous: &str| {
        let before = table.clone();
        let entry = apply(
            &mut table,
            HOST,
            &Action::LoadDeck {
                seat: HOST,
                main: vec![(name.into(), 1)],
                side: vec![(side.into(), 1)],
            },
            &mut chance,
        )
        .unwrap();
        let host = entry_seen_by(&entry, &before, HOST);
        assert_eq!(host, entry);
        let guest = serde_json::to_string(&entry_seen_by(&entry, &before, GUEST)).unwrap();
        assert!(!guest.contains(name) && !guest.contains(side) && !guest.contains(previous));
        let view = seen_by(&table, GUEST);
        assert!(view.game_match.seats[0].deck.is_empty());
        assert!(view.game_match.seats[0].sideboard.is_empty());
    };
    check_load("First List", "First Sideboard", "No Previous Deck");
    check_load("New List", "New Sideboard", "First List");
}

#[test]
fn a_counter_cannot_name_its_own_hidden_card() {
    let mut table = Table::new(2);
    let id = ObjectId(1);
    let name = "Private Card";
    add(&mut table, id, name, Zone::Battlefield, &[HOST]);
    table.objects.get_mut(&id).unwrap().face = Face::Down;
    let before = table.clone();
    let entry = apply(
        &mut table,
        HOST,
        &Action::Counter {
            selection: Selection {
                objects: vec![id],
                zone: Zone::Battlefield,
            },
            name: name.into(),
            adjustment: Adjustment::Add(1),
        },
        &mut Chance::fresh(1),
    )
    .unwrap();
    let host = seen_by(&table, HOST);
    assert_eq!(host.objects[&id].counters, vec![(name.into(), 1)]);
    assert_eq!(entry_seen_by(&entry, &before, HOST).action, entry.action);
    let guest = seen_by(&table, GUEST);
    assert!(guest.objects[&id].counters.is_empty());
    let entry = entry_seen_by(&entry, &before, GUEST);
    assert!(!serde_json::to_string(&entry).unwrap().contains(name));
}

#[test]
fn revealing_a_source_later_in_the_batch_keeps_its_marker_private() {
    let mut table = Table::new(2);
    let id = ObjectId(1);
    add(&mut table, id, "Private Card", Zone::Battlefield, &[HOST]);
    table.objects.get_mut(&id).unwrap().face = Face::Down;
    let before = table.clone();
    let selection = Selection {
        objects: vec![id],
        zone: Zone::Battlefield,
    };
    let text = "Private ability";
    let entry = apply(
        &mut table,
        HOST,
        &Action::Batch {
            actions: vec![
                Action::Marker {
                    selection: selection.clone(),
                    text: text.into(),
                },
                Action::Face {
                    selection,
                    face: Face::Front,
                },
            ],
        },
        &mut Chance::fresh(1),
    )
    .unwrap();
    let projected = |seat| entry_seen_by(&entry, &before, seat);
    assert!(
        serde_json::to_string(&projected(HOST))
            .unwrap()
            .contains(text)
    );
    assert!(
        !serde_json::to_string(&projected(GUEST))
            .unwrap()
            .contains(text)
    );
    let guest = seen_by(&table, GUEST);
    assert_eq!(guest.objects[&id].card, table.objects[&id].card);
    assert!(guest.markers[&guest.stack[0]].display_source.is_none());
}

#[test]
fn fixed_seed_exercises_redaction_after_two_thousand_actions() {
    let mut table = Table::new(2);
    let mut generator = Generator::new(227);
    let mut chance = Chance::fresh(481);
    let mut covered = BTreeSet::new();
    for index in 0..2_000 {
        let before = table.clone();
        let (kind, by, action) = generator.next(&table, CORE);
        let entry = apply(&mut table, by, &action, &mut chance)
            .unwrap_or_else(|error| panic!("action {index}: {action:?} failed with {error:?}"));
        covered.insert(kind);
        for seat in table.seats() {
            let view = seen_by(&table, seat);
            let log = entry_seen_by(&entry, &before, seat);
            for object in table.objects.values() {
                let expected = if object.known_to.contains(&seat) {
                    &object.card
                } else {
                    &Card::Hidden
                };
                assert_eq!(&view.objects[&object.id].card, expected, "action {index}");
            }
            let view_json = serde_json::to_string(&view).unwrap();
            let log_json = serde_json::to_string(&log).unwrap();
            assert_hidden_names(&table, seat, &view_json, index);
            assert_hidden_names(&table, seat, &log_json, index);
            assert_hidden_names(&before, seat, &log_json, index);
        }
    }
    let missing: Vec<_> = CORE.iter().filter(|kind| !covered.contains(kind)).collect();
    assert!(missing.is_empty(), "seed missed action kinds {missing:?}");
}

fn assert_hidden_names(table: &Table, seat: Seat, json: &str, index: usize) {
    let (visible, hidden): (Vec<_>, Vec<_>) = table
        .objects
        .values()
        .partition(|object| object.known_to.contains(&seat));
    let own = &table.game_match.seats[usize::from(seat.0)];
    let visible: BTreeSet<_> = visible
        .into_iter()
        .map(|object| name(&object.card))
        .chain(
            own.deck
                .iter()
                .chain(&own.sideboard)
                .map(|(name, _)| name.as_str()),
        )
        .collect();
    for name in hidden.into_iter().map(|object| name(&object.card)) {
        // A visible card or the viewer's own deck list can share the spelling.
        // The view assertion above checks each object's actual identity by id.
        assert!(
            visible.contains(name) || !json.contains(name),
            "action {index} leaked {name} for {seat:?}: {json}"
        );
    }
}

fn name(card: &Card) -> &str {
    match card {
        Card::Named(name) | Card::Custom { name, .. } => name,
        Card::Hidden => panic!("a host table should always have card identities"),
    }
}

#[test]
fn the_generator_has_templates_for_the_assist_and_log_layers() {
    let mut generator = Generator::new(5);
    let mut table = Table::new(2);
    add(&mut table, ObjectId(1), "First Card", LIBRARY, &[]);
    for &kind in ASSIST_AND_LOG {
        let action = generator.action(&table, kind, HOST).unwrap();
        assert!(matches!(
            (kind, action),
            (Kind::SetAssist, Action::SetAssist { .. })
                | (Kind::Flow, Action::Flow { .. })
                | (Kind::Undo, Action::Undo { entry: None })
                | (Kind::Rewind, Action::Rewind { .. })
                | (Kind::Batch, Action::Batch { .. })
        ));
    }
}

#[test]
fn a_public_copy_does_not_expose_the_identity_of_a_different_hidden_object() {
    let mut table = Table::new(2);
    let id = ObjectId(1);
    add(&mut table, id, "Shared Name", LIBRARY, &[]);
    let mut chance = Chance::fresh(1);
    apply(
        &mut table,
        HOST,
        &Action::Move {
            selection: Selection {
                objects: vec![id],
                zone: LIBRARY,
            },
            to: Destination::Zone(Zone::Battlefield),
            position: Position::Top,
            face: Some(beryl_table::state::Face::Down),
            tapped: None,
        },
        &mut chance,
    )
    .unwrap();
    add(
        &mut table,
        ObjectId(2),
        "Shared Name",
        Zone::Battlefield,
        &[HOST, GUEST],
    );
    let guest = seen_by(&table, GUEST);
    assert_eq!(guest.objects[&id].card, Card::Hidden);
    assert_eq!(
        guest.objects[&ObjectId(2)].card,
        Card::Named("Shared Name".into())
    );
}

#[test]
fn projection_keeps_die_rolls_but_hides_random_reveal_offsets() {
    let mut table = Table::new(2);
    add(&mut table, ObjectId(1), "First Card", LIBRARY, &[]);
    add(&mut table, ObjectId(2), "Second Card", LIBRARY, &[]);
    let before = table.clone();
    let entry = apply(
        &mut table,
        HOST,
        &Action::Reveal {
            zone: LIBRARY,
            objects: None,
            random: Some(1),
            to: vec![HOST],
        },
        &mut Chance::replaying(vec![1]),
    )
    .unwrap();
    assert!(matches!(entry.changes[0], Change::Random { .. }));
    assert_eq!(
        entry_seen_by(&entry, &before, HOST).changes,
        vec![Change::Learn {
            id: ObjectId(2),
            seat: HOST
        }],
    );
    assert!(entry_seen_by(&entry, &before, GUEST).changes.is_empty());

    let before = table.clone();
    let roll = apply(
        &mut table,
        HOST,
        &Action::Roll { count: 1, sides: 6 },
        &mut Chance::replaying(vec![3]),
    )
    .unwrap();
    assert_eq!(entry_seen_by(&roll, &before, GUEST), roll);
}

#[test]
fn a_seat_sees_its_deck_list_even_if_another_seat_has_the_same_card() {
    let mut table = Table::new(2);
    let mut chance = Chance::fresh(1);
    let main = vec![("Island".into(), 4)];
    let side = vec![("Plains".into(), 2)];
    add(&mut table, ObjectId(1), "Island", LIBRARY, &[]);
    for seat in [HOST, GUEST] {
        let before = table.clone();
        let action = Action::LoadDeck {
            seat,
            main: main.clone(),
            side: side.clone(),
        };
        let entry = apply(&mut table, seat, &action, &mut chance).unwrap();
        assert_eq!(entry_seen_by(&entry, &before, seat).action, action);
    }

    let guest = seen_by(&table, GUEST);
    assert_eq!(guest.game_match.seats[1].deck, main);
    assert_eq!(guest.game_match.seats[1].sideboard, side);
    assert!(guest.game_match.seats[0].deck.is_empty());
    assert_eq!(guest.objects[&ObjectId(1)].card, Card::Hidden);
}

#[test]
fn batched_rolls_keep_only_the_public_random_outcomes() {
    let mut table = Table::new(2);
    add(&mut table, ObjectId(1), "First Card", LIBRARY, &[]);
    add(&mut table, ObjectId(2), "Second Card", LIBRARY, &[]);
    let before = table.clone();
    let entry = apply(
        &mut table,
        HOST,
        &Action::Batch {
            actions: vec![
                Action::Roll { count: 1, sides: 6 },
                Action::Batch {
                    actions: vec![
                        Action::Shuffle {
                            seat: HOST,
                            portion: Portion::All,
                        },
                        Action::Roll {
                            count: 1,
                            sides: 20,
                        },
                    ],
                },
            ],
        },
        &mut Chance::replaying(vec![2, 1, 16]),
    )
    .unwrap();

    let entry = entry_seen_by(&entry, &before, GUEST);
    assert_eq!(
        entry.changes,
        vec![
            Change::Roll {
                sides: 6,
                outcomes: vec![2]
            },
            Change::Roll {
                sides: 20,
                outcomes: vec![16]
            },
        ]
    );
}

#[test]
fn another_seat_cannot_clone_a_card_it_cannot_identify() {
    let mut table = Table::new(2);
    let id = ObjectId(1);
    add(&mut table, id, "First Card", LIBRARY, &[]);
    let mut chance = Chance::fresh(1);
    apply(
        &mut table,
        HOST,
        &Action::Lend {
            zone: LIBRARY,
            to: GUEST,
        },
        &mut chance,
    )
    .unwrap();
    let action = Action::Clone {
        selection: Selection {
            objects: vec![id],
            zone: LIBRARY,
        },
    };
    let before = table.clone();
    assert_eq!(
        apply(&mut table, GUEST, &action, &mut chance),
        Err(beryl_table::Refusal::Permission),
    );
    assert_eq!(table, before);
}
