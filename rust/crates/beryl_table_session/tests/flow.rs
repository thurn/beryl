use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::mpsc::{Receiver, channel};

use beryl_protocol::ipc::{
    LobbyChange, Platform, TableActReply, TableHostRequest, TableHostSettings, TableJoinRequest,
    TableLink, TableLobbyRequest, TableResumeRequest,
};
use beryl_protocol::protocol::{Answer, ObjectId, Side, TablePhase, TablePrompt};
use beryl_protocol::{GameUpdate, SessionIds};
use beryl_table::action::Adjustment;
use beryl_table::state::LobbySettings;
use beryl_table::{Action, Change, Log, Seat, Table, save};
use beryl_table_session::connections::{MemoryInvites, TableConnections};

const HOST: Seat = Seat(0);
const GUEST: Seat = Seat(1);

fn lobby(table: &TableConnections, change: LobbyChange) {
    table.lobby(TableLobbyRequest { change }).unwrap();
}

fn load(table: &TableConnections, name: &str) {
    lobby(
        table,
        LobbyChange::LoadDeck {
            main: vec![(name.into(), 15)],
            side: vec![],
        },
    );
}

fn answer(table: &TableConnections, answer: Answer) {
    assert_eq!(table.answer(answer), TableActReply::Ok { ok: true });
}

fn opening(update: &GameUpdate) -> (&[ObjectId], u16, Option<u16>, &[ObjectId]) {
    let Some(TablePrompt::Opening {
        hand,
        mulligans,
        to_bottom,
        bottom,
    }) = &update.view.table.as_ref().unwrap().prompt
    else {
        panic!("expected an opening hand");
    };
    (hand, *mulligans, *to_bottom, bottom)
}

fn receive(views: &Receiver<GameUpdate>, matches: impl Fn(&GameUpdate) -> bool) -> GameUpdate {
    views.iter().find(matches).expect("publication ended")
}

fn receive_opening(views: &Receiver<GameUpdate>, count: u16) -> GameUpdate {
    receive(views, |update| {
        matches!(
            update.view.table.as_ref().unwrap().prompt,
            Some(TablePrompt::Opening { mulligans, .. }) if mulligans == count
        )
    })
}

fn linked_table(
    dir: &Path,
) -> (
    TableConnections,
    TableConnections,
    Receiver<GameUpdate>,
    MemoryInvites,
) {
    let memory = MemoryInvites::default();
    let host = TableConnections::new(SessionIds::default(), dir.join("host"), |_| {}, |_| {})
        .for_server(memory.clone());
    let (guest, views) = guest_profile(dir, memory.clone());
    let invite = host
        .host(
            Platform::Macos,
            TableHostRequest {
                link: TableLink::Memory,
                settings: TableHostSettings {
                    name: "Host".into(),
                    starting_life: None,
                    starting_hand_size: None,
                },
            },
        )
        .unwrap()
        .invite;
    guest
        .join(TableJoinRequest {
            invite,
            name: "Guest".into(),
        })
        .unwrap();
    load(&host, "Island");
    load(&guest, "Mountain");
    lobby(&guest, LobbyChange::Ready { ready: true });
    (host, guest, views, memory)
}

fn guest_profile(dir: &Path, memory: MemoryInvites) -> (TableConnections, Receiver<GameUpdate>) {
    let (send, views) = channel();
    let guest = TableConnections::new(
        SessionIds::default(),
        dir.join("guest"),
        move |update| send.send(update).unwrap(),
        |_| {},
    )
    .for_server(memory);
    (guest, views)
}

fn host_save(host: &TableConnections) -> TableResumeRequest {
    let [saved]: [_; 1] = host.saved(Platform::Macos).unwrap().try_into().unwrap();
    TableResumeRequest { id: saved.id }
}

fn read_host_save(dir: &Path, request: &TableResumeRequest) -> (Table, Log) {
    let path = dir
        .join("host/tables")
        .join(format!("{}.jsonl", request.id));
    save::open(BufReader::new(File::open(path).unwrap())).unwrap()
}

