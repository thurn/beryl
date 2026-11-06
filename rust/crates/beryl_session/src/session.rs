//! `GameSession`: a [`Game`] on its own thread (architecture §19.2), so agents think without
//! blocking IPC. The human's answers arrive by [`GameSession::choose`]; views leave through
//! the callback the session was started with.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};

use crate::SessionError;
use crate::game::{Answer, Game, OpenDecision};
use crate::protocol::{CandidateIndex, DecisionId, PlayerView, Status, StopsSetting};

/// A game against an agent, running on its own thread. Dropping it ends the thread once the
/// agent has finished its current decision, and nothing is emitted after that.
pub(crate) struct GameSession {
    commands: Sender<Command>,
    /// The human's open decision, as the thread last emitted it; none once it is answered.
    open: Arc<Mutex<Option<OpenDecision>>>,
    /// The game's stop flag ([`Game::stop_flag`]).
    stop: Arc<AtomicBool>,
    /// Set once the human concedes, so the thread concedes at the next decision even while
    /// the game plays on without them.
    conceded: Arc<AtomicBool>,
}

enum Command {
    Choose(Answer),
    SetStops(StopsSetting),
    Cancel,
    /// Wakes the thread waiting on the human to take their concession.
    Concede,
}

impl GameSession {
    /// Starts `game`. `emit` receives every view of the human's as the game plays
    /// ([`Game::next_view`]); the replay file at `replay`, if any, is rewritten before each
    /// view at which the human decides or the game ends.
    pub(crate) fn start(
        game: Game,
        replay: Option<PathBuf>,
        emit: impl FnMut(PlayerView) + Send + 'static,
    ) -> Result<GameSession, SessionError> {
        let stop = game.stop_flag();
        let (commands, received) = channel();
        let open = Arc::new(Mutex::new(None));
        let conceded = Arc::new(AtomicBool::new(false));
        let thread = Shared {
            open: Arc::clone(&open),
            conceded: Arc::clone(&conceded),
        };
        std::thread::Builder::new()
            .name("game session".to_owned())
            .spawn(move || run(game, &received, &thread, replay.as_deref(), emit))
            .map_err(|e| SessionError::Io {
                reason: e.to_string(),
            })?;
        Ok(GameSession {
            commands,
            open,
            stop,
            conceded,
        })
    }

    /// Answers the human's open decision `id` with its candidate `index`. It returns at once;
    /// the next view arrives through `emit`.
    pub(crate) fn choose(&self, id: DecisionId, index: CandidateIndex) -> Result<(), SessionError> {
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        let answer = open.ok_or(SessionError::NotDeciding)?.answer(id, index)?;
        *open = None;
        self.commands
            .send(Command::Choose(answer))
            .map_err(|_| SessionError::Ended)
    }

    /// Takes back the cast the human's open decision `id` is part of ([`Game::cancel`]). It
    /// returns at once; the view of their priority again arrives through `emit`.
    pub(crate) fn cancel(&self, id: DecisionId) -> Result<(), SessionError> {
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        open.ok_or(SessionError::NotDeciding)?.cancel(id)?;
        *open = None;
        self.commands
            .send(Command::Cancel)
            .map_err(|_| SessionError::Ended)
    }

    /// Changes the human's priority stops. It takes effect at their next decision, or at the
    /// open one, whose view is emitted again with the new setting.
    pub(crate) fn set_stops(&self, stops: StopsSetting) -> Result<(), SessionError> {
        self.commands
            .send(Command::SetStops(stops))
            .map_err(|_| SessionError::Ended)
    }

    /// The human concedes ([`Game::concede`]). It returns at once; the view of the game's end
    /// arrives through `emit` at the next decision, theirs or before the agent's next one.
    pub(crate) fn concede(&self) -> Result<(), SessionError> {
        self.conceded.store(true, Ordering::Relaxed);
        *self.open.lock().unwrap_or_else(PoisonError::into_inner) = None;
        self.commands
            .send(Command::Concede)
            .map_err(|_| SessionError::Ended)
    }
}

/// What the session shares with its thread.
struct Shared {
    open: Arc<Mutex<Option<OpenDecision>>>,
    conceded: Arc<AtomicBool>,
}

impl Drop for GameSession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The session thread: plays and emits each view, then waits for the human (or has the
/// stand-in answer). Commands are taken only while the human decides, so a stops change lands
/// at a definite point of the choice log.
fn run(
    mut game: Game,
    commands: &Receiver<Command>,
    shared: &Shared,
    replay: Option<&Path>,
    mut emit: impl FnMut(PlayerView),
) {
    let stop = game.stop_flag();
    // A stops change re-emits the open decision, which may already be answered, so it does
    // not reopen it. Returns whether the session is over: the session was dropped.
    let mut publish = |game: &Game, view: PlayerView, reopen: bool| {
        let deciding = matches!(view.status, Status::Deciding { .. });
        if let Some(path) = replay
            && (deciding || matches!(view.status, Status::Over { .. }))
            && let Err(e) = game.replay().save(path)
        {
            eprintln!("beryl_session: {e}");
        }
        // A concession closes the human's decisions for good.
        if reopen && deciding && !shared.conceded.load(Ordering::Relaxed) {
            *shared.open.lock().unwrap_or_else(PoisonError::into_inner) = game.open();
        }
        if stop.load(Ordering::Relaxed) {
            return true;
        }
        emit(view);
        false
    };
    loop {
        while let Some(view) = game.next_view() {
            if publish(&game, view, true) {
                return;
            }
            // The game plays on without the human: it takes their concession before the
            // agent's next decision.
            if shared.conceded.load(Ordering::Relaxed)
                && let Some(view) = game.concede()
            {
                publish(&game, view, false);
                return;
            }
        }
        if game.open().is_none() {
            // The game is over, or stopped.
            return;
        }
        loop {
            if game.answer_by_stand_in() {
                break;
            }
            let Ok(command) = commands.recv() else {
                return;
            };
            match command {
                // The session checked the answer against the open decision, which only this
                // thread changes.
                Command::Choose(answer) => {
                    game.choose(answer);
                    break;
                }
                // The session checked that the open decision can be taken back.
                Command::Cancel => match game.cancel() {
                    Ok(view) => {
                        if publish(&game, view, true) {
                            return;
                        }
                    }
                    Err(e) => eprintln!("beryl_session: {e}"),
                },
                Command::Concede => {
                    if let Some(view) = game.concede() {
                        publish(&game, view, false);
                    }
                    return;
                }
                Command::SetStops(stops) => match game.set_stops(stops) {
                    Ok(view) => {
                        if publish(&game, view, false) {
                            return;
                        }
                    }
                    Err(e) => eprintln!("beryl_session: {e}"),
                },
            }
        }
    }
}
