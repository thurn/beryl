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
}

enum Command {
    Choose(Answer),
    SetStops(StopsSetting),
}

impl GameSession {
    /// Starts `game`. `emit` receives the human's view at each of their decisions and once
    /// more at the end of the game; the replay file at `replay`, if any, is rewritten before
    /// each view is emitted.
    pub(crate) fn start(
        game: Game,
        replay: Option<PathBuf>,
        emit: impl FnMut(PlayerView) + Send + 'static,
    ) -> Result<GameSession, SessionError> {
        let stop = game.stop_flag();
        let (commands, received) = channel();
        let open = Arc::new(Mutex::new(None));
        let shared = Arc::clone(&open);
        std::thread::Builder::new()
            .name("game session".to_owned())
            .spawn(move || run(game, &received, &shared, replay.as_deref(), emit))
            .map_err(|e| SessionError::Io {
                reason: e.to_string(),
            })?;
        Ok(GameSession {
            commands,
            open,
            stop,
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

    /// Changes the human's priority stops. It takes effect at their next decision, or at the
    /// open one, whose view is emitted again with the new setting.
    pub(crate) fn set_stops(&self, stops: StopsSetting) -> Result<(), SessionError> {
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

/// The session thread: plays, emits, then waits for the human (or has the stand-in answer).
/// Commands are taken only while the human decides, so a stops change lands at a definite
/// point of the choice log.
fn run(
    mut game: Game,
    commands: &Receiver<Command>,
    open: &Mutex<Option<OpenDecision>>,
    replay: Option<&Path>,
    mut emit: impl FnMut(PlayerView),
) {
    let stop = game.stop_flag();
    // A stops change re-emits the open decision, which may already be answered, so it does
    // not reopen it. Returns whether the session is over: the game ended or stopped, or the
    // session was dropped.
    let mut publish = |game: &Game, view: PlayerView, reopen: bool| {
        if let Some(path) = replay
            && let Err(e) = game.replay().save(path)
        {
            eprintln!("beryl_session: {e}");
        }
        if reopen {
            *open.lock().unwrap_or_else(PoisonError::into_inner) = game.open();
        }
        if stop.load(Ordering::Relaxed) {
            return true;
        }
        let over = !matches!(view.status, Status::Deciding { .. });
        emit(view);
        over
    };
    let mut view = game.begin();
    let mut reopen = true;
    loop {
        if publish(&game, view, reopen) {
            return;
        }
        (view, reopen) = loop {
            if let Some(view) = game.answer_by_stand_in() {
                break (view, true);
            }
            let Ok(command) = commands.recv() else {
                return;
            };
            match command {
                // The session checked the answer against the open decision, which only this
                // thread changes.
                Command::Choose(answer) => break (game.choose(answer), true),
                Command::SetStops(stops) => match game.set_stops(stops) {
                    Ok(view) => break (view, false),
                    Err(e) => eprintln!("beryl_session: {e}"),
                },
            }
        };
    }
}
