//! The frame thread never waits for the session: while the session thread is inside a call
//! that does not return until the test lets it, the frame loop draws a frame on every tick and
//! the commands sent meanwhile queue up. Each reply then reaches the host's inbox, in order.
//! And what `--agent` reports of the opponent's searches, and that the live session starts a
//! solo manual table and sends its view back.

use std::sync::mpsc::{Receiver, channel};

use beryl_3d::session::{self, Session};
use beryl_host::{Class, Draw, FrameLoop, Inbox, Shown, Ticks};
use beryl_scene::command::{Reply, SessionCommand};
use beryl_scene::frame::Frame;
use beryl_scene::input::{Input, Lifecycle};
use beryl_scene::{Client, Config};
use beryl_session::{SessionError, SessionId};

/// A stand-in for `Sessions` whose every call takes as long as the test holds it: it returns
/// only when the test sends it a release.
struct Held {
    releases: Receiver<()>,
}

impl Session for Held {
    /// Answers each concession with an error that names its session, so that the test can
    /// tell the replies apart.
    fn run(&mut self, command: SessionCommand) -> Reply {
        self.releases.recv().expect("the test releases every call");
        let SessionCommand::Concede(session) = command else {
            panic!("the test sends only concessions");
        };
        Reply::Done(Err(SessionError::StaleSession { session }))
    }
}

struct NoTicks;

impl Ticks for NoTicks {
    fn set_running(&mut self, _running: bool) {}
}

struct Counter(u32);

impl Draw for Counter {
    fn draw(&mut self, _frame: &Frame) -> Option<Shown> {
        self.0 += 1;
        Some(Shown::default())
    }
}

#[test]
fn the_frame_loop_draws_every_tick_while_the_session_is_busy() {
    let (releases, held) = channel();
    let (sender, inbox) = channel();
    let commands = session::spawn(
        Class::Utility,
        move || Held { releases: held },
        Inbox::new(sender, || true),
    );
    let client = Client::new(Config {
        elements: 64,
        draws: 64,
        name_bytes: 1024,
    });
    let mut frames = FrameLoop::new(client, NoTicks);
    let mut counter = Counter(0);
    let mut sent = 0;
    for tick in 0..30_u32 {
        if tick % 5 == 0 {
            commands.send(SessionCommand::Concede(SessionId(sent)));
            sent += 1;
        }
        // Any input makes the client want the next frame.
        frames.handle(Input::Lifecycle(Lifecycle::Resumed));
        let drawn = frames.tick(u64::from(tick + 1) * 8_333_333, &mut counter);
        assert!(drawn.is_some(), "tick {tick} drew no frame");
    }
    assert_eq!(counter.0, 30);
    assert!(inbox.try_recv().is_err(), "a held call answered");
    for _ in 0..sent {
        releases.send(()).expect("the session thread runs");
    }
    for id in 0..sent {
        let reply = inbox.recv().expect("a reply for every command");
        let Input::Reply(Reply::Done(Err(SessionError::StaleSession { session }))) = reply else {
            panic!("not the stand-in's reply: {reply:?}");
        };
        assert_eq!(session, SessionId(id), "the replies came out of order");
    }
    commands.close();
}

#[test]
fn the_search_report_rates_rollouts_and_engine_steps_over_the_time_searched() {
    let searches = [
        session::Searched {
            ms: 1_500,
            rollouts: 3_000,
            work: 1_000_000,
        },
        session::Searched {
            ms: 500,
            rollouts: 1_000,
            work: 3_000_000,
        },
    ];
    assert_eq!(
        session::search_report(&searches).as_deref(),
        Some(
            "beryl3d: search: 2 decisions searched in 2.0 s, 4000 rollouts and 4000000 engine \
             steps: 2000 rollouts/s, 2000000 engine steps/s"
        )
    );
    assert_eq!(session::search_report(&[]), None);
}

#[test]
fn the_live_session_starts_a_solo_table_and_sends_its_view() {
    let (sender, inbox) = channel();
    let commands = session::spawn(
        Class::Utility,
        {
            let inbox = Inbox::new(sender.clone(), || true);
            move || session::Live::new(None, inbox)
        },
        Inbox::new(sender, || true),
    );
    let request = beryl_scene::manual::solo_request();
    assert!(
        request
            .deck
            .main
            .iter()
            .any(|(name, _)| name == "Counterspell"),
        "the solo table starts with the WU Control deck"
    );
    commands.send(SessionCommand::TableSolo(request));
    let (mut started, mut viewed) = (false, false);
    while !(started && viewed) {
        match inbox.recv().expect("the session thread answers") {
            Input::Reply(Reply::TableSolo(result)) => {
                result.expect("the solo table starts");
                started = true;
            }
            Input::Update(update) if update.view.table.is_some() => viewed = true,
            _ => {}
        }
    }
    commands.close();
}
