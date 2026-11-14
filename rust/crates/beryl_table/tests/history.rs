use std::collections::BTreeSet;

use beryl_table::action::{
    Adjustment, ComputedDestination, Destination, Portion, Position, Selection,
};
use beryl_table::log::{ConnectionEvent, connection_line, line, rebuild};
use beryl_table::redact::{entry_seen_by, seen_by};
use beryl_table::save::{self, SaveError};
use beryl_table::state::{Card, Object, ObjectId, Phase, Zone};
use beryl_table::{Action, Chance, Change, Entry, Log, Refusal, Seat, Table};

mod common;
use common::random::{ASSIST_AND_LOG, CORE, Generator, Kind};

const HOST: Seat = Seat(0);
const GUEST: Seat = Seat(1);
const LIBRARY: Zone = Zone::Library(HOST);
const HAND: Zone = Zone::Hand(HOST);
const BATTLEFIELD: Zone = Zone::Battlefield;
const UNDO: Action = Action::Undo { entry: None };

struct Game {
    table: Table,
    log: Log,
    chance: Chance,
    file: Vec<u8>,
}

impl Game {
    fn new(table: Table) -> Self {
        let mut file = Vec::new();
        save::write_header(&mut file, &table, 17).unwrap();
        Self {
            log: Log::new(&table),
            table,
            chance: Chance::fresh(17),
            file,
        }
    }

    fn act(&mut self, by: Seat, action: &Action) -> Entry {
        let entry = self
            .log
            .apply(&mut self.table, by, action, &mut self.chance)
            .unwrap_or_else(|error| panic!("{action:?}: {error:?}"));
        save::append(&mut self.file, entry).unwrap();
        entry.clone()
    }

    fn line(&mut self, by: Seat, action: &Action, viewer: Seat) -> String {
        let before = self.table.clone();
        let entry = self.act(by, action);
        line(
            &entry_seen_by(&entry, &before, viewer),
            &seen_by(&self.table, viewer),
            viewer,
        )
    }

    fn assert_reopens(&self) {
        let (table, log, chance) = save::open_with_chance(&self.file[..]).unwrap();
        assert_eq!(self.table, table);
        assert_eq!(self.log.entries(), log.entries());
        assert_eq!(self.chance, chance);
        for entry in log.entries() {
            assert_eq!(self.log.is_struck(entry.n), log.is_struck(entry.n));
        }
    }
}

fn add(table: &mut Table, id: u32, name: &str, zone: Zone, known_to: &[Seat]) {
    let mut object = Object::new(ObjectId(id), Card::Named(name.into()), HOST, zone);
    object.known_to = known_to.to_vec();
    Change::Create {
        object,
        index: table.zone(zone).unwrap().len(),
    }
    .perform(table);
}

fn fixtures() -> Table {
    let mut table = Table::new(2);
    add(
        &mut table,
        1,
        "Underground Sea",
        BATTLEFIELD,
        &[HOST, GUEST],
    );
    add(&mut table, 2, "Brainstorm", LIBRARY, &[]);
    add(&mut table, 3, "Island", LIBRARY, &[]);
    add(&mut table, 4, "Plains", LIBRARY, &[]);
    table
}

fn tap(id: u32, tapped: bool) -> Action {
    Action::Tap {
        selection: Selection {
            objects: vec![ObjectId(id)],
            zone: BATTLEFIELD,
        },
        tapped,
    }
}

fn draw(count: u16) -> Action {
    Action::Draw { seat: HOST, count }
}

fn life(seat: Seat, delta: i32) -> Action {
    Action::Life {
        seat,
        adjustment: Adjustment::Add(delta),
    }
}

fn move_card(id: u32, from: Zone, to: Zone, position: Position) -> Action {
    Action::Move {
        selection: Selection {
            zone: from,
            objects: vec![ObjectId(id)],
        },
        to: Destination::Zone(to),
        position,
        face: None,
        tapped: None,
    }
}

#[test]
fn undoing_a_tap_restores_the_original_card() {
    let mut game = Game::new(fixtures());
    let entry = game.act(HOST, &tap(1, true));
    let undo = game.act(HOST, &UNDO);
    assert_eq!(undo.undoes, Some(entry.n));
    assert!(!game.table.objects[&ObjectId(1)].tapped);
    assert_eq!(game.table.battlefield, [ObjectId(1)]);
    game.assert_reopens();
}

