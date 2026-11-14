use beryl_table::action::{Destination, Portion, Position, Selection, SortBy};
use beryl_table::apply::Entry;
use beryl_table::state::{Arrow, ArrowKind, ArrowTarget, Card, Face, Object, ObjectId, Zone};
use beryl_table::{Action, Chance, Change, Refusal, Seat, Table, apply};

const OWNER: Seat = Seat(0);
const GUEST: Seat = Seat(1);
const OTHER: Seat = Seat(2);
const LIBRARY: Zone = Zone::Library(OWNER);
const HAND: Zone = Zone::Hand(OWNER);

fn ids(values: &[u32]) -> Vec<ObjectId> {
    values.iter().copied().map(ObjectId).collect()
}

fn seed_zone(table: &mut Table, zone: Zone, cards: &[(u32, &str)], known_to: &[Seat]) {
    for (index, &(id, name)) in cards.iter().enumerate() {
        let mut object = Object::new(ObjectId(id), Card::Named(name.into()), OWNER, zone);
        object.known_to = known_to.to_vec();
        Change::Create { object, index }.perform(table);
    }
}

fn library() -> Table {
    let mut table = Table::new(3);
    seed_zone(
        &mut table,
        LIBRARY,
        &[
            (1, "Island"),
            (2, "Lightning Bolt"),
            (3, "Divination"),
            (4, "Serra Angel"),
        ],
        &[],
    );
    table
}

fn act(table: &mut Table, by: Seat, action: &Action) -> Entry {
    apply(table, by, action, &mut Chance::fresh(91)).unwrap()
}

fn names(table: &Table, zone: Zone) -> Vec<&str> {
    table
        .zone(zone)
        .unwrap()
        .iter()
        .map(|id| match &table.objects[id].card {
            Card::Named(name) | Card::Custom { name, .. } => name.as_str(),
            Card::Hidden => panic!("expected an unredacted card"),
        })
        .collect()
}

fn knowledge(table: &Table, zone: Zone, index: usize) -> &[Seat] {
    let id = table.zone(zone).unwrap()[index];
    &table.objects[&id].known_to
}

fn outcomes(entry: &Entry) -> Vec<u32> {
    entry
        .changes
        .iter()
        .flat_map(|change| match change {
            Change::Random { outcomes } => outcomes.as_slice(),
            _ => &[],
        })
        .copied()
        .collect()
}

fn undo(table: &mut Table, entry: &Entry) {
    for change in entry.changes.iter().rev() {
        change.inverse().perform(table);
    }
}

#[test]
fn entering_a_hand_refreshes_ids_and_leaves_only_the_hand_owner_in_known_to() {
    let mut table = library();
    seed_zone(&mut table, HAND, &[(5, "Counterspell")], &[OWNER]);
    for card in table
        .objects
        .values_mut()
        .filter(|card| card.zone == LIBRARY)
    {
        card.known_to = vec![GUEST, OTHER];
    }

    let entry = act(
        &mut table,
        OWNER,
        &Action::Draw {
            seat: OWNER,
            count: 2,
        },
    );
    assert_eq!(
        names(&table, HAND),
        ["Counterspell", "Island", "Lightning Bolt"]
    );
    assert_eq!(table.zone(LIBRARY).unwrap(), ids(&[3, 4]));
    for index in 1..3 {
        assert!(table.zone(HAND).unwrap()[index].0 > 5);
        assert_eq!(knowledge(&table, HAND, index), [OWNER]);
    }
    assert_eq!(knowledge(&table, LIBRARY, 0), [GUEST, OTHER]);

    undo(&mut table, &entry);
    assert_eq!(table.zone(HAND).unwrap(), ids(&[5]));
    assert_eq!(table.zone(LIBRARY).unwrap(), ids(&[1, 2, 3, 4]));
    assert_eq!(knowledge(&table, LIBRARY, 0), [GUEST, OTHER, OWNER]);
}

#[test]
fn drawing_past_the_end_takes_only_the_remaining_cards() {
    let mut table = library();
    let draw = Action::Draw {
        seat: OWNER,
        count: 9,
    };
    act(&mut table, OWNER, &draw);
    assert!(table.zone(LIBRARY).unwrap().is_empty());
    assert_eq!(table.zone(HAND).unwrap().len(), 4);
    assert!(act(&mut table, OWNER, &draw).changes.is_empty());
}

