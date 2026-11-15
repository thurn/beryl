//! The frame thread never waits for the session: while the session thread is inside a call
//! that does not return until the test lets it, the frame loop draws a frame on every tick and
//! the commands sent meanwhile queue up. Each reply then reaches the host's inbox, in order.

use std::sync::mpsc::{Receiver, channel};

use beryl_3d::session::{self, Session};
use beryl_host::{Draw, FrameLoop, Inbox, Shown, Ticks};
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
    let commands = session::spawn(move || Held { releases: held }, Inbox::new(sender, || true));
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