#[test]
fn undoing_a_draw_keeps_the_other_seats_tap_and_the_drawers_knowledge() {
    let mut game = Game::new(fixtures());
    assert_eq!(game.line(HOST, &draw(3), HOST), "Drew 3 cards.");
    assert_eq!(
        game.line(GUEST, &tap(1, true), HOST),
        "Tapped Underground Sea."
    );
    assert_eq!(game.line(HOST, &UNDO, HOST), "Undid: Drew 3 cards (seen).");
    assert_eq!(
        game.table.zone(LIBRARY).unwrap(),
        [ObjectId(2), ObjectId(3), ObjectId(4)]
    );
    assert!(game.table.zone(HAND).unwrap().is_empty());
    for id in [2, 3, 4] {
        assert_eq!(game.table.objects[&ObjectId(id)].known_to, [HOST]);
    }
    assert!(game.table.objects[&ObjectId(1)].tapped);
    game.assert_reopens();
}

#[test]
fn later_overlapping_entries_block_undo_until_they_are_undone() {
    let mut game = Game::new(fixtures());
    game.act(HOST, &tap(1, true));
    let first = game.act(GUEST, &tap(1, false));
    let second = game.act(GUEST, &tap(1, true));
    let before = game.table.clone();
    assert_eq!(
        game.log
            .apply(&mut game.table, HOST, &UNDO, &mut game.chance),
        Err(Refusal::Invalid {
            blocking: vec![first.n, second.n]
        })
    );
    assert_eq!(game.table, before);
    game.act(GUEST, &UNDO);
    game.act(GUEST, &UNDO);
    game.act(HOST, &UNDO);
    assert!(!game.table.objects[&ObjectId(1)].tapped);
    game.assert_reopens();
}

#[test]
fn a_later_insertion_can_block_undo_by_shifting_a_zone_position() {
    let mut initial = fixtures();
    add(
        &mut initial,
        5,
        "Mountain",
        Zone::Graveyard(HOST),
        &[HOST, GUEST],
    );
    let mut game = Game::new(initial);
    let move_first = move_card(1, BATTLEFIELD, Zone::Graveyard(HOST), Position::Bottom);
    let move_second = move_card(5, Zone::Graveyard(HOST), BATTLEFIELD, Position::Top);
    let first = game.act(HOST, &move_first);
    let second = game.act(GUEST, &move_second);
    assert!(first.touches.objects.is_disjoint(&second.touches.objects));
    assert!(
        !first
            .touches
            .positions
            .is_disjoint(&second.touches.positions)
    );
    assert_eq!(
        game.log
            .apply(&mut game.table, HOST, &UNDO, &mut game.chance),
        Err(Refusal::Invalid {
            blocking: vec![second.n]
        })
    );
}

#[test]
fn undo_blocks_when_a_later_removal_would_leave_its_insertion_index_invalid() {
    let mut initial = fixtures();
    add(&mut initial, 5, "Mountain", BATTLEFIELD, &[HOST, GUEST]);
    let mut game = Game::new(initial);
    game.act(
        HOST,
        &move_card(5, BATTLEFIELD, Zone::Graveyard(HOST), Position::Top),
    );
    let blocker = game.act(
        GUEST,
        &move_card(1, BATTLEFIELD, Zone::Graveyard(GUEST), Position::Top),
    );
    assert_eq!(
        game.log
            .apply(&mut game.table, HOST, &UNDO, &mut game.chance),
        Err(Refusal::Invalid {
            blocking: vec![blocker.n]
        })
    );
}

#[test]
fn a_later_edit_to_the_same_life_total_blocks_undo() {
    let mut game = Game::new(fixtures());
    game.act(HOST, &life(HOST, -2));
    game.act(GUEST, &life(GUEST, -1));
    let blocker = game.act(GUEST, &life(HOST, -3));
    assert_eq!(
        game.log
            .apply(&mut game.table, HOST, &UNDO, &mut game.chance),
        Err(Refusal::Invalid {
            blocking: vec![blocker.n]
        })
    );
    game.act(GUEST, &UNDO);
    game.act(HOST, &UNDO);
    assert_eq!(game.table.players[0].life, 20);
    assert_eq!(game.table.players[1].life, 19);
}

#[test]
fn undoing_a_shuffle_restores_card_ids_order_and_previous_knowledge() {
    let mut game = Game::new(fixtures());
    game.act(
        HOST,
        &Action::Look {
            seat: HOST,
            portion: Portion::Top(1),
        },
    );
    let before = game.table.clone();
    game.act(
        HOST,
        &Action::Shuffle {
            seat: HOST,
            portion: Portion::All,
        },
    );
    assert_ne!(game.table.zone(LIBRARY), before.zone(LIBRARY));
    game.act(HOST, &UNDO);
    assert_eq!(game.table.objects, before.objects);
    assert_eq!(game.table.zone(LIBRARY), before.zone(LIBRARY));
    game.assert_reopens();
}