#[test]
fn mulligan_shuffles_the_returned_hand_into_the_library_and_draws_the_requested_count() {
    let mut table = library();
    seed_zone(&mut table, HAND, &[(5, "Counterspell")], &[OWNER, GUEST]);
    let action = Action::Mulligan {
        seat: OWNER,
        count: 2,
    };
    // The first two swaps pick the last card and then the former hand card.
    let mut chance = Chance::replaying(vec![4, 3, 0, 0]);
    let entry = apply(&mut table, OWNER, &action, &mut chance).unwrap();
    assert_eq!(names(&table, HAND), ["Serra Angel", "Counterspell"]);
    assert_eq!(
        names(&table, LIBRARY),
        ["Lightning Bolt", "Divination", "Island"]
    );
    assert!(table.objects.keys().all(|id| id.0 > 5));
    assert_eq!(knowledge(&table, HAND, 1), [OWNER]);
    assert_eq!(outcomes(&entry), [4, 3, 0, 0]);

    undo(&mut table, &entry);
    assert_eq!(table.zone(HAND).unwrap(), ids(&[5]));
    assert_eq!(table.zone(LIBRARY).unwrap(), ids(&[1, 2, 3, 4]));
    assert_eq!(knowledge(&table, HAND, 0), [OWNER, GUEST]);
}

#[test]
fn shuffle_forgets_and_replaces_ids_only_in_the_requested_portion() {
    for (portion, range, expected) in [
        (
            Portion::Top(2),
            0..2,
            ["Lightning Bolt", "Island", "Divination", "Serra Angel"],
        ),
        (
            Portion::Bottom(2),
            2..4,
            ["Island", "Lightning Bolt", "Serra Angel", "Divination"],
        ),
        (
            Portion::All,
            0..4,
            ["Serra Angel", "Island", "Lightning Bolt", "Divination"],
        ),
        (
            Portion::Bottom(1),
            3..4,
            ["Island", "Lightning Bolt", "Divination", "Serra Angel"],
        ),
    ] {
        let mut table = library();
        for card in table.objects.values_mut() {
            card.known_to = vec![OWNER, GUEST];
        }
        let swaps = match range.len() {
            4 => vec![3, 2, 1],
            2 => vec![1],
            _ => vec![],
        };
        let entry = apply(
            &mut table,
            OWNER,
            &Action::Shuffle {
                seat: OWNER,
                portion,
            },
            &mut Chance::replaying(swaps),
        )
        .unwrap();
        assert_eq!(names(&table, LIBRARY), expected);
        for (index, &id) in table.zone(LIBRARY).unwrap().iter().enumerate() {
            if range.contains(&index) {
                assert!(id.0 > 4);
                assert!(table.objects[&id].known_to.is_empty());
            } else {
                assert_eq!(id, ids(&[1, 2, 3, 4])[index]);
                assert_eq!(table.objects[&id].known_to, [OWNER, GUEST]);
            }
        }
        undo(&mut table, &entry);
        assert_eq!(table.zone(LIBRARY).unwrap(), ids(&[1, 2, 3, 4]));
        assert!(
            table
                .objects
                .values()
                .all(|card| card.known_to == [OWNER, GUEST])
        );
    }
}

#[test]
fn recorded_shuffle_outcomes_replay_the_order_and_ids() {
    let mut original = library();
    let mut replay = original.clone();
    let shuffle = Action::Shuffle {
        seat: OWNER,
        portion: Portion::All,
    };
    let entry = act(&mut original, OWNER, &shuffle);
    let saved = serde_json::to_string(&entry).unwrap();
    let saved_entry: Entry = serde_json::from_str(&saved).unwrap();
    let recorded = outcomes(&saved_entry);
    assert_eq!(recorded.len(), 3);
    let replay_entry = apply(
        &mut replay,
        OWNER,
        &shuffle,
        &mut Chance::replaying(recorded),
    )
    .unwrap();
    assert_eq!(original, replay);
    assert_eq!(entry, replay_entry);
}

