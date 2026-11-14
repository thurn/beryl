use std::{
    sync::{Mutex, mpsc},
    thread,
    time::Duration,
};

use beryl_link::{
    Link, LinkError, PING_INTERVAL, Route,
    guest::{Event, REDIAL_DELAY, join},
    host::{Host, Hosted, Sender, serve},
    memory::{self, MemoryLink},
    wire::{Hello, Message, RefusalReason, TableStatus, VERSION_MISMATCH, decode, encode},
};
use flate2::read::DeflateDecoder;
use serde_json::{Value, json};

const WIRE: &str = "9c41e07ab2d35f18";
const SECRET: &str = "3mxd";

#[derive(Default)]
struct Counter {
    count: u64,
    sender: Option<Sender>,
    dropped: usize,
    left: usize,
}

impl Counter {
    fn increment(&mut self) {
        self.count += 1;
        if let Some(sender) = &self.sender {
            sender.update(json!({"count": self.count})).unwrap();
        }
    }
}

impl Hosted for Counter {
    fn seat_for(&mut self, name: &str) -> Option<u8> {
        (name == "Mara").then_some(1)
    }

    fn act(&mut self, seat: u8, action: Value) -> Result<(), RefusalReason> {
        assert_eq!(seat, 1);
        if action == json!("increment") {
            self.increment();
            Ok(())
        } else {
            Err(RefusalReason::Permission)
        }
    }

    fn update_for(&mut self, seat: u8) -> Value {
        assert_eq!(seat, 1);
        json!({"count": self.count})
    }

    fn lobby_for(&mut self, seat: u8) -> Option<Value> {
        assert_eq!(seat, 1);
        Some(json!({"ready": true}))
    }

    fn connected(&mut self, seat: u8, sender: Sender) {
        assert_eq!(seat, 1);
        self.sender = Some(sender);
    }

    fn left(&mut self, seat: u8) {
        assert_eq!(seat, 1);
        self.sender = None;
        self.left += 1;
    }

    fn dropped(&mut self, seat: u8) {
        assert_eq!(seat, 1);
        self.sender = None;
        self.dropped += 1;
    }
}

fn hello() -> Hello {
    Hello {
        wire: WIRE.into(),
        secret: SECRET.into(),
        name: "Mara".into(),
    }
}

fn host() -> Host {
    Host::new(WIRE, SECRET, "Derek")
}

fn verify_connected(events: &mpsc::Receiver<Event>, first: &Event, count: u64) {
    assert_eq!(&events.recv().unwrap(), first);
    assert_eq!(
        events.recv().unwrap(),
        Event::Update(json!({"count": count}))
    );
    assert_eq!(events.recv().unwrap(), Event::Lobby(json!({"ready": true})));
}

#[test]
fn wire_uses_the_documented_json_envelopes() {
    for (message, value) in [
        (
            Message::Hello(hello()),
            json!({"hello": {"wire": WIRE, "secret": SECRET, "name": "Mara"}}),
        ),
        (
            Message::Welcome {
                seat: 1,
                table: TableStatus::Resumed,
                host: "Derek".into(),
            },
            json!({"welcome": {"seat": 1, "table": "resumed", "host": "Derek"}}),
        ),
        (
            Message::Act {
                n: 118,
                action: json!({"tap": {"objects": [41], "in": "battlefield", "to": true}}),
            },
            json!({"act": {"n": 118, "action": {"tap": {"objects": [41], "in": "battlefield", "to": true}}}}),
        ),
        (Message::Done { n: 118 }, json!({"done": {"n": 118}})),
        (
            Message::Refused {
                n: 118,
                reason: RefusalReason::Stale,
            },
            json!({"refused": {"n": 118, "reason": "stale"}}),
        ),
        (
            Message::Update(json!({"view": {}, "script": {}})),
            json!({"update": {"view": {}, "script": {}}}),
        ),
        (
            Message::Lobby(json!({"ready": true})),
            json!({"lobby": {"ready": true}}),
        ),
    ] {
        let frame = encode(&message);
        assert_eq!(
            serde_json::from_reader::<_, Value>(DeflateDecoder::new(frame.as_slice())).unwrap(),
            value
        );
        assert_eq!(decode(&frame), Ok(message));
    }
    assert!(decode(b"invalid compressed bytes").is_err());
}

#[test]
fn wrong_secret_closes_without_sending_anything() {
    let (link, mut peer) = memory::pair();
    let table = Mutex::new(Counter::default());
    let mut hello = hello();
    hello.secret = "wrong".into();
    hello.wire = "also wrong".into();
    peer.send(&encode(&Message::Hello(hello))).unwrap();
    assert!(matches!(
        serve(link, &mut host(), &table, PING_INTERVAL),
        Err(LinkError::Refused(_))
    ));
    assert_eq!(peer.receive(), Err(LinkError::Closed));
    assert!(table.into_inner().unwrap().sender.is_none());
}

#[test]
fn both_sides_see_the_version_mismatch() {
    let (link, peer) = memory::pair();
    let mut hello = hello();
    hello.wire = "wrong".into();
    let mut peer = Some(peer);
    let mut guest = join(
        move || Ok(peer_once(&mut peer)),
        hello,
        REDIAL_DELAY,
        PING_INTERVAL,
    );
    let events = guest.take_events().unwrap();
    let table = Mutex::new(Counter::default());
    assert_eq!(
        serve(link, &mut host(), &table, PING_INTERVAL),
        Err(LinkError::Refused(VERSION_MISMATCH.into()))
    );
    assert_eq!(
        events.recv().unwrap(),
        Event::Mismatch(VERSION_MISMATCH.into())
    );
    assert!(events.recv().is_err());
}