#[test]
fn rewinding_a_shuffle_and_undoing_the_rewind_restore_both_branches() {
    let mut game = Game::new(fixtures());
    let look = game.act(
        HOST,
        &Action::Look {
            seat: HOST,
            portion: Portion::Top(1),
        },
    );
    let looked = game.table.clone();
    let shuffle = game.act(
        HOST,
        &Action::Shuffle {
            seat: HOST,
            portion: Portion::All,
        },
    );
    game.act(HOST, &Action::NextTurn {});
    let shuffled = game.table.clone();
    let rewind = game.act(GUEST, &Action::Rewind { to: look.n });
    assert_eq!(game.table.zone(LIBRARY), looked.zone(LIBRARY));
    assert_eq!(game.table.objects, looked.objects);
    assert!(game.log.is_struck(shuffle.n));
    assert!(game.log.is_struck(shuffle.n + 1));
    assert!(!game.log.is_struck(look.n));
    assert!(!game.log.is_struck(rewind.n));
    assert_eq!(rebuild(&game.log, shuffle.n + 1).unwrap(), shuffled);
    game.assert_reopens();

    let undo = game.act(GUEST, &UNDO);
    assert_eq!(undo.undoes, Some(rewind.n));
    assert_eq!(game.table.zone(LIBRARY), shuffled.zone(LIBRARY));
    assert_eq!(game.table.turn, shuffled.turn);
    assert!(!game.log.is_struck(shuffle.n));
    assert!(!game.log.is_struck(shuffle.n + 1));
    let top_before_shuffle = game
        .table
        .objects
        .values()
        .find(|card| card.card == Card::Named("Brainstorm".into()))
        .unwrap();
    assert_eq!(top_before_shuffle.known_to, [HOST]);
    assert_eq!(rebuild(&game.log, undo.n).unwrap(), game.table);
    game.assert_reopens();
}

#[test]
fn rewind_keeps_knowledge_of_cards_removed_at_the_end_of_the_game() {
    let mut initial = fixtures();
    initial.game_match.in_game = true;
    let mut game = Game::new(initial);
    game.act(HOST, &draw(1));
    game.act(HOST, &Action::NextGame { winner: None });
    assert!(game.table.objects.is_empty());

    game.act(GUEST, &Action::Rewind { to: 0 });
    let card = &game.table.objects[&ObjectId(2)];
    assert_eq!(card.zone, LIBRARY);
    assert_eq!(card.known_to, [HOST]);
    game.assert_reopens();
}

#[test]
fn rewinding_into_a_struck_branch_reactivates_its_own_prefix() {
    let mut game = Game::new(fixtures());
    game.act(HOST, &life(HOST, -1));
    let second = game.act(HOST, &life(HOST, -2));
    let first_rewind = game.act(GUEST, &Action::Rewind { to: 0 });
    assert_eq!(game.table.players[0].life, 20);
    let second_rewind = game.act(GUEST, &Action::Rewind { to: second.n });
    assert_eq!(game.table.players[0].life, 17);
    assert!(!game.log.is_struck(second.n));
    assert!(game.log.is_struck(first_rewind.n));
    game.act(GUEST, &UNDO);
    assert_eq!(game.table.players[0].life, 20);
    assert!(game.log.is_struck(second.n));
    assert!(!game.log.is_struck(first_rewind.n));
    assert!(!game.log.is_struck(second_rewind.n));
    game.assert_reopens();
}

#[test]
fn undoing_a_roll_discards_its_result_and_does_not_reuse_it() {
    let mut game = Game::new(fixtures());
    let roll = Action::Roll {
        count: 4,
        sides: 1_000_000,
    };
    let first = game.act(HOST, &roll);
    let undo = game.act(HOST, &UNDO);
    assert_eq!(undo.undoes, Some(first.n));
    assert!(undo.changes.is_empty());
    let second = game.act(HOST, &roll);
    assert_ne!(first.changes, second.changes);
    game.assert_reopens();
}

#[test]
fn a_seat_can_name_its_last_draw_after_an_unrelated_play() {
    let mut game = Game::new(fixtures());
    let drawn = game.act(HOST, &draw(1));
    let life_change = game.act(HOST, &life(HOST, 2));

    for (by, entry) in [(GUEST, drawn.n), (HOST, life_change.n)] {
        assert_eq!(
            game.log.apply(
                &mut game.table,
                by,
                &Action::Undo { entry: Some(entry) },
                &mut game.chance,
            ),
            Err(Refusal::INVALID),
        );
    }

    let undo = game.act(
        HOST,
        &Action::Undo {
            entry: Some(drawn.n),
        },
    );
    assert_eq!(undo.undoes, Some(drawn.n));
    assert!(undo.seen);
    assert_eq!(game.table.players[0].life, 22);
    assert!(game.table.zone(HAND).unwrap().is_empty());
    assert_eq!(game.table.zone(LIBRARY).unwrap()[0], ObjectId(2));
    game.assert_reopens();
}

