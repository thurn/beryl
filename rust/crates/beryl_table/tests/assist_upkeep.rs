use beryl_table::assist::{Assist, apply_proposal, enabled, upkeep::proposal};
use beryl_table::log::rebuild;
use beryl_table::state::{Card, Object, ObjectId, Phase, Zone};
use beryl_table::{Action, Chance, Change, Log, Refusal, Seat, Table, apply};

const HOST: Seat = Seat(0);
const GUEST: Seat = Seat(1);

fn switch(on: bool) -> Action {
    Action::SetAssist {
        assist: Assist::TurnUpkeep,
        on,
    }
}

fn start(first: Seat) -> (Table, Chance) {
    let mut table = Table::new(2);
    let mut chance = Chance::fresh(42);
    table.game_match.starting_hand_size = 0;
    for seat in [HOST, GUEST] {
        apply(
            &mut table,
            seat,
            &Action::LoadDeck {
                seat,
                main: vec![("Island".into(), 3)],
                side: Vec::new(),
            },
            &mut chance,
        )
        .unwrap();
    }
    apply(&mut table, HOST, &Action::Start { first }, &mut chance).unwrap();
    (table, chance)
}

#[test]
fn switches_default_off_and_belong_to_the_actor() {
    let mut table = Table::new(2);
    let mut chance = Chance::fresh(1);
    let mut log = Log::new(&table);
    assert_eq!(table.assists, [vec![], vec![]]);
    log.apply(&mut table, GUEST, &switch(true), &mut chance)
        .unwrap();
    assert!(enabled(&table, GUEST, Assist::TurnUpkeep));
    assert!(!enabled(&table, HOST, Assist::TurnUpkeep));
    assert_eq!(proposal(&table, GUEST), None);
    assert!(
        log.apply(&mut table, GUEST, &switch(true), &mut chance)
            .unwrap()
            .changes
            .is_empty()
    );
    log.apply(&mut table, GUEST, &switch(false), &mut chance)
        .unwrap();
    assert!(!enabled(&table, GUEST, Assist::TurnUpkeep));
    assert_eq!(
        apply(&mut table, Seat(2), &switch(true), &mut chance),
        Err(Refusal::INVALID)
    );
    log.apply(
        &mut table,
        GUEST,
        &Action::Undo { entry: None },
        &mut chance,
    )
    .unwrap();
    assert!(enabled(&table, GUEST, Assist::TurnUpkeep));
    assert_eq!(rebuild(&log, 4).unwrap(), table);
}

#[test]
fn only_the_active_seat_gets_upkeep_and_only_turn_one_skips_the_draw() {
    let (mut table, mut chance) = start(GUEST);
    assert_eq!(proposal(&table, GUEST), None);
    for seat in [HOST, GUEST] {
        apply(&mut table, seat, &switch(true), &mut chance).unwrap();
    }
    let first = proposal(&table, GUEST).unwrap();
    assert_eq!(first.lines.len(), 1);
    assert_eq!(
        first.lines[0].actions,
        [Action::Phase {
            phase: Phase::FirstMain
        }]
    );
    assert_eq!(proposal(&table, HOST), None);
    apply(&mut table, HOST, &Action::NextTurn {}, &mut chance).unwrap();
    assert_eq!(proposal(&table, GUEST), None);
    let second = proposal(&table, HOST).unwrap();
    assert_eq!(
        second.lines[0].actions,
        [Action::Draw {
            seat: HOST,
            count: 1
        }]
    );
}

#[test]
fn checked_upkeep_lines_apply_as_one_entry_and_either_seat_can_undo() {
    let (mut table, mut chance) = start(HOST);
    for (id, controller, flagged, tapped) in [
        (101, GUEST, false, true),
        (102, GUEST, false, true),
        (103, GUEST, true, true),
        (104, HOST, false, true),
        (105, GUEST, false, false),
    ] {
        let mut object = Object::new(
            ObjectId(id),
            Card::Named("Island".into()),
            HOST,
            Zone::Battlefield,
        );
        object.controller = controller;
        object.tapped = tapped;
        object.doesnt_untap = flagged;
        object.known_to = vec![HOST, GUEST];
        Change::Create {
            object,
            index: table.battlefield.len(),
        }
        .perform(&mut table);
    }

    let mut log = Log::new(&table);
    log.apply(&mut table, GUEST, &switch(true), &mut chance)
        .unwrap();
    log.apply(&mut table, HOST, &Action::NextTurn {}, &mut chance)
        .unwrap();
    let mut offered = proposal(&table, GUEST).unwrap();
    assert_eq!(offered.lines.len(), 4);
    assert!(
        offered
            .lines
            .iter()
            .all(|line| line.whose == GUEST && line.on)
    );
    offered.lines[1].on = false;
    let apply_entry = log
        .apply(&mut table, GUEST, &apply_proposal(&offered), &mut chance)
        .unwrap()
        .n;
    assert!(!table.objects[&ObjectId(101)].tapped);
    assert!(table.objects[&ObjectId(102)].tapped);
    assert!(table.objects[&ObjectId(103)].tapped);
    assert!(table.objects[&ObjectId(104)].tapped);
    assert_eq!(table.zone(Zone::Hand(GUEST)).unwrap().len(), 1);
    assert_eq!(table.phase, Phase::FirstMain);
    assert_eq!(proposal(&table, GUEST), None);

    let undo = log
        .apply(
            &mut table,
            HOST,
            &Action::Undo {
                entry: Some(apply_entry),
            },
            &mut chance,
        )
        .unwrap();
    assert_eq!(undo.undoes, Some(apply_entry));
    assert!(table.objects[&ObjectId(101)].tapped);
    assert!(table.zone(Zone::Hand(GUEST)).unwrap().is_empty());
    assert_eq!(table.phase, Phase::Untap);
    assert!(proposal(&table, GUEST).is_some());
    assert_eq!(rebuild(&log, apply_entry + 1).unwrap(), table);
}

#[test]
fn manual_play_withdraws_upkeep_even_if_the_phase_stays_at_untap() {
    let (mut table, mut chance) = start(HOST);
    let mut log = Log::new(&table);
    log.apply(&mut table, HOST, &switch(true), &mut chance)
        .unwrap();
    assert!(proposal(&table, HOST).is_some());
    log.apply(
        &mut table,
        HOST,
        &Action::Draw {
            seat: HOST,
            count: 1,
        },
        &mut chance,
    )
    .unwrap();
    assert_eq!(proposal(&table, HOST), None);
    log.apply(
        &mut table,
        GUEST,
        &Action::Draw {
            seat: GUEST,
            count: 1,
        },
        &mut chance,
    )
    .unwrap();
    log.apply(&mut table, HOST, &Action::Undo { entry: None }, &mut chance)
        .unwrap();
    assert_eq!(proposal(&table, HOST), None);
    assert_eq!(table.phase, Phase::Untap);
    log.apply(
        &mut table,
        GUEST,
        &Action::Undo { entry: None },
        &mut chance,
    )
    .unwrap();
    assert!(proposal(&table, HOST).is_some());
}
