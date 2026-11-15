use std::convert::Infallible;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel, sync_channel};
use std::thread;

use beryl_link::memory;
use beryl_link::wire::{Hello, Message, TableStatus, VERSION_MISMATCH, decode, encode};
use beryl_link::{Link, LinkError, PING_INTERVAL};
use beryl_session::ipc::LobbyUpdate;
use beryl_session::protocol::{LinkState, TableAction, TableObjectRef, ZoneName};
use beryl_session::script::{Act, Span, Subject, Table as Stage, failure};
use beryl_session::{GameUpdate, SessionId, SessionIds};
use beryl_table::action::{
    Adjustment, ComputedDestination, Destination, Position, SeenObject, Selection, Target,
};
use beryl_table::assist::{apply_proposal, combat};
use beryl_table::state::{ArrowKind, Assist, Phase, Zone};
use beryl_table::{Action, Log, Refusal, Seat, Table, save};
use beryl_table_session::publication::SeatPublication;
use beryl_table_session::{HostSettings, TableEvent, TableSessions};

const HOST: Seat = Seat(0);
const GUEST: Seat = Seat(1);
const SECRET: &str = "3mxd";
const INVITE: &str = "beryl-local:52114:3mxd";
const PRIVATE_CARD: &str = "Ancestral Recall";

fn watch(
    path: &Path,
    ids: SessionIds,
) -> (TableSessions, Receiver<GameUpdate>, Receiver<TableEvent>) {
    let (emit, updates) = channel();
    let (event, events) = channel();
    let tables = TableSessions::with_data_dir(ids, path, move |update| {
        let _ = emit.send(update);
    })
    .with_events(move |value| {
        let _ = event.send(value);
    });
    (tables, updates, events)
}

fn load(seat: Seat, names: [&str; 3]) -> TableAction {
    Action::LoadDeck {
        seat,
        main: names.into_iter().map(|name| (name.into(), 1)).collect(),
        side: Vec::new(),
    }
}

fn reference(view: &GameUpdate, name: &str, zone: ZoneName) -> TableObjectRef {
    let slot = view
        .view
        .objects
        .iter()
        .find(|card| card.zone == zone && card.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("cannot find {name} in {zone:?}"))
        .slot;
    view.view
        .table
        .as_ref()
        .unwrap()
        .object_actions
        .iter()
        .find(|action| action.subject == Subject::Object { slot })
        .unwrap()
        .reference
        .clone()
}

fn check_update(before: Option<&GameUpdate>, after: &GameUpdate) {
    if let Some(script) = &after.view.script {
        let stage = before.map_or_else(|| Stage::of(&after.view), |update| Stage::of(&update.view));
        assert_eq!(failure(&stage, script, &after.view), None);
    }
}

fn replay(path: &Path) -> (Table, Log) {
    save::open(BufReader::new(File::open(path).unwrap())).unwrap()
}

struct Game {
    host: TableSessions,
    guest: TableSessions,
    updates: [Receiver<GameUpdate>; 2],
    views: [GameUpdate; 2],
    path: PathBuf,
}

impl Game {
    fn check_privacy(&self) {
        let (table, _) = replay(&self.path);
        let guest = serde_json::to_string(&self.views[1]).unwrap();
        assert!(!guest.contains(PRIVATE_CARD));
        for card in table
            .objects
            .values()
            .filter(|card| !card.known_to.contains(&GUEST))
        {
            // This deck has no duplicate names, so a known copy cannot account for a match.
            assert!(!guest.contains(card.card.name().unwrap()));
        }
    }

    fn accept(&mut self, actor: Seat, action: TableAction) {
        [&self.host, &self.guest][usize::from(actor.0)]
            .act(action)
            .unwrap();
        let next = self
            .updates
            .each_ref()
            .map(|updates| updates.recv().unwrap());
        for (before, after) in self.views.iter().zip(&next) {
            assert_eq!(before.session, after.session);
            check_update(Some(before), after);
        }
        self.views = next;
        self.check_privacy();
    }

    fn move_card(
        &mut self,
        seat: Seat,
        name: &str,
        from: Zone,
        zone: ZoneName,
        to: ComputedDestination,
    ) {
        let view = &self.views[usize::from(seat.0)];
        self.accept(
            seat,
            Action::Move {
                selection: Selection {
                    objects: vec![reference(view, name, zone)],
                    zone: from,
                },
                to: Destination::Computed(to),
                position: Position::Top,
                face: None,
                tapped: None,
            },
        );
    }