#[test]
fn a_batch_uses_the_state_left_by_each_member_and_either_seat_can_reverse_it() {
    let mut game = Game::new(fixtures());
    let batch = game.act(
        HOST,
        &Action::Batch {
            actions: vec![
                move_card(2, LIBRARY, BATTLEFIELD, Position::Top),
                tap(2, true),
                Action::Phase {
                    phase: Phase::DeclareAttackers,
                },
            ],
        },
    );
    assert_eq!(game.log.entries().len(), 1);
    assert!(game.table.objects[&ObjectId(2)].tapped);
    assert_eq!(game.table.phase, Phase::DeclareAttackers);
    game.act(GUEST, &life(GUEST, -2));
    let undo = game.act(
        GUEST,
        &Action::Undo {
            entry: Some(batch.n),
        },
    );
    assert_eq!(undo.undoes, Some(batch.n));
    assert_eq!(game.table.players[1].life, 18);
    assert_eq!(game.table.phase, Phase::Untap);
    assert_eq!(game.table.zone(LIBRARY).unwrap()[0], ObjectId(2));
    assert!(!game.table.objects[&ObjectId(2)].tapped);
    assert_eq!(game.table.objects[&ObjectId(2)].known_to, [HOST, GUEST]);
    game.assert_reopens();
}

#[test]
fn an_invalid_batch_leaves_the_table_history_and_chance_unchanged() {
    let mut game = Game::new(fixtures());
    let before = game.table.clone();
    let chance = game.chance.clone();
    let batch = Action::Batch {
        actions: vec![
            Action::Roll { count: 1, sides: 6 },
            tap(1, true),
            draw(1),
            tap(1234, true),
        ],
    };
    assert!(
        game.log
            .apply(&mut game.table, HOST, &batch, &mut game.chance)
            .is_err()
    );
    assert_eq!(game.table, before);
    assert_eq!(game.chance, chance);
    assert!(game.log.entries().is_empty());
}

#[test]
fn log_lines_use_the_documented_wording_and_hide_unknown_cards() {
    let mut initial = fixtures();
    add(&mut initial, 5, "Brainstorm", HAND, &[HOST]);
    let mut game = Game::new(initial);
    let cast = Action::Move {
        selection: Selection {
            zone: HAND,
            objects: vec![ObjectId(5)],
        },
        to: Destination::Computed(ComputedDestination::Play),
        position: Position::Top,
        face: None,
        tapped: None,
    };
    assert_eq!(game.line(HOST, &cast, HOST), "Cast Brainstorm.");
    game.act(HOST, &draw(3));
    assert_eq!(
        game.line(HOST, &draw(1), GUEST),
        "Drew from an empty library."
    );
    let ids = game.table.zone(HAND).unwrap()[..2].to_vec();
    let put = Action::Move {
        selection: Selection {
            zone: HAND,
            objects: ids,
        },
        to: Destination::Zone(LIBRARY),
        position: Position::Top,
        face: None,
        tapped: None,
    };
    assert_eq!(
        game.line(HOST, &put, HOST),
        "Put 2 cards from your hand on top of your library."
    );
    assert_eq!(
        game.line(HOST, &UNDO, HOST),
        "Undid: Put 2 cards from your hand on top of your library."
    );
    let secret = "A private card";
    let mut initial = fixtures();
    add(&mut initial, 5, secret, HAND, &[HOST]);
    let mut game = Game::new(initial);
    let before = game.table.clone();
    let face_down = Action::Move {
        selection: Selection {
            zone: HAND,
            objects: vec![ObjectId(5)],
        },
        to: Destination::Computed(ComputedDestination::Play),
        position: Position::Top,
        face: Some(beryl_table::state::Face::Down),
        tapped: None,
    };
    let entry = game.act(HOST, &face_down);
    let projected = entry_seen_by(&entry, &before, GUEST);
    let guest = seen_by(&game.table, GUEST);
    assert!(!line(&projected, &guest, GUEST).contains(secret));
    let before = game.table.clone();
    let rewind = game.act(HOST, &Action::Rewind { to: 0 });
    assert!(
        !serde_json::to_string(&entry_seen_by(&rewind, &before, GUEST))
            .unwrap()
            .contains(secret)
    );
}

