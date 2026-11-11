//! `GameSession`: a [`Game`] on its own thread (architecture §19.2), so agents think without
//! blocking IPC. The human's answers arrive by [`GameSession::choose`]; views leave through
//! the callback the session was started with.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::json;

use crate::game::{Answer, Game, OpenDecision};
use crate::ipc::{ChooseRequest, WaterbendPreview, WaterbendRequest};
use crate::prompt::{ClickReply, ClickRequest};
use crate::protocol::{DecisionId, PlayerView, Status, StopsSetting};
use crate::sessions::LatestReplay;
use crate::{SessionError, journal};

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
    PreviewWaterbend(
        WaterbendRequest,
        Sender<Result<Option<WaterbendPreview>, SessionError>>,
    ),
    Choose(Answer),
    Click(ClickRequest, Sender<Result<ClickReply, SessionError>>),
    SetStops(StopsSetting),
    Cancel,
    Undo,
    /// Pass on to the human's next stop, beginning with this pass.
    PassOn(Answer),
    /// Wakes the thread waiting on the human to take their concession.
    Concede,
}

/// The name of the thread each game plays on.
pub const THREAD: &str = "game session";

impl GameSession {
    /// Starts `game`. `emit` receives every view of the human's as the game plays
    /// ([`Game::next_view`]), beginning with `first`, the view where the game already stands,
    /// if given (a resumed game's); the replay file at `replay`, if any, is rewritten before
    /// each view at which the human decides or the game ends, and so is `latest`, if given.
    pub(crate) fn start(
        game: Game,
        first: Option<PlayerView>,
        replay: Option<PathBuf>,
        latest: Option<LatestReplay>,
        emit: impl FnMut(PlayerView) + Send + 'static,
    ) -> Result<GameSession, SessionError> {
        let stop = game.stop_flag();
        let (commands, received) = channel();
        let open = Arc::new(Mutex::new(None));
        let conceded = Arc::new(AtomicBool::new(false));
        let thread = Shared {
            open: Arc::clone(&open),
            conceded: Arc::clone(&conceded),
            latest,
        };
        std::thread::Builder::new()
            .name(THREAD.to_owned())
            .spawn(move || {
                let session = game.session();
                let end = run(game, first, &received, &thread, replay.as_deref(), emit);
                journal::info(
                    "session",
                    "thread ended",
                    json!({ "session": session.0, "why": end }),
                );
            })
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
    pub(crate) fn choose(&self, request: ChooseRequest) -> Result<(), SessionError> {
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        let answer = open
            .ok_or(SessionError::NotDeciding)?
            .answer(request.decision, request.index)?;
        let answer = if request.continuation == Some(true) {
            answer.as_continuation()
        } else {
            answer
        };
        *open = None;
        self.commands
            .send(Command::Choose(answer))
            .map_err(|_| SessionError::Ended)
    }

    /// Takes a click on the prompt of the human's open decision ([`Game::click`]) and returns
    /// the prompt's state after it. A click that answers closes the decision, and the next
    /// view arrives through `emit`.
    pub(crate) fn click(&self, request: ClickRequest) -> Result<ClickReply, SessionError> {
        self.open
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .ok_or(SessionError::NotDeciding)?
            .answer(request.decision, crate::CandidateIndex(0))?;
        let (reply, received) = channel();
        self.commands
            .send(Command::Click(request, reply))
            .map_err(|_| SessionError::Ended)?;
        received.recv().map_err(|_| SessionError::Ended)?
    }

    /// Computes a payment while leaving the open decision and game untouched.
    pub(crate) fn preview_waterbend(
        &self,
        request: WaterbendRequest,
    ) -> Result<Option<WaterbendPreview>, SessionError> {
        self.open
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .ok_or(SessionError::NotDeciding)?
            .answer(request.decision, crate::CandidateIndex(0))?;
        let (reply, received) = channel();
        self.commands
            .send(Command::PreviewWaterbend(request, reply))
            .map_err(|_| SessionError::Ended)?;
        received.recv().map_err(|_| SessionError::Ended)?
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

    /// Takes back the human's latest answer from their open decision `id` ([`Game::undo`]).
    /// It returns at once; the view of the decision it answered arrives through `emit`.
    pub(crate) fn undo(&self, id: DecisionId) -> Result<(), SessionError> {
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        open.ok_or(SessionError::NotDeciding)?.undo(id)?;
        *open = None;
        self.commands
            .send(Command::Undo)
            .map_err(|_| SessionError::Ended)
    }

    /// Passes on from the human's open decision `id` to their next stop ([`Game::pass_on`]).
    /// It returns at once; the next view arrives through `emit`.
    pub(crate) fn pass_on(&self, id: DecisionId) -> Result<(), SessionError> {
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        let answer = open.ok_or(SessionError::NotDeciding)?.pass_on(id)?;
        *open = None;
        self.commands
            .send(Command::PassOn(answer))
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
    latest: Option<LatestReplay>,
}

impl Drop for GameSession {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The session thread: plays and emits each view, then waits for the human (or has the
/// stand-in answer). Commands are taken only while the human decides, so a stops change lands
/// at a definite point of the choice log. Returns why it ended, for the journal.
fn run(
    mut game: Game,
    first: Option<PlayerView>,
    commands: &Receiver<Command>,
    shared: &Shared,
    replay: Option<&Path>,
    mut emit: impl FnMut(PlayerView),
) -> &'static str {
    let stop = game.stop_flag();
    // A stops change re-emits the open decision, which may already be answered, so it does
    // not reopen it. Returns whether the session is over: the session was dropped.
    let mut publish = |game: &mut Game, view: PlayerView, reopen: bool| {
        let open = game.open();
        let decides = open.is_some() || matches!(view.status, Status::Over { .. });
        if let Some(path) = replay
            && decides
            && let Err(e) = game.replay().save(path)
        {
            journal::error("session", format!("replay not saved: {e}"), json!(null));
        }
        if let Some(latest) = &shared.latest
            && decides
        {
            *latest.lock().unwrap_or_else(PoisonError::into_inner) = Some(game.replay().clone());
        }
        journal::debug("view", view_kind(&view), view_summary(game, &view));
        // A concession closes the human's decisions for good.
        if reopen && open.is_some() && !shared.conceded.load(Ordering::Relaxed) {
            *shared.open.lock().unwrap_or_else(PoisonError::into_inner) = open;
        }
        if stop.load(Ordering::Relaxed) {
            return true;
        }
        game.publish_review(&view);
        emit(view);
        false
    };
    if let Some(view) = first
        && publish(&mut game, view, true)
    {
        return "replaced";
    }
    loop {
        while let Some(view) = game.next_view() {
            if publish(&mut game, view, true) {
                return "replaced";
            }
            // The game plays on without the human: it takes their concession before the
            // agent's next decision.
            if shared.conceded.load(Ordering::Relaxed)
                && let Some(view) = game.concede()
            {
                publish(&mut game, view, false);
                return "conceded";
            }
        }
        if game.open().is_none() {
            // The game is over, or stopped.
            return "over";
        }
        loop {
            if game.answer_by_stand_in() {
                break;
            }
            let Ok(command) = commands.recv() else {
                return "closed";
            };
            match command {
                Command::PreviewWaterbend(request, response) => {
                    let _ = response.send(game.preview_waterbend(&request));
                }
                // A click that answers its prompt plays the game on.
                Command::Click(request, to) if click(&mut game, shared, request, &to) => break,
                Command::Click(..) => {}
                // The session checked the answer against the open decision, which only this
                // thread changes.
                Command::Choose(answer) => {
                    game.choose(answer);
                    break;
                }
                Command::PassOn(answer) => {
                    game.pass_on(answer);
                    break;
                }
                // The session checked that the open decision can go back.
                Command::Undo => match game.undo() {
                    Ok(view) => {
                        if publish(&mut game, view, true) {
                            return "replaced";
                        }
                    }
                    Err(e) => refused(&e),
                },
                Command::Cancel => match game.cancel() {
                    Ok(view) => {
                        if publish(&mut game, view, true) {
                            return "replaced";
                        }
                    }
                    Err(e) => refused(&e),
                },
                Command::Concede => {
                    if let Some(view) = game.concede() {
                        publish(&mut game, view, false);
                    }
                    return "conceded";
                }
                Command::SetStops(stops) => match game.set_stops(stops) {
                    Ok(view) => {
                        if publish(&mut game, view, false) {
                            return "replaced";
                        }
                    }
                    Err(e) => refused(&e),
                },
            }
        }
    }
}

/// Takes a click on the game's open prompt and tells its sender what it did. Returns whether
/// it answered the prompt: the game then plays on.
fn click(
    game: &mut Game,
    shared: &Shared,
    request: ClickRequest,
    response: &Sender<Result<ClickReply, SessionError>>,
) -> bool {
    let clicked = game.click(request);
    let answered = matches!(clicked, Ok(ClickReply::Answered));
    if answered {
        // The decision is closed before the click's sender hears so.
        *shared.open.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }
    let _ = response.send(clicked);
    answered
}

/// A command the session accepted that the game then refused: the two disagree on what is open.
fn refused(e: &SessionError) {
    journal::error(
        "session",
        format!("the game refused a command: {e}"),
        json!(e),
    );
}

fn view_kind(view: &PlayerView) -> &'static str {
    match view.status {
        Status::Deciding { .. } => "deciding",
        Status::Thinking => "thinking",
        Status::Playing => "playing",
        Status::Stopped => "stopped",
        Status::Over { .. } => "over",
    }
}

/// Where a view stands, in a line of the journal: enough to find it in the replay.
fn view_summary(game: &Game, view: &PlayerView) -> serde_json::Value {
    let mut summary = json!({
        "session": game.session().0,
        "log": game.replay().log.len(),
        "turn": view.turn.number,
        "step": view.turn.step,
        "events": view.events.len(),
        "stack": view.stack.len(),
    });
    if let Status::Deciding { decision } = &view.status {
        summary["decision"] = json!(decision.id.seq);
        summary["kind"] = json!(decision.kind);
        summary["candidates"] = json!(decision.candidates.len());
    }
    if view.undone {
        summary["undone"] = json!(true);
    }
    summary
}