    fn arrow(&mut self, seat: Seat, from: &str, to: Target<TableObjectRef>) {
        let view = &self.views[usize::from(seat.0)];
        self.accept(
            seat,
            Action::Arrow {
                selection: Selection {
                    objects: vec![reference(view, from, ZoneName::Battlefield)],
                    zone: Zone::Battlefield,
                },
                to,
            },
        );
    }

    fn opening(&mut self) {
        self.accept(HOST, load(HOST, ["Forest", "Grizzly Bears", PRIVATE_CARD]));
        self.accept(GUEST, load(GUEST, ["Plains", "Runeclaw Bear", "Opt"]));
        self.accept(
            HOST,
            Action::Start {
                first: HOST,
                opening: false,
            },
        );
        for (seat, land, creature) in [
            (HOST, "Forest", "Grizzly Bears"),
            (GUEST, "Plains", "Runeclaw Bear"),
        ] {
            self.accept(seat, Action::Draw { seat, count: 1 });
            self.move_card(
                seat,
                land,
                Zone::Hand(seat),
                ZoneName::Hand,
                ComputedDestination::Play,
            );
            self.move_card(
                seat,
                creature,
                Zone::Hand(seat),
                ZoneName::Hand,
                ComputedDestination::Play,
            );
            self.move_card(
                seat,
                creature,
                Zone::Stack,
                ZoneName::Stack,
                ComputedDestination::Resolve,
            );
        }
    }

    fn combat(&mut self) {
        self.accept(
            HOST,
            Action::SetAssist {
                assist: Assist::Combat,
                on: true,
            },
        );
        self.accept(
            HOST,
            Action::Phase {
                phase: Phase::DeclareAttackers,
            },
        );
        self.arrow(HOST, "Grizzly Bears", Target::Player(GUEST));
        self.accept(
            HOST,
            Action::Phase {
                phase: Phase::DeclareBlockers,
            },
        );
        let attacker = reference(&self.views[1], "Grizzly Bears", ZoneName::Battlefield);
        self.arrow(
            GUEST,
            "Runeclaw Bear",
            Target::Object(SeenObject {
                id: attacker,
                zone: Zone::Battlefield,
            }),
        );

        let (table, _) = replay(&self.path);
        assert_eq!(
            table
                .arrows
                .iter()
                .map(|arrow| arrow.kind)
                .collect::<Vec<_>>(),
            [ArrowKind::Attack, ArrowKind::Block]
        );
        let proposal = combat::proposal(&table).unwrap();
        let action = apply_proposal(&proposal)
            .try_map_objects(&mut |id, zone| {
                assert_eq!(zone, Zone::Battlefield);
                Ok::<_, Infallible>(reference(
                    &self.views[1],
                    table.objects[&id].card.name().unwrap(),
                    ZoneName::Battlefield,
                ))
            })
            .unwrap();
        self.accept(GUEST, action);
        let (after, log) = replay(&self.path);
        assert_eq!(after.phase, Phase::EndOfCombat);
        for seat in [HOST, GUEST] {
            assert_eq!(after.zone(Zone::Graveyard(seat)).unwrap().len(), 1);
        }
        let applied = log.entries().last().unwrap();
        assert_eq!(applied.by, GUEST);
        self.accept(
            GUEST,
            Action::Undo {
                entry: Some(applied.n),
            },
        );
        let (undone, _) = replay(&self.path);
        assert_eq!(undone.phase, Phase::DeclareBlockers);
        assert_eq!(undone.arrows, table.arrows);
        assert_eq!(
            undone.zone(Zone::Battlefield),
            table.zone(Zone::Battlefield)
        );
    }
    fn check_blocked_undo(&mut self) -> TableAction {
        let lose_life = Action::Life {
            seat: GUEST,
            adjustment: Adjustment::Add(-1),
        };
        self.accept(
            GUEST,
            Action::Batch {
                actions: vec![lose_life.clone()],
            },
        );
        let (_, log) = replay(&self.path);
        let batch = log.entries().last().unwrap().n;
        self.accept(HOST, lose_life.clone());
        assert_eq!(
            self.guest.act(Action::Undo { entry: Some(batch) }),
            Err(Refusal::Invalid {
                blocking: vec![batch + 1]
            })
        );
        assert!(
            self.updates
                .iter()
                .all(|updates| updates.try_recv().is_err())
        );

        lose_life
    }

