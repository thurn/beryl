//! The session thread (docs/beryl-3d.md, "Threads"). The frame thread never calls a function of
//! `Sessions`, because one can wait on a lock: it sends each `SessionCommand` down a channel,
//! and this thread calls the function the command names and hands the result back to the host
//! as an `Input::Reply`, which wakes the event loop. A bug report goes down the same channel,
//! so that it is written after every command sent before it, and its result comes back as an
//! `Input::BugSaved`.
//!
//! The thread puts itself in the default scheduling class before it starts or resumes a game.
//! The game's thread and the search threads are started from it and inherit that class, so the
//! AI runs below drawing and the asset workers (decision C9) with no hook in the session or the
//! search, and as fast as the web app's session, which runs in the same class.
//! `--search-class` puts it in another class, to compare them.

use std::sync::mpsc::{Sender, channel};
use std::thread::JoinHandle;

use beryl_host::{Class, Inbox};
use beryl_protocol::AgentView;
use beryl_protocol::ipc::TableLobbyRequest;
use beryl_scene::command::{Reply, SessionCommand};
use beryl_scene::input::Input;
use beryl_scene::manual::tokens;
use beryl_session::ai::AgentMove;
use beryl_session::{Seat, Sessions, StartGame, StopsSetting};
use beryl_shell::Dirs;
use beryl_table_session::TableEvent;
use beryl_table_session::connections::TableConnections;
use beryl_table_session::decks::{self, Decks};

use crate::bugs::Job;

/// What answers the frame thread's commands: `Sessions`, or a test's stand-in.
pub trait Session {
    /// Called once on the session thread before the first command: starts or resumes a game.
    fn begin(&mut self) {}

    /// Calls the function of the session that `command` names, and returns its result.
    fn run(&mut self, command: SessionCommand) -> Reply;

    /// Writes a bug report, with what the session knows of the game.
    ///
    /// # Errors
    ///
    /// When the report cannot be written, with the reason the client shows.
    fn save_bug(&mut self, job: Job) -> Result<(), String> {
        job.write(None)
    }

    /// Called once on the session thread after the last command, when the frame thread has
    /// closed its end.
    fn end(&mut self) {}
}

/// The frame thread's end of the session thread: where commands go. Dropping it ends the
/// session thread once it has run every command sent so far, and waits for it.
pub struct Commands {
    sender: Option<Sender<Work>>,
    thread: Option<JoinHandle<()>>,
}

impl Commands {
    /// Sends `command` to the session thread and returns at once.
    pub fn send(&self, command: SessionCommand) {
        // The thread ends only when this handle is dropped, so a failed send means it
        // panicked, and the panic has already been printed.
        if let Some(sender) = &self.sender {
            let _ = sender.send(Work::Command(command));
        }
    }

    /// Sends a bug report to be written on the session thread and returns at once.
    pub fn save_bug(&self, job: Job) {
        if let Some(sender) = &self.sender {
            let _ = sender.send(Work::Bug(Box::new(job)));
        }
    }

    /// Ends the session thread once it has run every command sent so far, and waits for it.
    pub fn close(self) {
        drop(self);
    }
}

