//! `Sessions`: the games a process runs, one at a time (decision D12). It numbers sessions,
//! turns the UI's start request into a game, names replay files, and refuses commands for a
//! session that is no longer in progress. The app's commands and the CLI's fixture recorder
//! both drive games through it.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use beryl_cards::CARDS;
use beryl_cards::decks::WU_CONTROL;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::game::Game;
use crate::protocol::{CandidateIndex, DecisionId, SessionId, StopsSetting};
use crate::session::GameSession;
use crate::{GameUpdate, Prepared, Seat, SessionError, StartGame};

/// The UI's request for a game: the WU Control mirror against `agent`, the human in seat 0
/// under the smart stops.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct StartRequest {
    pub agent: String,
    /// The game's seed; none picks one from the clock.
    pub seed: Option<u32>,
}

impl StartRequest {
    pub fn game(&self) -> StartGame {
        let deck: Vec<(u8, String)> = WU_CONTROL
            .iter()
            .map(|&(n, name)| (n, name.to_owned()))
            .collect();
        StartGame {
            seed: self.seed.map_or_else(clock_seed, u64::from),
            decks: [deck.clone(), deck],
            human: Seat::P0,
            opponent: self.agent.clone(),
            stops: StopsSetting::SMART,
        }
    }
}

type Emit = dyn Fn(GameUpdate) + Send + Sync;

/// The game in progress, if any, and every view it emits, sent to one callback.
pub struct Sessions {
    /// Where replay files go, if anywhere.
    replays: Option<PathBuf>,
    emit: Arc<Emit>,
    current: Mutex<Current>,
    /// The latest view emitted, of whichever game emitted last.
    latest: Arc<Mutex<Option<GameUpdate>>>,
}

#[derive(Default)]
struct Current {
    /// The last session number given out; numbers only grow.
    last: u32,
    game: Option<(SessionId, GameSession)>,
}

impl Sessions {
    /// Sessions that write each game's replay into `replays`, if given, and send every view
    /// to `emit`.
    pub fn new(
        replays: Option<PathBuf>,
        emit: impl Fn(GameUpdate) + Send + Sync + 'static,
    ) -> Sessions {
        Sessions {
            replays,
            emit: Arc::new(emit),
            current: Mutex::default(),
            latest: Arc::default(),
        }
    }

    /// Starts the game the UI asked for, replacing any game in progress.
    pub fn start(&self, request: &StartRequest) -> Result<SessionId, SessionError> {
        self.start_game(&request.game(), None)
    }

    /// Starts `start`, replacing any game in progress. A `stand_in` agent answers the human's
    /// decisions in their place (UI fixtures); their views are emitted all the same.
    pub fn start_game(
        &self,
        start: &StartGame,
        stand_in: Option<&str>,
    ) -> Result<SessionId, SessionError> {
        let replay = self.replay_path(start.seed);
        self.launch(
            |session| Game::new(&CARDS, start, session),
            stand_in,
            replay,
        )
    }

    /// Starts a game from `prepared`, replacing any game in progress. It writes no replay,
    /// which could not reproduce it.
    pub fn start_prepared(&self, prepared: Prepared) -> Result<SessionId, SessionError> {
        self.launch(|session| Ok(Game::prepared(prepared, session)), None, None)
    }

    fn launch(
        &self,
        game: impl FnOnce(SessionId) -> Result<Game, SessionError>,
        stand_in: Option<&str>,
        replay: Option<PathBuf>,
    ) -> Result<SessionId, SessionError> {
        let mut current = self.lock();
        // Stop the old game before the new one can emit.
        current.game = None;
        current.last += 1;
        let session = SessionId(current.last);
        let mut game = game(session)?;
        if let Some(name) = stand_in {
            game.set_stand_in(name)?;
        }
        let emit = Arc::clone(&self.emit);
        let latest = Arc::clone(&self.latest);
        let game = GameSession::start(game, replay, move |view| {
            let update = GameUpdate { session, view };
            {
                let mut latest = latest.lock().unwrap_or_else(PoisonError::into_inner);
                // A replaced game's thread may emit once more as the next one starts.
                if latest.as_ref().is_none_or(|l| l.session <= session) {
                    *latest = Some(update.clone());
                }
            }
            emit(update);
        })?;
        current.game = Some((session, game));
        Ok(session)
    }

    /// The latest view of the game in progress, or of the one that ended last: what a UI that
    /// lost its views (a page reload) shows to take up the game where it is. None before the
    /// game's first view, or with no game.
    pub fn current(&self) -> Option<GameUpdate> {
        let current = self.lock();
        let (session, _) = current.game.as_ref()?;
        self.latest
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .filter(|u| u.session == *session)
    }

    /// The human concedes game `session` ([`crate::Game::concede`]).
    pub fn concede(&self, session: SessionId) -> Result<(), SessionError> {
        self.with(session, GameSession::concede)
    }

    /// Answers the human's open decision `decision` with its candidate `index`.
    pub fn choose(&self, decision: DecisionId, index: CandidateIndex) -> Result<(), SessionError> {
        self.with(decision.session, |game| game.choose(decision, index))
    }

    /// Takes back the cast the human's open decision `decision` is part of.
    pub fn cancel(&self, decision: DecisionId) -> Result<(), SessionError> {
        self.with(decision.session, |game| game.cancel(decision))
    }

    /// Goes back from the human's open decision `decision` to the start of their turn.
    pub fn undo(&self, decision: DecisionId) -> Result<(), SessionError> {
        self.with(decision.session, |game| game.undo(decision))
    }

    /// Switches the human's priority stops in game `session`.
    pub fn set_stops(&self, session: SessionId, stops: StopsSetting) -> Result<(), SessionError> {
        self.with(session, |game| game.set_stops(stops))
    }

    fn with(
        &self,
        session: SessionId,
        f: impl FnOnce(&GameSession) -> Result<(), SessionError>,
    ) -> Result<(), SessionError> {
        match &self.lock().game {
            Some((current, game)) if *current == session => f(game),
            _ => Err(SessionError::StaleSession { session }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Current> {
        self.current.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Where a game's replay goes: the replay directory, by start time and seed.
    fn replay_path(&self, seed: u64) -> Option<PathBuf> {
        let dir = self.replays.as_ref()?;
        if let Err(e) = std::fs::create_dir_all(dir) {
            eprintln!("beryl_session: {}: {e}", dir.display());
            return None;
        }
        Some(dir.join(format!("{}-{seed}.json", now().as_secs())))
    }
}

fn clock_seed() -> u64 {
    // The low 64 bits of the nanoseconds: any seed will do.
    #[allow(clippy::cast_possible_truncation)]
    let seed = now().as_nanos() as u64;
    seed
}

fn now() -> std::time::Duration {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
}
