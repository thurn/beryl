use std::fs::{File, OpenOptions};
use std::io::{BufReader, Write};
use std::path::Path;
use std::sync::mpsc::{Receiver, SyncSender, channel, sync_channel};

use beryl_link::memory::{self, MemoryLink};
use beryl_link::wire::{Hello, Message, RefusalReason, TableStatus, decode, encode};
use beryl_link::{Link, LinkError};
use beryl_session::protocol::{LinkState, TableAction, TableView};
use beryl_session::script::{Table as Stage, failure};
use beryl_session::{GameUpdate, SessionIds, WIRE_HASH};
use beryl_table::action::{Adjustment, Destination, Position, Selection};
use beryl_table::save;
use beryl_table::state::Zone;
use beryl_table::{Action, Refusal, Seat};
use beryl_table_session::{HostSettings, TableSessions};

const HOST: Seat = Seat(0);
const GUEST: Seat = Seat(1);
const INVITE: &str = "beryl-local:52114:3mxd";
const SECRET: &str = "3mxd";

fn tables(path: &Path) -> (TableSessions, Receiver<GameUpdate>) {
    let (emit, updates) = channel();
    let tables = TableSessions::with_data_dir(SessionIds::default(), path, move |update| {
        let _ = emit.send(update);
    });
    (tables, updates)
}

fn dial(links: &SyncSender<MemoryLink>, name: &str) -> MemoryLink {
    let (host, mut guest) = memory::pair();
    links.send(host).unwrap();
    send(
        &mut guest,
        &Message::Hello(Hello {
            wire: WIRE_HASH.into(),
            secret: SECRET.into(),
            name: name.into(),
        }),
    );
    guest
}

fn send(link: &mut MemoryLink, message: &Message) {
    link.send(&encode(message)).unwrap();
}

fn receive(link: &mut MemoryLink) -> Result<Message, LinkError> {
    loop {
        let message = decode(&link.receive()?)?;
        if let Message::Ping { n } = message {
            send(link, &Message::Pong { n });
        } else {
            return Ok(message);
        }
    }
}

fn guest_update(link: &mut MemoryLink) -> GameUpdate {
    let Message::Update(payload) = receive(link).unwrap() else {
        panic!("expected the guest's view");
    };
    serde_json::from_value(payload).unwrap()
}

fn join(links: &SyncSender<MemoryLink>, status: TableStatus) -> (MemoryLink, GameUpdate) {
    let mut guest = dial(links, "Mara");
    assert_eq!(
        receive(&mut guest).unwrap(),
        Message::Welcome {
            seat: GUEST.0,
            table: status,
            host: "Derek".into()
        },
    );
    let update = guest_update(&mut guest);
    assert_eq!(manual(&update).seat, Some(GUEST.0));
    assert_eq!(
        manual(&update).link,
        LinkState::Connected {
            direct: true,
            round_trip_ms: None
        }
    );
    assert!(update.view.script.as_ref().unwrap().reset.is_some());
    (guest, update)
}

fn guest_act(guest: &mut MemoryLink, n: u64, action: &TableAction) {
    send(
        guest,
        &Message::Act {
            n,
            action: serde_json::to_value(action).unwrap(),
        },
    );
}

fn accepted_guest_action(guest: &mut MemoryLink, n: u64, action: &TableAction) -> GameUpdate {
    guest_act(guest, n, action);
    let update = guest_update(guest);
    assert_eq!(receive(guest).unwrap(), Message::Done { n });
    update
}

fn manual(update: &GameUpdate) -> &TableView {
    update.view.table.as_ref().unwrap()
}

fn load(seat: Seat, card: &str) -> TableAction {
    Action::LoadDeck {
        seat,
        main: vec![(card.into(), 12)],
        side: Vec::new(),
    }
}

fn life(seat: Seat, delta: i32) -> TableAction {
    Action::Life {
        seat,
        adjustment: Adjustment::Add(delta),
    }
}

fn assert_revision(path: &Path, revision: u32) {
    let (saved, _) = save::open(BufReader::new(File::open(path).unwrap())).unwrap();
    assert_eq!(saved.revision(), revision);
}

fn check_transition(before: &GameUpdate, after: &GameUpdate) {
    assert_eq!(before.session, after.session);
    let script = after.view.script.as_ref().unwrap();
    assert_eq!(failure(&Stage::of(&before.view), script, &after.view), None);
}