impl Drop for Commands {
    fn drop(&mut self) {
        drop(self.sender.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Starts the session thread in `class`: `make` builds the session there, which `begin`s,
/// runs each command sent, sending each reply to `inbox`, and `end`s when `Commands` is
/// dropped.
///
/// # Panics
///
/// When the system cannot start a thread.
pub fn spawn<S: Session>(
    class: Class,
    make: impl FnOnce() -> S + Send + 'static,
    inbox: Inbox,
) -> Commands {
    let (sender, commands) = channel::<Work>();
    let thread = std::thread::Builder::new()
        .name("session".to_owned())
        .spawn(move || {
            beryl_host::set_class(class);
            let mut session = make();
            session.begin();
            for work in commands {
                let answer = match work {
                    Work::Command(command) => Input::Reply(session.run(command)),
                    Work::Bug(job) => Input::BugSaved(session.save_bug(*job)),
                };
                if !inbox.send(answer) {
                    break;
                }
            }
            session.end();
        })
        .expect("the session thread starts");
    Commands {
        sender: Some(sender),
        thread: Some(thread),
    }
}

/// What the session thread is sent.
enum Work {
    Command(SessionCommand),
    Bug(Box<Job>),
}

/// The real session. Its views go to the host's inbox as they are emitted.
pub struct Live {
    sessions: Sessions,
    /// The manual table, which numbers its sessions in the same sequence as `sessions` and
    /// sends its views to the same inbox, as the web app's `table_*` commands share one.
    tables: TableConnections,
    /// The deck library, in the folder the web app keeps its decks in (`decks` in the data
    /// folder), so that a deck saved in one app is offered in the other.
    decks: Decks,
    /// The host's inbox, which `begin` sends the agents to.
    inbox: Inbox,
    /// The game `--agent` asked for, and the agent that answers for the viewer in it.
    played_by: Option<(StartGame, String)>,
    /// The agents `--offer` adds after the session's own on the start screen.
    offered: Vec<AgentView>,
}

impl Live {
    /// Sessions that keep their replays and ledger in the data folder of `dirs`, the web app's,
    /// or keep none without it, and send every view to `inbox`. The manual table keeps its
    /// saves, settings and deck library in the same folder, or in the system's temporary folder
    /// without it.
    #[must_use]
    pub fn new(dirs: Option<Dirs>, inbox: Inbox) -> Self {
        let views = inbox.clone();
        let emit = move |update| {
            views.send(Input::Update(Box::new(update)));
        };
        let data = dirs.as_ref().map_or_else(
            || std::env::temp_dir().join("beryl3d-tables"),
            |dirs| dirs.data.clone(),
        );
        let decks = Decks::new(data.join("decks"));
        let sessions = match dirs {
            Some(dirs) => beryl_shell::sessions(&dirs, emit),
            None => Sessions::new(None, emit),
        };
        let table_views = inbox.clone();
        let lobbies = inbox.clone();
        // Of the table's events only its lobby's reach the client: its link's belong to hosted
        // and joined tables, which Beryl 3D does not open yet.
        let tables = TableConnections::new(
            sessions.session_ids(),
            data,
            move |update| {
                table_views.send(Input::Update(Box::new(update)));
            },
            move |event| {
                if let TableEvent::Lobby(update) = event {
                    lobbies.send(Input::Lobby(Box::new(update)));
                }
            },
        );
        Self {
            sessions,
            tables,
            decks,
            inbox,
            played_by: None,
            offered: Vec::new(),
        }
    }

    /// The same sessions, whose start screen also offers the agents `names`, after the
    /// session's own list. A name no agent has is left out.
    #[must_use]
    pub fn offering(mut self, names: &[String]) -> Self {
        self.offered = (names.iter())
            .filter_map(|name| beryl_search::agent(name))
            .map(|agent| AgentView {
                name: agent.name.to_owned(),
                title: agent.title.to_owned(),
                description: agent.description.to_owned(),
            })
            .collect();
        self
    }

    /// The same sessions, which start a fresh game of the web app's decks between `opponent`
    /// and `agent`, which answers for the viewer, dealt from `seed`, instead of resuming one.
    #[must_use]
    pub fn played_by(mut self, agent: &str, opponent: &str, seed: u64) -> Self {
        let deck = beryl_cards::decks::default_deck();
        let start = StartGame {
            seed,
            decks: [deck.clone(), deck],
            human: Seat::P0,
            opponent: opponent.to_owned(),
            stops: StopsSetting::SMART,
        };
        self.played_by = Some((start, agent.to_owned()));
        self
    }

    /// The same sessions, which start a fresh game against the searching agent `--agent`
    /// plays against (`options::OPPONENT`), with the agent `name` answering the person's
    /// decisions in their place: for a phone's unattended runs, which measure drawing while
    /// the AI searches. (The expert, which this once played, plays by rules and never searches,
    /// so a game between it and a stand-in ended in a tenth of a second.) The seed is the
    /// clock's, as `Sessions::start` deals one.
    #[must_use]
    pub fn with_stand_in(self, name: &str) -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos() as u64);
        self.played_by(name, crate::options::OPPONENT, seed)
    }
}

/// One search of the opponent's: how long its decision took in milliseconds, search and all,
/// as the game timed it, and the rollouts and engine steps it spent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Searched {
    pub ms: u64,
    pub rollouts: u64,
    pub work: u64,
}