#[test]
fn a_failed_random_action_leaves_the_table_and_chance_untouched() {
    let mut table = library();
    let before = table.clone();
    let mut chance = Chance::replaying(vec![1, 3]);
    let before_chance = chance.clone();
    assert_eq!(
        apply(
            &mut table,
            OWNER,
            &Action::Shuffle {
                seat: OWNER,
                portion: Portion::All
            },
            &mut chance
        ),
        Err(Refusal::Invalid),
    );
    assert_eq!(table, before);
    assert_eq!(chance, before_chance);
}

#[test]
fn looking_adds_only_the_looking_seat_to_known_to() {
    let mut table = library();
    table
        .objects
        .get_mut(&ObjectId(4))
        .unwrap()
        .known_to
        .push(OTHER);
    act(
        &mut table,
        OWNER,
        &Action::Lend {
            zone: LIBRARY,
            to: GUEST,
        },
    );
    act(
        &mut table,
        GUEST,
        &Action::Look {
            seat: OWNER,
            portion: Portion::Top(1),
        },
    );
    act(
        &mut table,
        GUEST,
        &Action::Look {
            seat: OWNER,
            portion: Portion::Bottom(2),
        },
    );
    assert_eq!(knowledge(&table, LIBRARY, 0), [GUEST]);
    assert!(knowledge(&table, LIBRARY, 1).is_empty());
    assert_eq!(knowledge(&table, LIBRARY, 2), [GUEST]);
    assert_eq!(knowledge(&table, LIBRARY, 3), [OTHER, GUEST]);
    let entry = act(
        &mut table,
        OWNER,
        &Action::Look {
            seat: OWNER,
            portion: Portion::All,
        },
    );
    undo(&mut table, &entry);
    assert_eq!(knowledge(&table, LIBRARY, 1), [OWNER]);
    assert_eq!(knowledge(&table, LIBRARY, 3), [OTHER, GUEST, OWNER]);
}

#[test]
fn revealing_selected_cards_or_a_whole_hand_adds_only_the_named_seats() {
    let mut table = Table::new(3);
    seed_zone(
        &mut table,
        HAND,
        &[(1, "Island"), (2, "Lightning Bolt")],
        &[OWNER],
    );
    act(
        &mut table,
        OWNER,
        &Action::Reveal {
            zone: HAND,
            objects: Some(ids(&[2])),
            random: None,
            to: vec![GUEST],
        },
    );
    assert_eq!(knowledge(&table, HAND, 0), [OWNER]);
    assert_eq!(knowledge(&table, HAND, 1), [OWNER, GUEST]);
    act(
        &mut table,
        OWNER,
        &Action::Reveal {
            zone: HAND,
            objects: None,
            random: None,
            to: vec![OTHER, OTHER],
        },
    );
    assert_eq!(knowledge(&table, HAND, 0), [OWNER, OTHER]);
    assert_eq!(knowledge(&table, HAND, 1), [OWNER, GUEST, OTHER]);
}

#[test]
fn random_reveal_selects_distinct_cards_and_records_the_draws() {
    let mut table = library();
    let entry = apply(
        &mut table,
        OWNER,
        &Action::Reveal {
            zone: LIBRARY,
            objects: None,
            random: Some(2),
            to: vec![GUEST],
        },
        &mut Chance::replaying(vec![3, 2]),
    )
    .unwrap();
    assert_eq!(outcomes(&entry), [3, 2]);
    assert_eq!(table.zone(LIBRARY).unwrap(), ids(&[1, 2, 3, 4]));
    assert_eq!(knowledge(&table, LIBRARY, 0), [GUEST]);
    assert!(knowledge(&table, LIBRARY, 1).is_empty());
    assert!(knowledge(&table, LIBRARY, 2).is_empty());
    assert_eq!(knowledge(&table, LIBRARY, 3), [GUEST]);
}

#[test]
fn lending_changes_access_until_that_library_is_shuffled() {
    let mut table = library();
    let lend = Action::Lend {
        zone: LIBRARY,
        to: GUEST,
    };
    act(&mut table, OWNER, &lend);
    assert!(act(&mut table, OWNER, &lend).changes.is_empty());
    assert!(knowledge(&table, LIBRARY, 0).is_empty());
    act(
        &mut table,
        OWNER,
        &Action::Lend {
            zone: LIBRARY,
            to: OTHER,
        },
    );
    act(
        &mut table,
        OWNER,
        &Action::Lend {
            zone: HAND,
            to: GUEST,
        },
    );
    let shuffle = Action::Shuffle {
        seat: OWNER,
        portion: Portion::Top(2),
    };
    let entry = act(&mut table, OWNER, &shuffle);
    assert_eq!(table.lent_zones, [(HAND, GUEST)]);
    undo(&mut table, &entry);
    assert_eq!(
        table.lent_zones,
        [(LIBRARY, GUEST), (LIBRARY, OTHER), (HAND, GUEST)]
    );
}