    fn check_resumed(&self, guest_id: SessionId) {
        let (table, log) = replay(&self.path);
        let resumed = &self.views[1];
        assert_eq!(resumed.session, guest_id);
        let script = resumed.view.script.as_ref().unwrap();
        assert!(script.reset.is_some());
        // Restored arrows are applied immediately by the stage on a reset.
        assert_eq!(script.steps(), 2);
        assert!(script.beats.iter().flatten().all(|step| matches!(
            step.act,
            Act::Point {
                span: Span::Begin,
                ..
            },
        )));
        check_update(None, resumed);
        let expected = SeatPublication::new(&table, &log, GUEST).unwrap();
        assert!(Stage::of(&resumed.view).shows(&Stage::of(expected.view())));
        assert_eq!(resumed.view.players, expected.view().players);
        assert_eq!(self.guest.current().unwrap(), *resumed);
    }
}

#[test]
fn a_host_and_guest_play_a_scripted_game() {
    let directory = tempfile::tempdir().unwrap();
    let (host, host_updates, _) = watch(directory.path(), SessionIds::default());
    let guest_ids = SessionIds::default();
    let earlier_guest_game = guest_ids.allocate();
    let (guest, guest_updates, guest_events) = watch(directory.path(), guest_ids);
    let (links, host_links) = sync_channel(1);
    let mut settings = HostSettings::new("Derek", INVITE, SECRET);
    settings.starting_hand_size = 2;
    let host_id = host.host(host_links, settings).unwrap();
    let first_host = host_updates.recv().unwrap();
    check_update(None, &first_host);

    // The test releases each connection through a channel, including the
    // redial. It can inspect the disconnected state before the host answers.
    let (dial, guest_links) = sync_channel(1);
    let (host_link, guest_link) = memory::pair();
    let sever = guest_link.try_clone_sender().unwrap();
    links.send(host_link).unwrap();
    dial.send(guest_link).unwrap();
    let guest_id = guest.join(
        move || guest_links.recv().map_err(|_| LinkError::Closed),
        "Mara",
        SECRET,
    );
    assert!(guest_id > earlier_guest_game);
    assert_ne!(guest_id, host_id);
    let views = [host_updates.recv().unwrap(), guest_updates.recv().unwrap()];
    assert_eq!(views[1].session, guest_id);
    check_update(None, &views[1]);
    assert_connected(&guest_events);

    let saved = host.list_saved().unwrap().pop().unwrap();
    let mut game = Game {
        host,
        guest,
        updates: [host_updates, guest_updates],
        views,
        path: directory
            .path()
            .join("tables")
            .join(format!("{}.jsonl", saved.id)),
    };
    game.check_privacy();
    assert_eq!(
        game.guest
            .act(load(HOST, ["Forest", "Grizzly Bears", PRIVATE_CARD])),
        Err(Refusal::Permission)
    );
    assert!(
        game.updates
            .iter()
            .all(|updates| updates.try_recv().is_err())
    );

    game.opening();
    game.combat();

    let lose_life = game.check_blocked_undo();

    sever.close().unwrap();
    let frozen = game.updates[0].recv().unwrap();
    assert_eq!(
        frozen.view.table.as_ref().unwrap().link,
        LinkState::Reconnecting
    );
    assert!(Stage::of(&frozen.view).shows(&Stage::of(&game.views[0].view)));
    assert_eq!(
        link_event(&guest_events),
        TableEvent::Link(LinkState::Reconnecting)
    );
    assert_eq!(game.host.act(lose_life.clone()), Err(Refusal::Stale));
    assert_eq!(game.guest.act(lose_life), Err(Refusal::Stale));

    let old_reference = reference(&game.views[1], "Runeclaw Bear", ZoneName::Battlefield);
    let (host_link, guest_link) = memory::pair();
    links.send(host_link).unwrap();
    dial.send(guest_link).unwrap();
    game.views = game
        .updates
        .each_ref()
        .map(|updates| updates.recv().unwrap());
    assert_connected(&guest_events);
    game.check_privacy();
    game.check_resumed(guest_id);
    let tap = |reference| Action::Tap {
        selection: Selection {
            objects: vec![reference],
            zone: Zone::Battlefield,
        },
        tapped: true,
    };
    assert_eq!(game.guest.act(tap(old_reference)), Err(Refusal::Stale));
    let blocker = reference(&game.views[1], "Runeclaw Bear", ZoneName::Battlefield);
    game.accept(GUEST, tap(blocker));

    game.host.leave();
    assert_eq!(link_event(&guest_events), TableEvent::Link(LinkState::Left));
    game.guest.leave();
    assert!(game.guest.current().is_none());
}

