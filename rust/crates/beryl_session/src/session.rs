//! `GameSession`: a [`Game`] on its own thread (architecture §19.2), so agents think without
//! blocking IPC. The human's answers arrive by [`GameSession::choose`]; views leave through
//! the callback the session was started with.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};

use beryl_engine::CardTable;

use crate::game::Game;
use crate::protocol::{PlayerView, StopsSetting};
use crate::{SessionError, StartGame};

/// A game against an agent, running on its own thread. Dropping it ends the thread once the
/// agent has finished its current decision, and nothing is emitted after that.
pub struct GameSession {
    commands: Sender<Command>,
    /// The human's open decision: its number and how many candidates it has.
    open: Arc<Mutex<Option<(u32, usize)>>>,
    /// The game's stop flag ([`Game::stop_flag`]).
    stop: Arc<AtomicBool>,
}

enum Command {
    Choose { seq: u32, index: usize },
    SetStops(StopsSetting),
}

impl GameSession {
    /// Starts a game. `emit` receives the human's view at each of their decisions and once
    /// more at the end of the game; the replay file at `replay`, if any, is rewritten before
    /// each view is emitted.
    pub fn start(
        cards: &'static CardTable,
        start: &StartGame,
        replay: Option<PathBuf>,
        emit: impl FnMut(PlayerView) + Send + 'static,
    ) -> Result<GameSession, SessionError> {
        let game = Game::new(cards, start)?;
        let stop = game.stop_flag();
        let (commands, received) = channel();
        let open = Arc::new(Mutex::new(None));
        let shared = Arc::clone(&open);
        std::thread::Builder::new()
            .name("game session".to_owned())
            .spawn(move || run(game, &received, &shared, replay.as_deref(), emit))
            .map_err(|e| SessionError::Io(e.to_string()))?;
        Ok(GameSession {
            commands,
            open,
            stop,
        })
    }

    /// Answers the human's open decision `seq` with its candidate `index`. It returns at once;
    /// the next view arrives through `emit`.
    pub fn choose(&self, seq: u32, index: usize) -> Result<(), SessionError> {
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        match *open {
            None => return Err(SessionError::NotDeciding),
            Some((current, _)) if current != seq => {
                return Err(SessionError::StaleDecision { seq, open: current });
            }
            Some((_, candidates)) if index >= candidates => {
                return Err(SessionError::NoSuchCandidate { index, candidates });
            }
            Some(_) => *open = None,
        }
        self.commands
            .send(Command::Choose { seq, index })
            .map_err(|_| SessionError::Ended)
    }

    /// Changes the human's priority stops. It takes effect at their next decision, or at the
    /// open one, whose view is emitted again with the new setting.
    pub fn set_stops(&self, stops: StopsSetting) -> Result<(), SessionError> {
        self.commands
            .send(Command::SetStops(stops))
            .map_err(|_| SessionError::Ended)
    }
}

impl Drop for GameSession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The session thread: plays, emits, then waits for the human. Commands are taken only while
/// the human decides, so a stops change lands at a definite point of the choice log.
fn run(
    mut game: Game,
    commands: &Receiver<Command>,
    open: &Mutex<Option<(u32, usize)>>,
    replay: Option<&Path>,
    mut emit: impl FnMut(PlayerView),
) {
    let stop = game.stop_flag();
    // A stops change re-emits the open decision, which may already be answered. Returns
    // whether the session is over: the game ended, or the session was dropped.
    let mut publish = |game: &Game, view: PlayerView, reopen: bool| {
        if let Some(path) = replay
            && let Err(e) = game.replay().save(path)
        {
            eprintln!("beryl_session: {e}");
        }
        if reopen {
            *open.lock().unwrap_or_else(PoisonError::into_inner) = view
                .decision
                .as_ref()
                .map(|d| (view.seq, d.candidates.len()));
        }
        if stop.load(Ordering::Relaxed) {
            return true;
        }
        let over = view.outcome.is_some();
        emit(view);
        over
    };
    let view = game.begin();
    if publish(&game, view, true) {
        return;
    }
    while let Ok(command) = commands.recv() {
        let (view, reopen) = match command {
            Command::Choose { seq, index } => (game.choose(seq, index), true),
            Command::SetStops(stops) => (game.set_stops(stops), false),
        };
        // `choose` checked the answer against the open decision, which only this thread
        // changes; a stops change while none is open has nothing to apply to.
        match view {
            Ok(view) => {
                if publish(&game, view, reopen) {
                    return;
                }
            }
            Err(e) => eprintln!("beryl_session: {e}"),
        }
    }
}