#[test]
fn always_reveal_top_keeps_each_new_top_known_to_all_even_after_shuffle() {
    let mut table = library();
    let action = Action::AlwaysRevealTop {
        seat: OWNER,
        on: true,
    };
    let entry = act(&mut table, OWNER, &action);
    assert_eq!(knowledge(&table, LIBRARY, 0), [OWNER, GUEST, OTHER]);
    assert!(knowledge(&table, LIBRARY, 1).is_empty());
    undo(&mut table, &entry);
    assert!(table.always_reveal_top.is_empty());
    assert_eq!(knowledge(&table, LIBRARY, 0), [OWNER, GUEST, OTHER]);
    act(&mut table, OWNER, &action);
    let shuffle = Action::Shuffle {
        seat: OWNER,
        portion: Portion::All,
    };
    apply(
        &mut table,
        OWNER,
        &shuffle,
        &mut Chance::replaying(vec![1, 0, 0]),
    )
    .unwrap();
    assert_eq!(names(&table, LIBRARY)[0], "Lightning Bolt");
    assert_eq!(knowledge(&table, LIBRARY, 0), [OWNER, GUEST, OTHER]);
    assert!(knowledge(&table, LIBRARY, 1).is_empty());
    act(
        &mut table,
        OWNER,
        &Action::Draw {
            seat: OWNER,
            count: 1,
        },
    );
    assert_eq!(knowledge(&table, LIBRARY, 0), [OWNER, GUEST, OTHER]);
    assert_eq!(knowledge(&table, HAND, 0), [OWNER]);
}

#[test]
fn always_look_at_top_keeps_the_owner_informed_until_switched_off() {
    let mut table = library();
    act(
        &mut table,
        OWNER,
        &Action::AlwaysLookAtTop {
            seat: OWNER,
            on: true,
        },
    );
    assert_eq!(knowledge(&table, LIBRARY, 0), [OWNER]);
    act(
        &mut table,
        OWNER,
        &Action::Draw {
            seat: OWNER,
            count: 1,
        },
    );
    assert_eq!(knowledge(&table, LIBRARY, 0), [OWNER]);
    act(
        &mut table,
        OWNER,
        &Action::AlwaysRevealTop {
            seat: OWNER,
            on: true,
        },
    );
    act(
        &mut table,
        OWNER,
        &Action::AlwaysRevealTop {
            seat: OWNER,
            on: false,
        },
    );
    assert_eq!(knowledge(&table, LIBRARY, 0), [OWNER, GUEST, OTHER]);
    act(
        &mut table,
        OWNER,
        &Action::Draw {
            seat: OWNER,
            count: 1,
        },
    );
    assert_eq!(knowledge(&table, LIBRARY, 0), [OWNER]);
    act(
        &mut table,
        OWNER,
        &Action::AlwaysLookAtTop {
            seat: OWNER,
            on: false,
        },
    );
    assert_eq!(knowledge(&table, LIBRARY, 0), [OWNER]);
    act(
        &mut table,
        OWNER,
        &Action::Draw {
            seat: OWNER,
            count: 1,
        },
    );
    assert!(knowledge(&table, LIBRARY, 0).is_empty());
}

#[test]
fn moving_a_known_card_into_a_library_keeps_every_seat_that_knew_it() {
    let mut table = library();
    seed_zone(&mut table, HAND, &[(5, "Counterspell")], &[OWNER, GUEST]);
    act(
        &mut table,
        OWNER,
        &Action::Move {
            selection: Selection {
                objects: ids(&[5]),
                zone: HAND,
            },
            to: Destination::Zone(LIBRARY),
            position: Position::Top,
            face: None,
            tapped: None,
        },
    );
    assert_eq!(table.zone(LIBRARY).unwrap(), ids(&[5, 1, 2, 3, 4]));
    assert_eq!(knowledge(&table, LIBRARY, 0), [OWNER, GUEST]);
}