#[test]
fn an_interrupted_action_wakes_its_caller_and_lobby_payloads_pass_through() {
    let directory = tempfile::tempdir().unwrap();
    let (guest, updates, events) = watch(directory.path(), SessionIds::default());
    let (mut peer, link) = memory::pair();
    let mut link = Some(link);
    let session = guest.join(move || link.take().ok_or(LinkError::Closed), "Mara", SECRET);
    assert!(matches!(
        decode(&peer.receive().unwrap()).unwrap(),
        Message::Hello(_)
    ));

    let link_state = LinkState::Connected {
        direct: true,
        round_trip_ms: None,
    };
    let table = Table::new(2);
    let view = SeatPublication::new(&table, &Log::new(&table), GUEST).unwrap();
    let mut update = GameUpdate {
        session: SessionId(50),
        view: view.view().clone(),
    };
    update.view.table.as_mut().unwrap().link = link_state;
    let lobby = LobbyUpdate {
        view: None,
        game: None,
    };
    for message in [
        Message::Welcome {
            seat: GUEST.0,
            table: TableStatus::New,
            host: "Derek".into(),
        },
        Message::Update(serde_json::to_value(&update).unwrap()),
        Message::Lobby(serde_json::to_value(&lobby).unwrap()),
    ] {
        peer.send(&encode(&message)).unwrap();
    }
    update.session = session;
    assert_eq!(
        serde_json::to_value(updates.recv().unwrap()).unwrap(),
        serde_json::to_value(update).unwrap(),
    );
    assert_eq!(events.recv().unwrap(), TableEvent::Link(link_state));
    assert_eq!(events.recv().unwrap(), TableEvent::Lobby(lobby));

    thread::scope(|scope| {
        let reply = scope.spawn(|| {
            guest.act(Action::Draw {
                seat: GUEST,
                count: 1,
            })
        });
        loop {
            match decode(&peer.receive().unwrap()).unwrap() {
                Message::Act { n: 1, .. } => break,
                Message::Ping { n } => peer.send(&encode(&Message::Pong { n })).unwrap(),
                message => panic!("unexpected message: {message:?}"),
            }
        }
        // Lose the connection after the request is delivered but before any reply.
        peer.sever();
        assert_eq!(
            events.recv().unwrap(),
            TableEvent::Link(LinkState::Reconnecting)
        );
        assert_eq!(reply.join().unwrap(), Err(Refusal::Stale));
    });
    guest.leave();
}

fn link_event(events: &Receiver<TableEvent>) -> TableEvent {
    loop {
        let event = events.recv().unwrap();
        if !matches!(event, TableEvent::Lobby(_)) {
            return event;
        }
    }
}

fn assert_connected(events: &Receiver<TableEvent>) {
    assert!(matches!(
        link_event(events),
        TableEvent::Link(LinkState::Connected { direct: true, .. })
    ));
}

#[test]
fn an_authenticated_version_mismatch_reaches_both_players() {
    let directory = tempfile::tempdir().unwrap();
    let (host, _, host_events) = watch(directory.path(), SessionIds::default());
    let (links, incoming) = sync_channel(1);
    host.host(incoming, HostSettings::new("Derek", INVITE, SECRET))
        .unwrap();
    let (host_link, guest_link) = memory::pair();
    links.send(host_link).unwrap();
    let mut guest_link = Some(guest_link);
    let mut guest = beryl_link::guest::join(
        move || guest_link.take().ok_or(LinkError::Closed),
        Hello {
            wire: "another version".into(),
            secret: SECRET.into(),
            name: "Mara".into(),
        },
        beryl_link::guest::REDIAL_DELAY,
        PING_INTERVAL,
    );

    assert_eq!(link_event(&host_events), TableEvent::Link(LinkState::Alone));
    assert_eq!(
        link_event(&host_events),
        TableEvent::Error(VERSION_MISMATCH.into())
    );
    assert_eq!(
        guest.take_events().unwrap().recv().unwrap(),
        beryl_link::guest::Event::Mismatch(VERSION_MISMATCH.into()),
    );
    assert!(host.list_saved().unwrap()[0].guest.is_none());
}