#[test]
fn both_seats_share_the_order_and_keep_their_private_views() {
    let directory = tempfile::tempdir().unwrap();
    let (tables, emitted) = tables(directory.path());
    let (links, source) = sync_channel(1);
    let session = tables
        .host(source, HostSettings::new("Derek", INVITE, SECRET))
        .unwrap();
    let alone = emitted.recv().unwrap();
    assert_eq!(alone.session, session);
    assert_eq!(manual(&alone).link, LinkState::Alone);
    assert!(tables.list_saved().unwrap()[0].guest.is_none());

    tables.act(load(HOST, "Island")).unwrap();
    emitted.recv().unwrap();
    let (mut guest, _) = join(&links, TableStatus::New);
    assert!(matches!(
        manual(&emitted.recv().unwrap()).link,
        LinkState::Connected { .. }
    ));
    assert_eq!(
        tables.list_saved().unwrap()[0].guest.as_deref(),
        Some("Mara")
    );

    guest_act(&mut guest, 1, &load(HOST, "Plains"));
    assert_eq!(
        receive(&mut guest).unwrap(),
        Message::Refused {
            n: 1,
            reason: RefusalReason::Permission
        }
    );
    assert!(emitted.try_recv().is_err());

    accepted_guest_action(&mut guest, 2, &load(GUEST, "Plains"));
    emitted.recv().unwrap();
    tables.act(Action::Start { first: HOST }).unwrap();
    let (mut host_view, mut guest_view) = (emitted.recv().unwrap(), guest_update(&mut guest));
    assert_eq!(manual(&host_view).orders.you.hand.len(), 7);
    assert_eq!(manual(&guest_view).orders.you.hand.len(), 7);
    for (update, known, hidden) in [
        (&host_view, "Island", "Plains"),
        (&guest_view, "Plains", "Island"),
    ] {
        let json = serde_json::to_string(update).unwrap();
        assert!(json.contains(known));
        assert!(!json.contains(hidden));
    }

    let action = Action::Batch {
        actions: vec![life(GUEST, -1)],
    };
    let next_guest = accepted_guest_action(&mut guest, 3, &action);
    let next_host = emitted.recv().unwrap();
    check_transition(&host_view, &next_host);
    check_transition(&guest_view, &next_guest);
    let batch = manual(&next_guest).log.last().unwrap().n;
    assert_eq!(manual(&next_host).log.last().unwrap().by, "Mara");
    assert_eq!(manual(&next_guest).log.last().unwrap().by, "You");
    (host_view, guest_view) = (next_host, next_guest);

    for delta in [-2, -3] {
        tables.act(life(GUEST, delta)).unwrap();
        let (next_host, next_guest) = (emitted.recv().unwrap(), guest_update(&mut guest));
        check_transition(&host_view, &next_host);
        check_transition(&guest_view, &next_guest);
        let host_line = manual(&next_host).log.last().unwrap();
        let guest_line = manual(&next_guest).log.last().unwrap();
        assert_eq!(host_line.n, guest_line.n);
        assert_eq!(host_line.by, "You");
        assert_eq!(guest_line.by, "Derek");
        (host_view, guest_view) = (next_host, next_guest);
    }

    guest_act(&mut guest, 4, &Action::Undo { entry: Some(batch) });
    assert_eq!(
        receive(&mut guest).unwrap(),
        Message::Refused {
            n: 4,
            reason: RefusalReason::Invalid {
                blocking: vec![batch + 1, batch + 2]
            }
        },
    );
    assert!(emitted.try_recv().is_err());
    assert_eq!(
        manual(&tables.current().unwrap()).log,
        manual(&host_view).log
    );
    assert_eq!(host_view.view.players.opponent.life, 14);
    assert_eq!(guest_view.view.players.you.life, 14);

    let id = &tables.list_saved().unwrap()[0].id;
    let file = File::open(directory.path().join("tables").join(format!("{id}.jsonl"))).unwrap();
    let (saved, log) = save::open(BufReader::new(file)).unwrap();
    assert_eq!(
        log.entries()
            .iter()
            .map(|entry| entry.by)
            .collect::<Vec<_>>(),
        [HOST, GUEST, HOST, GUEST, HOST, HOST]
    );
    assert_eq!(saved.players[1].life, 14);
    tables.leave();
    assert_eq!(receive(&mut guest), Ok(Message::Leave));
    assert!(tables.current().is_none());
}