fn reopen_table(
    dir: &Path,
    host: TableConnections,
    guest: TableConnections,
    memory: MemoryInvites,
    request: &TableResumeRequest,
) -> (TableConnections, TableConnections, Receiver<GameUpdate>) {
    host.shutdown();
    guest.shutdown();
    drop((host, guest));
    let host = TableConnections::new(SessionIds::default(), dir.join("host"), |_| {}, |_| {})
        .for_server(memory.clone());
    host.resume(Platform::Macos, request).unwrap();
    let (guest, views) = guest_profile(dir, memory);
    let guest_save = host_save(&guest);
    guest.resume(Platform::Macos, &guest_save).unwrap();
    (host, guest, views)
}

fn hand_names(update: &GameUpdate) -> Vec<&str> {
    opening(update)
        .0
        .iter()
        .map(|slot| {
            update
                .view
                .objects
                .iter()
                .find(|card| card.slot == *slot)
                .unwrap()
                .name
                .as_deref()
                .unwrap()
        })
        .collect()
}

fn gain_life(guest: &TableConnections) {
    assert_eq!(
        guest.act(Action::Life {
            seat: GUEST,
            adjustment: Adjustment::Add(3),
        }),
        TableActReply::Ok { ok: true }
    );
}

#[test]
fn guest_resumes_the_same_game_after_quitting() {
    let dir = tempfile::tempdir().unwrap();
    let (host, guest, views, memory) = linked_table(dir.path());
    assert_eq!(
        host.act(Action::Start {
            first: GUEST,
            opening: false,
        }),
        TableActReply::Ok { ok: true }
    );
    receive(&views, |update| update.view.turn.number == 1);
    gain_life(&guest);
    let before = receive(&views, |update| update.view.players.you.life == 23);
    guest.shutdown();
    drop(guest);

    let (guest, views) = guest_profile(dir.path(), memory.clone());
    // Mobile guests can resume using the same start screen command as desktop guests.
    let [saved]: [_; 1] = guest.saved(Platform::Ios).unwrap().try_into().unwrap();
    assert_eq!(saved.host, "Host");
    assert_eq!(saved.guest.as_deref(), Some("Guest"));
    let request = TableResumeRequest { id: saved.id };
    guest.resume(Platform::Ios, &request).unwrap();

    let resumed = views.recv().unwrap();
    assert_eq!(resumed.view.players.you.life, 23);
    assert_eq!(resumed.view.turn, before.view.turn);
    let resumed = resumed.view.table.unwrap();
    let before = before.view.table.unwrap();
    assert_eq!(resumed.orders.you.hand, before.orders.you.hand);
    assert_eq!(resumed.log, before.log);

    gain_life(&guest);
    assert_eq!(host.current().unwrap().view.players.opponent.life, 26);
    guest.leave().unwrap();
    drop(guest);

    let (guest, _views) = guest_profile(dir.path(), memory);
    assert!(guest.saved(Platform::Ios).unwrap().is_empty());
    assert!(guest.resume(Platform::Ios, &request).is_err());
}