impl Searched {
    /// The search behind `decision`, or `None` when it made its answer without one.
    #[must_use]
    pub fn of(decision: &AgentMove) -> Option<Self> {
        let search = decision.search.as_ref()?;
        Some(Self {
            ms: decision.ms,
            rollouts: u64::from(search.rollouts),
            work: search.work,
        })
    }
}

/// What `searches` did, one line: how many there were, the rollouts and engine steps they
/// spent, and both a second of thinking. `None` when they took no time.
#[must_use]
pub fn search_report(searches: &[Searched]) -> Option<String> {
    let ms: u64 = searches.iter().map(|s| s.ms).sum();
    if ms == 0 {
        return None;
    }
    let rollouts: u64 = searches.iter().map(|s| s.rollouts).sum();
    let work: u64 = searches.iter().map(|s| s.work).sum();
    let seconds = ms as f64 / 1e3;
    Some(format!(
        "beryl3d: search: {} decisions searched in {seconds:.1} s, {rollouts} rollouts and \
         {work} engine steps: {:.0} rollouts/s, {:.0} engine steps/s",
        searches.len(),
        rollouts as f64 / seconds,
        work as f64 / seconds,
    ))
}

impl Session for Live {
    /// Starts the game `played_by` asked for, or otherwise resumes the newest unfinished game,
    /// as the web app does on launch. Then it sends the agents a person may play, which put the
    /// client on the start screen when no game has sent a view.
    fn begin(&mut self) {
        if let Some((start, agent)) = &self.played_by {
            if let Err(error) = self.sessions.start_game(start, Some(agent)) {
                eprintln!("beryl3d: the game did not start: {error:?}");
            }
        } else {
            beryl_shell::resume_latest(&self.sessions);
        }
        let mut agents = beryl_session::agents();
        agents.extend(self.offered.iter().cloned());
        self.inbox.send(Input::Agents(agents));
    }

    fn run(&mut self, command: SessionCommand) -> Reply {
        let sessions = &self.sessions;
        match command {
            SessionCommand::Click(request) => Reply::Click(sessions.click(request)),
            SessionCommand::Start(request) => Reply::Done(sessions.start(&request).map(|_| ())),
            SessionCommand::Resume => {
                beryl_shell::resume_latest(sessions);
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
            SessionCommand::TableSolo(request) => Reply::TableSolo(self.tables.solo(request)),
            SessionCommand::TableAct(action) => Reply::TableAct(self.tables.act(action)),
            SessionCommand::TableLeave => {
                Reply::TableLeave(self.tables.leave().map_err(|error| error.reason))
            }
            SessionCommand::TableLobby(change) => {
                let lobby = self.tables.lobby(TableLobbyRequest { change });
                Reply::TableLobby(lobby.map(|lobby| lobby.map(Box::new)))
            }
            SessionCommand::CardSearch(text) => {
                let hits = beryl_table_session::decks::search(&tokens::request(text.clone()));
                Reply::CardSearch(text, hits)
            }
            SessionCommand::DeckList => Reply::DeckList(decks::list(&self.decks)),
            SessionCommand::DeckImport(request) => {
                Reply::DeckImport(decks::import(&self.decks, request))
            }
        }
    }

    fn save_bug(&mut self, job: Job) -> Result<(), String> {
        job.write(Some(&self.sessions))
    }

    /// Shuts the manual table down so that it can be resumed, as the web app does on quit.
    /// With `--agent`, prints what the opponent's searches did (`search_report`), over the
    /// latest of its decisions the session keeps (200).
    fn end(&mut self) {
        self.tables.shutdown();
        if self.played_by.is_none() {
            return;
        }
        let searches: Vec<Searched> = self
            .sessions
            .agent_moves()
            .iter()
            .filter_map(Searched::of)
            .collect();
        let report = search_report(&searches);
        println!(
            "{}",
            report.unwrap_or_else(|| "beryl3d: search: nothing was searched".to_owned())
        );
    }
}