#[test]
fn a_drop_freezes_actions_and_resume_replays_the_save_for_the_same_guest() {
    let directory = tempfile::tempdir().unwrap();
    let (host, emitted) = tables(directory.path());
    let (links, source) = sync_channel(1);
    let mut settings = HostSettings::new("Derek", INVITE, SECRET);
    settings.starting_life = 27;
    settings.starting_hand_size = 5;
    host.host(source, settings).unwrap();
    emitted.recv().unwrap();
    host.act(load(HOST, "Island")).unwrap();
    emitted.recv().unwrap();
    let (mut guest, _) = join(&links, TableStatus::New);
    emitted.recv().unwrap();
    accepted_guest_action(&mut guest, 1, &load(GUEST, "Plains"));
    emitted.recv().unwrap();
    host.act(Action::Start { first: GUEST }).unwrap();
    let before = emitted.recv().unwrap();
    let guest_before = guest_update(&mut guest);
    assert_eq!(manual(&before).orders.you.hand.len(), 5);
    assert_eq!(guest_before.view.players.you.life, 27);
    let old_reference = {
        let hand = manual(&guest_before).orders.you.hand[0];
        manual(&guest_before)
            .object_actions
            .iter()
            .find(|action| action.subject == beryl_session::script::Subject::Object { slot: hand })
            .unwrap()
            .reference
            .clone()
    };

    guest.sever();
    let frozen = emitted.recv().unwrap();
    assert_eq!(manual(&frozen).link, LinkState::Reconnecting);
    assert_eq!(host.act(life(HOST, -1)), Err(Refusal::Stale));
    assert!(emitted.try_recv().is_err());
    assert_eq!(manual(&frozen).log, manual(&before).log);

    assert_eq!(
        receive(&mut dial(&links, "Someone else")),
        Err(LinkError::Closed)
    );
    let (mut guest, redialed) = join(&links, TableStatus::Resumed);
    emitted.recv().unwrap();
    assert!(Stage::of(&redialed.view).shows(&Stage::of(&guest_before.view)));
    guest_act(
        &mut guest,
        2,
        &Action::Move {
            selection: Selection {
                objects: vec![old_reference],
                zone: Zone::Hand(GUEST),
            },
            to: Destination::Zone(Zone::Battlefield),
            position: Position::Top,
            face: None,
            tapped: None,
        },
    );
    assert_eq!(
        receive(&mut guest).unwrap(),
        Message::Refused {
            n: 2,
            reason: RefusalReason::Stale
        }
    );
    assert!(emitted.try_recv().is_err());

    let id = host.list_saved().unwrap().pop().unwrap().id;
    assert_eq!(host.saved_invite(&id).unwrap(), INVITE);
    drop(host);
    assert_eq!(receive(&mut guest), Err(LinkError::Closed));
    let path = directory.path().join("tables").join(format!("{id}.jsonl"));
    OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"{\"n\":")
        .unwrap();

    let (host, emitted) = tables(directory.path());
    let (links, source) = sync_channel(1);
    host.resume(&id, source).unwrap();
    let reopened = emitted.recv().unwrap();
    assert_eq!(manual(&reopened).link, LinkState::Reconnecting);
    assert!(reopened.view.script.as_ref().unwrap().reset.is_some());
    assert!(Stage::of(&reopened.view).shows(&Stage::of(&before.view)));
    assert_eq!(manual(&reopened).log, manual(&before).log);
    assert_eq!(host.act(life(HOST, -1)), Err(Refusal::Stale));

    assert_eq!(
        receive(&mut dial(&links, "Someone else")),
        Err(LinkError::Closed)
    );
    let (mut guest, resumed) = join(&links, TableStatus::Resumed);
    emitted.recv().unwrap();
    assert!(Stage::of(&resumed.view).shows(&Stage::of(&guest_before.view)));
    host.act(life(HOST, -1)).unwrap();
    assert_eq!(guest_update(&mut guest).view.players.opponent.life, 26);
    emitted.recv().unwrap();
    assert_revision(&path, 4);

    send(&mut guest, &Message::Leave);
    let left = emitted.recv().unwrap();
    assert_eq!(manual(&left).link, LinkState::Left);
    assert_eq!(host.act(life(HOST, -1)), Err(Refusal::Stale));
}

#[test]
fn overlapping_local_and_guest_actions_publish_the_same_order() {
    let directory = tempfile::tempdir().unwrap();
    let (host, emitted) = tables(directory.path());
    let (links, source) = sync_channel(1);
    host.host(source, HostSettings::new("Derek", INVITE, SECRET))
        .unwrap();
    emitted.recv().unwrap();
    let (mut guest, _) = join(&links, TableStatus::New);
    emitted.recv().unwrap();

    // Submit both mutations before waiting for the guest's reply. Either may
    // enter the host queue first, and both viewers must see that same order.
    guest_act(&mut guest, 1, &life(HOST, -1));
    host.act(life(HOST, -2)).unwrap();
    let local_life = [(); 2].map(|()| emitted.recv().unwrap().view.players.you.life);
    let mut peer_life = Vec::new();
    let mut done = false;
    for _ in 0..3 {
        match receive(&mut guest).unwrap() {
            Message::Update(payload) => {
                let update: GameUpdate = serde_json::from_value(payload).unwrap();
                peer_life.push(update.view.players.opponent.life);
            }
            Message::Done { n: 1 } => {
                assert!(!peer_life.is_empty());
                done = true;
            }
            message => panic!("unexpected message: {message:?}"),
        }
    }
    assert!(done);
    assert_eq!(local_life.as_slice(), peer_life);
    assert_eq!(local_life[1], 17);
}