#[test]
fn connection_lines_cover_each_lifecycle_event() {
    assert_eq!(connection_line(ConnectionEvent::Joined), "Joined the game.");
    assert_eq!(connection_line(ConnectionEvent::Left), "Left the game.");
    assert_eq!(connection_line(ConnectionEvent::Lost), "Lost connection.");
    assert_eq!(
        connection_line(ConnectionEvent::Restored),
        "Restored connection."
    );
}

#[test]
fn a_save_rejects_new_formats_and_recovers_a_partial_append() {
    assert!(matches!(
        save::open(&b"{\"format\":2}\n"[..]),
        Err(SaveError::UnsupportedFormat(2))
    ));
    let mut game = Game::new(fixtures());
    game.act(HOST, &tap(1, true));
    game.file.extend_from_slice(b"{\"n\":2");
    game.assert_reopens();
}

#[test]
fn a_recovered_save_can_be_appended_to_and_opened_again() {
    use std::fs::OpenOptions;
    use std::io::Write;

    let path =
        std::env::temp_dir().join(format!("beryl-table-recovery-{}.jsonl", std::process::id()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    let mut game = Game::new(fixtures());
    game.act(HOST, &tap(1, true));
    file.write_all(&game.file).unwrap();
    file.write_all(b"{\"n\":2,\"action\":\"\xc3").unwrap();
    let (mut table, mut log, mut chance) = save::open_for_append(&mut file).unwrap();
    assert_eq!(table, game.table);
    let entry = log.apply(&mut table, HOST, &UNDO, &mut chance).unwrap();
    save::append(&mut file, entry).unwrap();
    let reopened = save::open_for_append(&mut file).unwrap();
    drop(file);
    std::fs::remove_file(path).unwrap();
    assert_eq!(reopened.0, table);
    assert_eq!(reopened.1.entries(), log.entries());
    assert_eq!(reopened.2, chance);
}

fn assert_restored(before: &Table, after: &Table) {
    for (id, object) in &before.objects {
        assert!(
            object
                .known_to
                .iter()
                .all(|seat| after.objects[id].known_to.contains(seat)),
            "lost knowledge of {id:?}"
        );
    }
    let mut actual = serde_json::to_value(after).unwrap();
    let expected = serde_json::to_value(before).unwrap();
    actual["next_id"] = expected["next_id"].clone();
    actual["entry_number"] = expected["entry_number"].clone();
    for (id, object) in &before.objects {
        actual["objects"][id.0.to_string()]["known_to"] =
            serde_json::to_value(&object.known_to).unwrap();
    }
    assert_eq!(actual, expected);
}

#[test]
fn generated_actions_undo_without_losing_knowledge_and_reopen_from_json() {
    let mut generator = Generator::new(2011);
    let mut covered = BTreeSet::new();
    // The library flow is implemented separately. Undo and rewind are
    // applied explicitly below.
    let kinds: Vec<_> = CORE
        .iter()
        .chain(ASSIST_AND_LOG)
        .copied()
        .filter(|kind| !matches!(kind, Kind::Flow | Kind::Undo | Kind::Rewind))
        .collect();
    let mut game = Game::new(fixtures());
    for index in 0..1_024 {
        let (kind, by, action) = generator.next(&game.table, &kinds);
        let before = game.table.clone();
        let Ok(entry) = game
            .log
            .apply(&mut game.table, by, &action, &mut game.chance)
        else {
            continue;
        };
        let entry = entry.clone();
        save::append(&mut game.file, &entry).unwrap();
        let projected = entry_seen_by(&entry, &before, GUEST);
        assert!(!line(&projected, &seen_by(&game.table, GUEST), GUEST).is_empty());
        if covered.insert(kind) || index % 4 == 0 {
            let undo = game.act(by, &UNDO);
            assert_eq!(undo.undoes, Some(entry.n));
            assert_restored(&before, &game.table);
        }
        if index % 128 == 0 {
            let before_rewind = game.table.clone();
            let rewind = game.act(
                by,
                &Action::Rewind {
                    to: entry.n.saturating_sub(5),
                },
            );
            let undo = game.act(by, &UNDO);
            assert_eq!(undo.undoes, Some(rewind.n));
            assert_restored(&before_rewind, &game.table);
        }
    }
    let missing: Vec<_> = kinds
        .iter()
        .filter(|kind| !covered.contains(kind))
        .collect();
    assert!(missing.is_empty(), "missing {missing:?}");
    game.assert_reopens();
}
