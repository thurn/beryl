//! `Sessions`: the games a process runs, one at a time (decision D12). It numbers sessions,
//! turns the UI's start request into a game, names replay files, and refuses commands for a
//! session that is no longer in progress. The app's commands and the CLI's fixture recorder
//! both drive games through it.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use beryl_cards::CARDS;
use beryl_cards::decks::WU_CONTROL;
use serde::{Deserialize, Serialize};
use serde_json::json;
use ts_rs::TS;

use crate::ai::{self, AgentMove, AgentMoves};
use crate::game::Game;
use crate::ipc::ChooseRequest;
use crate::protocol::{CandidateIndex, DecisionId, PlayerView, SessionId, Status, StopsSetting};
use crate::replay::ReplayFile;
use crate::session::GameSession;
use crate::{GameUpdate, Prepared, Seat, SessionError, StartGame, bugs, journal};

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

/// A game's replay as of its latest decision, which a bug report copies.
pub(crate) type LatestReplay = Arc<Mutex<Option<ReplayFile>>>;

#[derive(Default)]
struct Current {
    /// The last session number given out; numbers only grow.
    last: u32,
    game: Option<(SessionId, GameSession)>,
    /// The replay of the game in progress, if it was dealt from a seed: a prepared game has
    /// none that could reproduce it.
    replay: Option<LatestReplay>,
    /// The agent's latest decisions in the game in progress ([`crate::ai`]).
    moves: Option<AgentMoves>,
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
        let about = json!({
            "seed": start.seed,
            "opponent": start.opponent,
            "human": start.human as u8,
            "standIn": stand_in,
            "replay": replay,
        });
        self.launch(
            |session| Ok((Game::new(&CARDS, start, session)?, None)),
            stand_in,
            replay,
            true,
            about,
        )
    }

    /// Starts a game from `prepared`, replacing any game in progress. It writes no replay,
    /// which could not reproduce it.
    pub fn start_prepared(&self, prepared: Prepared) -> Result<SessionId, SessionError> {
        self.launch(
            |session| Ok((Game::prepared(prepared, session), None)),
            None,
            None,
            false,
            json!({ "prepared": true }),
        )
    }

    /// Starts a prepared game whose two seats are both played manually after setup.
    pub fn start_review(
        &self,
        prepared: Prepared,
        emit: crate::ReviewViews,
    ) -> Result<SessionId, SessionError> {
        self.launch(
            |session| {
                let mut game = Game::prepared(prepared, session);
                game.review(emit);
                Ok((game, None))
            },
            None,
            None,
            false,
            json!({ "review": true }),
        )
    }

    /// Resumes the newest game in the replay directory if it is unfinished: what the app
    /// takes up on launch. With a game already in progress, it resumes nothing and returns
    /// that game's session. None with nothing to resume: no replay directory or file, the
    /// newest game is over, or its file no longer replays (an engine change broke it, say),
    /// which is journaled. Only the newest file counts: starting a game concedes the one in
    /// progress, so no older one is unfinished but by a crash, and finding one would mean
    /// replaying every game ever played.
    pub fn resume_latest(&self) -> Option<SessionId> {
        if let Some((session, _)) = &self.lock().game {
            return Some(*session);
        }
        let path = newest_replay(self.replays.as_ref()?)?;
        let resumed = ReplayFile::load(&path).and_then(|file| {
            // Replayed apart first, so an ended game is left alone without a session.
            let (_, view) = Game::from_replay(&CARDS, &file, SessionId(0))?;
            if matches!(view.status, Status::Over { .. }) {
                return Ok(None);
            }
            self.resume_file(&file, &path).map(Some)
        });
        resumed.unwrap_or_else(|e| {
            journal::warn(
                "session",
                format!("the latest game does not resume: {e}"),
                json!({ "replay": path, "error": e }),
            );
            None
        })
    }

    /// Resumes `file`, read from `path`, replacing any game in progress: it replays the record
    /// to where it ends ([`Game::from_replay`]), emits the view there and plays on, saving to
    /// `path` still.
    fn resume_file(&self, file: &ReplayFile, path: &Path) -> Result<SessionId, SessionError> {
        let commit = build_commit();
        if file.commit.as_deref() != Some(commit) {
            journal::info(
                "session",
                "resuming a game another build wrote",
                json!({ "replay": path, "wrote": file.commit, "resumes": commit }),
            );
        }
        let about = json!({
            "seed": file.seed,
            "opponent": file.opponent,
            "human": file.human,
            "replay": path,
            "resumed": file.log.len(),
        });
        self.launch(
            |session| {
                let (game, view) = Game::from_replay(&CARDS, file, session)?;
                Ok((game, Some(view)))
            },
            None,
            Some(path.to_owned()),
            true,
            about,
        )
    }

    /// Starts a game, writing its replay file to `replay`, if given, and emitting first the
    /// view the game was built with, if any (a resumed game's). A `seeded` game keeps its
    /// latest replay for a bug report and records this build in it; a prepared game has none
    /// that would reproduce it.
    fn launch(
        &self,
        game: impl FnOnce(SessionId) -> Result<(Game, Option<PlayerView>), SessionError>,
        stand_in: Option<&str>,
        replay: Option<PathBuf>,
        seeded: bool,
        mut about: serde_json::Value,
    ) -> Result<SessionId, SessionError> {
        let mut current = self.lock();
        // Stop the old game before the new one can emit.
        if let Some((old, _)) = current.game.take() {
            journal::info("session", "game replaced", json!({ "session": old.0 }));
        }
        current.replay = None;
        current.moves = None;
        current.last += 1;
        let session = SessionId(current.last);
        about["session"] = json!(session.0);
        let started = game(session).and_then(|(mut game, first)| {
            if let Some(name) = stand_in {
                game.set_stand_in(name)?;
            }
            if seeded {
                game.set_commit(build_commit().to_owned());
            }
            Ok((game, first))
        });
        let (game, first) = match started {
            Ok(game) => game,
            Err(e) => {
                about["error"] = json!(e.to_string());
                journal::warn("session", "game failed to start", about);
                return Err(e);
            }
        };
        journal::info("session", "game started", about);
        let moves = game.agent_moves();
        let latest_replay = seeded.then(LatestReplay::default);
        let emit = Arc::clone(&self.emit);
        let latest = Arc::clone(&self.latest);
        let game = GameSession::start(game, first, replay, latest_replay.clone(), move |view| {
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
        current.replay = latest_replay;
        current.moves = Some(moves);
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

    /// The replay of the game in progress, or of the one that ended last, as of its latest
    /// decision: none for a prepared game, or before the first decision.
    pub fn current_replay(&self) -> Option<ReplayFile> {
        let replay = self.lock().replay.clone()?;
        replay
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The agent's latest decisions in the game in progress, or the one that ended last,
    /// oldest first, each with its search.
    pub fn agent_moves(&self) -> Vec<AgentMove> {
        self.lock()
            .moves
            .as_ref()
            .map(ai::latest)
            .unwrap_or_default()
    }

    /// The human concedes game `session` ([`crate::Game::concede`]).
    pub fn concede(&self, session: SessionId) -> Result<(), SessionError> {
        self.with(
            "concede",
            json!({ "session": session.0 }),
            session,
            GameSession::concede,
        )
    }

    /// Answers the human's open decision `decision` with its candidate `index`.
    pub fn choose(&self, decision: DecisionId, index: CandidateIndex) -> Result<(), SessionError> {
        self.choose_request(ChooseRequest {
            decision,
            index,
            continuation: None,
        })
    }

    /// Answers the decision, optionally as part of the player's preceding choice.
    pub fn choose_request(&self, request: ChooseRequest) -> Result<(), SessionError> {
        let ChooseRequest {
            decision,
            index,
            continuation,
        } = request;
        let data = json!({ "decision": decision.seq, "index": index.0, "continuation": continuation.unwrap_or(false) });
        self.with("choose", data, decision.session, |game| {
            game.choose(request)
        })
    }

    /// Takes back the cast the human's open decision `decision` is part of.
    pub fn cancel(&self, decision: DecisionId) -> Result<(), SessionError> {
        let data = json!({ "decision": decision.seq });
        self.with("cancel", data, decision.session, |game| {
            game.cancel(decision)
        })
    }

    /// Takes back the human's latest answer from their open decision `decision`.
    pub fn undo(&self, decision: DecisionId) -> Result<(), SessionError> {
        let data = json!({ "decision": decision.seq });
        self.with("undo", data, decision.session, |game| game.undo(decision))
    }

    /// Passes on from the human's open decision `decision` to their next stop.
    pub fn pass_on(&self, decision: DecisionId) -> Result<(), SessionError> {
        let data = json!({ "decision": decision.seq });
        self.with("pass_on", data, decision.session, |game| {
            game.pass_on(decision)
        })
    }

    /// Switches the human's priority stops in game `session`.
    pub fn set_stops(&self, session: SessionId, stops: StopsSetting) -> Result<(), SessionError> {
        let data = json!({ "stops": stops });
        self.with("set_stops", data, session, |game| game.set_stops(stops))
    }

    /// Runs the human's `command` on game `session`, journaling it with `data` and how it
    /// went: a refused command is how a UI out of step with the game shows itself, though
    /// most are a double click, so they go to the journal without a warning.
    fn with(
        &self,
        command: &str,
        mut data: serde_json::Value,
        session: SessionId,
        f: impl FnOnce(&GameSession) -> Result<(), SessionError>,
    ) -> Result<(), SessionError> {
        let result = match &self.lock().game {
            Some((current, game)) if *current == session => f(game),
            _ => Err(SessionError::StaleSession { session }),
        };
        data["session"] = json!(session.0);
        match &result {
            Ok(()) => journal::debug("command", command, data),
            Err(e) => {
                data["error"] = json!(e);
                journal::info("command", format!("{command} refused: {e}"), data);
            }
        }
        result
    }

    fn lock(&self) -> MutexGuard<'_, Current> {
        self.current.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Where a game's replay goes: the replay directory, by start time and seed.
    fn replay_path(&self, seed: u64) -> Option<PathBuf> {
        let dir = self.replays.as_ref()?;
        if let Err(e) = std::fs::create_dir_all(dir) {
            journal::error(
                "session",
                format!("no replay: {}: {e}", dir.display()),
                serde_json::Value::Null,
            );
            return None;
        }
        Some(dir.join(format!("{}-{seed}.json", now().as_secs())))
    }
}

/// The newest replay file in `dir`, by the start time that begins its name.
fn newest_replay(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.extension()? != "json" {
                return None;
            }
            let (start, _) = path.file_stem()?.to_str()?.split_once('-')?;
            Some((start.parse::<u64>().ok()?, path))
        })
        .max()
        .map(|(_, path)| path)
}

/// The commit of this build, as replay files record it: `-dirty` marks a checkout with
/// uncommitted changes.
fn build_commit() -> &'static str {
    static COMMIT: OnceLock<String> = OnceLock::new();
    COMMIT.get_or_init(|| match bugs::commit() {
        (commit, true) => format!("{commit}-dirty"),
        (commit, false) => commit,
    })
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