fn peer_once(peer: &mut Option<MemoryLink>) -> MemoryLink {
    peer.take().expect("only one connection was expected")
}

#[test]
fn acts_and_host_updates_share_one_order_and_a_refusal_keeps_its_number() {
    let (link, peer) = memory::pair();
    let mut peer = Some(peer);
    let mut guest = join(
        move || Ok(peer_once(&mut peer)),
        hello(),
        REDIAL_DELAY,
        PING_INTERVAL,
    );
    let events = guest.take_events().unwrap();
    let table = Mutex::new(Counter::default());
    thread::scope(|scope| {
        let server = scope.spawn(|| serve(link, &mut host(), &table, PING_INTERVAL));
        verify_connected(&events, &Event::Joined { seat: 1 }, 0);
        assert_eq!(guest.peer().unwrap().name, "Derek");
        assert_eq!(guest.peer().unwrap().route, Route::Memory);
        table.lock().unwrap().increment();

        let first = guest.act(json!("increment")).unwrap();
        let second = guest.act(json!("increment")).unwrap();
        let refused = guest.act(json!("decrement")).unwrap();
        assert_eq!(events.recv().unwrap(), Event::Update(json!({"count": 1})));
        assert_eq!(events.recv().unwrap(), Event::Update(json!({"count": 2})));
        assert_eq!(events.recv().unwrap(), Event::Done { n: first });
        assert_eq!(events.recv().unwrap(), Event::Update(json!({"count": 3})));
        assert_eq!(events.recv().unwrap(), Event::Done { n: second });
        assert_eq!(
            events.recv().unwrap(),
            Event::Refused {
                n: refused,
                reason: RefusalReason::Permission
            }
        );

        guest.leave();
        assert_eq!(server.join().unwrap(), Ok(()));
    });
    let table = table.into_inner().unwrap();
    assert_eq!(table.count, 3);
    assert_eq!(table.left, 1);
    assert_eq!(table.dropped, 0);
}

#[test]
fn a_severed_link_redials_and_resumes_with_a_whole_view() {
    let (link, peer) = memory::pair();
    let disconnect = peer.try_clone_sender().unwrap();
    let (resume_link, resume_peer) = memory::pair();
    let mut peers = [peer, resume_peer].into_iter();
    let mut guest = join(
        move || Ok(peers.next().unwrap()),
        hello(),
        Duration::ZERO,
        PING_INTERVAL,
    );
    let events = guest.take_events().unwrap();
    let table = Mutex::new(Counter::default());
    let mut host = host();
    thread::scope(|scope| {
        let server = scope.spawn(|| {
            assert_eq!(
                serve(link, &mut host, &table, PING_INTERVAL),
                Err(LinkError::Closed)
            );
            serve(resume_link, &mut host, &table, PING_INTERVAL)
        });

        verify_connected(&events, &Event::Joined { seat: 1 }, 0);
        let n = guest.act(json!("increment")).unwrap();
        assert_eq!(events.recv().unwrap(), Event::Update(json!({"count": 1})));
        assert_eq!(events.recv().unwrap(), Event::Done { n });
        disconnect.close().unwrap();
        assert_eq!(events.recv().unwrap(), Event::Reconnecting);
        verify_connected(&events, &Event::Resumed { seat: 1 }, 1);
        let sender = table.lock().unwrap().sender.clone().unwrap();
        sender.leave().unwrap();
        assert_eq!(events.recv().unwrap(), Event::Left);
        assert!(events.recv().is_err());
        assert_eq!(server.join().unwrap(), Ok(()));
    });
    let table = table.into_inner().unwrap();
    assert_eq!(table.dropped, 1);
    assert_eq!(table.left, 1);
}

#[test]
fn leave_cancels_redial_during_an_attempt() {
    let (link, peer) = memory::pair();
    let disconnect = peer.try_clone_sender().unwrap();
    let mut peer = Some(peer);
    let (dialing, attempted) = mpsc::channel();
    let (dial_result, blocked_dial) = mpsc::channel::<Result<MemoryLink, LinkError>>();
    let mut guest = join(
        move || {
            if let Some(peer) = peer.take() {
                return Ok(peer);
            }
            dialing.send(()).unwrap();
            blocked_dial.recv().unwrap()
        },
        hello(),
        Duration::ZERO,
        PING_INTERVAL,
    );
    let events = guest.take_events().unwrap();
    let table = Mutex::new(Counter::default());
    thread::scope(|scope| {
        let server = scope.spawn(|| serve(link, &mut host(), &table, PING_INTERVAL));
        verify_connected(&events, &Event::Joined { seat: 1 }, 0);
        disconnect.close().unwrap();
        assert_eq!(events.recv().unwrap(), Event::Reconnecting);
        attempted.recv().unwrap();
        guest.leave();
        let (new_link, mut new_peer) = memory::pair();
        dial_result.send(Ok(new_link)).unwrap();
        assert_eq!(new_peer.receive(), Err(LinkError::Closed));
        assert!(events.recv().is_err());
        assert!(attempted.recv().is_err());
        assert_eq!(server.join().unwrap(), Err(LinkError::Closed));
    });
}

#[test]
fn host_disconnects_after_three_unanswered_pings() {
    let (link, mut peer) = memory::pair();
    peer.send(&encode(&Message::Hello(hello()))).unwrap();
    let table = Mutex::new(Counter::default());
    assert_eq!(
        serve(link, &mut host(), &table, Duration::ZERO),
        Err(LinkError::Closed)
    );
    assert_eq!(table.into_inner().unwrap().dropped, 1);
}
