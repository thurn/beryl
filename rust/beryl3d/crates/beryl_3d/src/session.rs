//! The session thread (docs/beryl-3d.md, "Threads"). The frame thread never calls a function of
//! `Sessions`, because one can wait on a lock: it sends each `SessionCommand` down a channel,
//! and this thread calls the function the command names and hands the result back to the host
//! as an `Input::Reply`, which wakes the event loop.
//!
//! The thread puts itself in the utility scheduling class before it starts or resumes a game.
//! The game's thread and the search threads are started from it and inherit that class, so the
//! AI runs one class below drawing (decision C9) with no hook in the session or the search.

use std::path::PathBuf;
use std::sync::mpsc::{Sender, channel};
use std::thread::JoinHandle;

use beryl_host::Inbox;
use beryl_scene::command::{Reply, SessionCommand};
use beryl_scene::input::Input;
use beryl_session::{Sessions, StartRequest};

/// What answers the frame thread's commands: `Sessions`, or a test's stand-in.
pub trait Session {
    /// Called once on the session thread before the first command: starts or resumes a game.
    fn begin(&mut self) {}

    /// Calls the function of the session that `command` names, and returns its result.
    fn run(&mut self, command: SessionCommand) -> Reply;
}

/// The frame thread's end of the session thread: where commands go.
pub struct Commands {
    sender: Sender<SessionCommand>,
    thread: JoinHandle<()>,
}

impl Commands {
    /// Sends `command` to the session thread and returns at once.
    pub fn send(&self, command: SessionCommand) {
        // The thread ends only when this handle is dropped, so a failed send means it
        // panicked, and the panic has already been printed.
        let _ = self.sender.send(command);
    }

    /// Ends the session thread once it has run every command sent so far, and waits for it.
    pub fn close(self) {
        drop(self.sender);
        let _ = self.thread.join();
    }
}

/// Starts the session thread: `make` builds the session there, which `begin`s and then runs
/// each command sent, sending each reply to `inbox`.
///
/// # Panics
///
/// When the system cannot start a thread.
pub fn spawn<S: Session>(make: impl FnOnce() -> S + Send + 'static, inbox: Inbox) -> Commands {
    let (sender, commands) = channel::<SessionCommand>();
    let thread = std::thread::Builder::new()
        .name("session".to_owned())
        .spawn(move || {
            beryl_host::utility_thread();
            let mut session = make();
            session.begin();
            for command in commands {
                if !inbox.send(Input::Reply(session.run(command))) {
                    return;
                }
            }
        })
        .expect("the session thread starts");
    Commands { sender, thread }
}

/// The real session. Its views go to the host's inbox as they are emitted.
pub struct Live {
    sessions: Sessions,
}

impl Live {
    /// Sessions that keep their replays and ledger in `data`, as the web app keeps them in its
    /// data directory, and send every view to `inbox`.
    #[must_use]
    pub fn new(data: Option<PathBuf>, inbox: Inbox) -> Self {
        let replays = data.as_ref().map(|data| data.join("replays"));
        let sessions = Sessions::new(replays, move |update| {
            inbox.send(Input::Update(Box::new(update)));
        });
        let sessions = match data {
            Some(data) => sessions.with_ledger(data.join("games.jsonl")),
            None => sessions,
        };
        Self { sessions }
    }
}

impl Session for Live {
    /// Resumes the newest unfinished game, as the web app does on launch, or else starts a game
    /// against the first agent a person is offered: the app has no start screen yet.
    fn begin(&mut self) {
        if self.sessions.resume_latest().is_some() {
            return;
        }
        let Some(agent) = beryl_session::agents().into_iter().next() else {
            eprintln!("beryl3d: no agent to play against");
            return;
        };
        let request = StartRequest {
            agent: agent.name,
            seed: None,
            fair: false,
        };
        if let Err(error) = self.sessions.start(&request) {
            eprintln!("beryl3d: the game did not start: {error:?}");
        }
    }

    fn run(&mut self, command: SessionCommand) -> Reply {
        let sessions = &self.sessions;
        match command {
            SessionCommand::Click(request) => Reply::Click(sessions.click(request)),
            SessionCommand::Start(request) => Reply::Done(sessions.start(&request).map(|_| ())),
            SessionCommand::Resume => {
                sessions.resume_latest();
                Reply::Done(Ok(()))
            }
            SessionCommand::Choose(request) => Reply::Done(sessions.choose_request(request)),
            SessionCommand::Cancel(decision) => Reply::Done(sessions.cancel(decision)),
            SessionCommand::Undo(decision) => Reply::Done(sessions.undo(decision)),
            SessionCommand::PassOn(decision) => Reply::Done(sessions.pass_on(decision)),
            SessionCommand::SetStops(session, stops) => {
                Reply::Done(sessions.set_stops(session, stops))
            }
            SessionCommand::Concede(session) => Reply::Done(sessions.concede(session)),
        }
    }
}