#[test]
fn sort_hand_uses_card_names_printed_type_lines_and_mana_values_without_replacing_ids() {
    let mut table = Table::new(3);
    seed_zone(
        &mut table,
        HAND,
        &[
            (1, "Serra Angel"),
            (2, "Lightning Bolt"),
            (3, "Island"),
            (4, "Counterspell"),
            (5, "Fire // Ice"),
            (6, "Divination"),
        ],
        &[OWNER, GUEST],
    );
    let original = table.objects.clone();
    for (by, expected) in [
        (SortBy::Name, [4, 6, 5, 3, 2, 1]),
        (SortBy::Type, [3, 1, 2, 4, 5, 6]),
        (SortBy::ManaValue, [3, 2, 4, 6, 5, 1]),
    ] {
        let entry = act(&mut table, OWNER, &Action::SortHand { seat: OWNER, by });
        assert_eq!(table.zone(HAND).unwrap(), ids(&expected));
        assert_eq!(table.objects, original);
        undo(&mut table, &entry);
        assert_eq!(table.zone(HAND).unwrap(), ids(&[1, 2, 3, 4, 5, 6]));
    }
}

#[test]
fn public_cards_are_known_to_every_seat_unless_they_enter_face_down() {
    let mut table = library();
    for (id, face) in [(1, Face::Front), (2, Face::Down)] {
        act(
            &mut table,
            OWNER,
            &Action::Move {
                selection: Selection {
                    objects: ids(&[id]),
                    zone: LIBRARY,
                },
                to: Destination::Zone(Zone::Battlefield),
                position: Position::Bottom,
                face: Some(face),
                tapped: None,
            },
        );
    }
    assert_eq!(
        knowledge(&table, Zone::Battlefield, 0),
        [OWNER, GUEST, OTHER]
    );
    assert!(knowledge(&table, Zone::Battlefield, 1).is_empty());
}

#[test]
fn a_card_moved_to_a_sideboard_becomes_known_to_its_owner() {
    let mut table = library();
    let sideboard = Zone::Sideboard(OWNER);
    act(
        &mut table,
        OWNER,
        &Action::Move {
            selection: Selection {
                objects: ids(&[1]),
                zone: LIBRARY,
            },
            to: Destination::Zone(sideboard),
            position: Position::Top,
            face: None,
            tapped: None,
        },
    );
    assert_eq!(knowledge(&table, sideboard, 0), [OWNER]);
}

#[test]
fn an_always_reveal_setting_takes_effect_when_a_card_enters_an_empty_library() {
    let mut table = Table::new(3);
    seed_zone(&mut table, HAND, &[(1, "Island")], &[OWNER]);
    act(
        &mut table,
        OWNER,
        &Action::AlwaysRevealTop {
            seat: OWNER,
            on: true,
        },
    );
    let action =
        serde_json::from_str(r#"{"move":{"objects":[1],"in":{"hand":0},"to":{"library":0}}}"#)
            .unwrap();
    let entry = act(&mut table, OWNER, &action);
    assert_eq!(knowledge(&table, LIBRARY, 0), [OWNER, GUEST, OTHER]);
    assert!(entry.changes.contains(&Change::Learn {
        id: ObjectId(1),
        seat: GUEST
    }));
}

#[test]
fn shuffling_removes_arrows_that_refer_to_replaced_ids() {
    let mut table = library();
    table.arrows = vec![
        Arrow {
            from: ObjectId(4),
            to: ArrowTarget::Object(ObjectId(2)),
            by: OWNER,
            kind: ArrowKind::FriendlyTarget,
        },
        Arrow {
            from: ObjectId(3),
            to: ArrowTarget::Player(OWNER),
            by: OWNER,
            kind: ArrowKind::FriendlyTarget,
        },
    ];
    let before = table.arrows.clone();
    let entry = act(
        &mut table,
        OWNER,
        &Action::Shuffle {
            seat: OWNER,
            portion: Portion::Top(2),
        },
    );
    assert_eq!(table.arrows, before[1..]);
    undo(&mut table, &entry);
    assert_eq!(table.arrows, before);
}