#[test]
fn linked_seats_keep_independently_and_bottom_two_cards() {
    let dir = tempfile::tempdir().unwrap();
    let (host, guest, views, _) = linked_table(dir.path());
    lobby(
        &host,
        LobbyChange::Settings {
            settings: LobbySettings {
                first: Some(GUEST),
                starting_life: 20,
                starting_hand_size: 7,
            },
        },
    );
    lobby(&host, LobbyChange::Start);

    assert!(host.current().unwrap().view.table.unwrap().prompt.is_none());
    receive(&views, |update| {
        matches!(
            update.view.table.as_ref().unwrap().prompt,
            Some(TablePrompt::Choose { .. })
        )
    });
    answer(&guest, Answer::Pick { option: 1 });
    answer(&guest, Answer::Done);
    let dealt = host.current().unwrap();
    assert_eq!(opening(&dealt).0.len(), 7);
    assert_eq!(dealt.view.turn.number, 0);
    assert_eq!(dealt.view.turn.active, Side::You);
    assert!(!serde_json::to_string(&dealt).unwrap().contains("Mountain"));
    let guest_dealt = receive_opening(&views, 0);
    assert_eq!(opening(&guest_dealt).0.len(), 7);
    assert!(
        !serde_json::to_string(&guest_dealt)
            .unwrap()
            .contains("Island")
    );

    answer(&host, Answer::Keep);
    assert_eq!(host.current().unwrap().view.turn.number, 0);
    for count in 1..=2 {
        answer(&guest, Answer::Mulligan);
        let redrawn = guest.current().unwrap();
        assert_eq!(opening(&redrawn).1, count);
        assert_eq!(opening(&redrawn).0.len(), 7);
        assert_eq!(redrawn.view.turn.number, 0);
    }
    answer(&guest, Answer::Keep);
    let update = guest.current().unwrap();
    let (hand, _, to_bottom, _) = opening(&update);
    assert_eq!(to_bottom, Some(2));
    answer(&guest, Answer::Bottom { card: hand[0] });
    assert!(matches!(
        guest.answer(Answer::Done),
        TableActReply::Refused(_)
    ));
    answer(&guest, Answer::Bottom { card: hand[1] });
    answer(&guest, Answer::Done);

    let after = guest.current().unwrap().view;
    let manual = after.table.unwrap();
    assert!(manual.prompt.is_none());
    assert_eq!(manual.phase, TablePhase::Untap);
    assert_eq!(after.turn.number, 1);
    assert_eq!(after.turn.active, Side::Opponent);
    assert_eq!(manual.orders.you.hand.len(), 5);
    assert_eq!(manual.orders.opponent.hand.len(), 7);
    let last = manual.log.last().unwrap();
    assert!(last.undoable);
    assert!(last.text.contains("Put 2 cards on the bottom"));
    assert!(last.text.contains("untap"));

    assert_eq!(
        host.act(Action::Undo {
            entry: Some(last.n)
        }),
        TableActReply::Ok { ok: true }
    );
    let undone = host.current().unwrap().view;
    let manual = undone.table.unwrap();
    assert_eq!(manual.orders.opponent.hand.len(), 7);
    assert_eq!(undone.turn.number, 0);
    assert!(
        !manual
            .log
            .iter()
            .find(|line| line.n == last.n)
            .unwrap()
            .undoable
    );
    assert!(
        manual
            .log
            .last()
            .unwrap()
            .text
            .starts_with("Undid: Kept the opening hand")
    );
}

#[test]
fn a_reopened_host_asks_the_same_seat_to_play_or_draw() {
    let dir = tempfile::tempdir().unwrap();
    let (host, guest, views, memory) = linked_table(dir.path());
    lobby(&host, LobbyChange::Start);
    let before_guest = receive(&views, |update| {
        update.view.table.as_ref().unwrap().choosing_first
    });
    let before_host = host.current().unwrap();
    let request = host_save(&host);
    let (_, before_log) = read_host_save(dir.path(), &request);
    assert!(
        before_log
            .entries()
            .last()
            .unwrap()
            .changes
            .iter()
            .any(|change| { matches!(change, Change::Roll { .. }) })
    );

    let (host, guest, views) = reopen_table(dir.path(), host, guest, memory, &request);
    let after_host = host.current().unwrap();
    let after_guest = views.recv().unwrap();
    for (before, after) in [(before_host, after_host), (before_guest, after_guest)] {
        let before = before.view.table.unwrap();
        let after = after.view.table.unwrap();
        assert!(after.choosing_first);
        assert_eq!(after.prompt, before.prompt);
        assert_eq!(after.log, before.log);
    }
    let chooser = if host.current().unwrap().view.table.unwrap().prompt.is_some() {
        &host
    } else {
        &guest
    };
    answer(chooser, Answer::Pick { option: 0 });
    answer(chooser, Answer::Done);
    let (table, log) = read_host_save(dir.path(), &request);
    assert_eq!(log.entries().len(), before_log.entries().len() + 1);
    let choice = before_log.entries().last().unwrap().n;
    assert_eq!(
        log.undo_target(&table, HOST, Some(choice)),
        Err(beryl_table::Refusal::Invalid {
            blocking: vec![log.entries().last().unwrap().n],
        }),
    );
    assert_eq!(table.turn, 0);
    assert_eq!(opening(&host.current().unwrap()).1, 0);
    receive_opening(&views, 0);
}

#[test]
fn a_reopened_host_restores_each_mulligan_and_keep_through_undo_and_rewind() {
    let dir = tempfile::tempdir().unwrap();
    let (host, guest, views, memory) = linked_table(dir.path());
    lobby(
        &guest,
        LobbyChange::LoadDeck {
            main: vec![("Mountain".into(), 8), ("Plains".into(), 7)],
            side: vec![],
        },
    );
    lobby(&guest, LobbyChange::Ready { ready: true });
    lobby(
        &host,
        LobbyChange::Settings {
            settings: LobbySettings {
                first: Some(HOST),
                starting_life: 20,
                starting_hand_size: 7,
            },
        },
    );
    lobby(&host, LobbyChange::Start);
    answer(&host, Answer::Pick { option: 0 });
    answer(&host, Answer::Done);
    receive_opening(&views, 0);
    answer(&host, Answer::Keep);
    answer(&guest, Answer::Mulligan);
    let before = guest.current().unwrap();
    assert_eq!(opening(&before).1, 1);
    let request = host_save(&host);
    let (table, log) = read_host_save(dir.path(), &request);
    assert_eq!(table.turn, 0);
    let checkpoint = log.entries().last().unwrap().n;

    answer(&guest, Answer::Keep);
    answer(
        &guest,
        Answer::Bottom {
            card: opening(&before).0[0],
        },
    );
    let (host, guest, views) = reopen_table(dir.path(), host, guest, memory, &request);
    let resumed = views.recv().unwrap();
    assert_eq!(opening(&resumed).1, 1);
    assert_eq!(opening(&resumed).2, None);
    assert_eq!(hand_names(&resumed), hand_names(&before));
    assert!(host.current().unwrap().view.table.unwrap().prompt.is_none());
    assert_eq!(
        host.answer(Answer::Keep),
        TableActReply::Refused(beryl_table::Refusal::Stale.into())
    );

    answer(&guest, Answer::Keep);
    answer(
        &guest,
        Answer::Bottom {
            card: opening(&resumed).0[0],
        },
    );
    answer(&guest, Answer::Done);
    let (finished, log) = read_host_save(dir.path(), &request);
    assert_eq!(finished.turn, 1);
    assert_eq!(guest.current().unwrap().view.turn.number, 1);
    receive(&views, |update| update.view.turn.number == 1);
    let final_keep = log.entries().last().unwrap().n;
    assert_eq!(final_keep, checkpoint + 1);
    let first_keep = checkpoint - 1;
    assert_eq!(
        host.act(Action::Undo {
            entry: Some(first_keep)
        }),
        TableActReply::Refused(
            beryl_table::Refusal::Invalid {
                blocking: vec![final_keep],
            }
            .into()
        ),
    );

    for action in [
        Action::Undo {
            entry: Some(final_keep),
        },
        Action::Rewind { to: checkpoint },
    ] {
        assert_eq!(host.act(action), TableActReply::Ok { ok: true });
        let guest_reopened = receive_opening(&views, 1);
        assert_eq!(hand_names(&guest_reopened), hand_names(&before));
        let (restored, log) = read_host_save(dir.path(), &request);
        assert_eq!(restored.turn, 0);
        assert_eq!(
            restored,
            beryl_table::log::rebuild(&log, restored.revision()).unwrap()
        );
        assert!(host.current().unwrap().view.table.unwrap().prompt.is_none());
        answer(&guest, Answer::Keep);
        answer(&guest, Answer::Done);
        assert_eq!(guest.current().unwrap().view.turn.number, 1);
        receive(&views, |update| update.view.turn.number == 1);
    }
}

#[test]
fn solo_can_decline_to_bottom_cards() {
    let dir = tempfile::tempdir().unwrap();
    let solo = TableConnections::new(SessionIds::default(), dir.path().into(), |_| {}, |_| {});
    lobby(&solo, LobbyChange::Solo);
    load(&solo, "Island");
    lobby(&solo, LobbyChange::Start);
    assert_eq!(opening(&solo.current().unwrap()).1, 0);
    answer(&solo, Answer::Mulligan);
    answer(&solo, Answer::Keep);
    assert_eq!(opening(&solo.current().unwrap()).2, Some(1));
    answer(&solo, Answer::Done);
    let view = solo.current().unwrap().view;
    let manual = view.table.unwrap();
    assert!(manual.prompt.is_none());
    assert_eq!(manual.orders.you.hand.len(), 7);
    assert_eq!(view.turn.number, 1);
    assert_eq!(view.turn.active, Side::You);
}
