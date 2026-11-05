//! The game commands: thin wrappers around `beryl_session`. Views reach the UI as
//! `game-update` events carrying a [`GameUpdate`].

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use beryl_cards::CARDS;
use beryl_cards::decks::WU_CONTROL;
use beryl_session::{AgentView, GameSession, GameUpdate, StartGame, StopsSetting};
use tauri::{AppHandle, Emitter, Manager, State};

/// The event every view arrives on.
const UPDATE: &str = "game-update";

/// The game in progress, if any: one at a time (decision D12).
#[derive(Default)]
pub struct Games {
    current: Mutex<Current>,
}

#[derive(Default)]
struct Current {
    /// The last session number given out; numbers only grow, so the UI can drop a stale view.
    session: u32,
    game: Option<GameSession>,
}

#[tauri::command]
pub fn list_agents() -> Vec<AgentView> {
    beryl_session::agents()
}

/// Starts a WU Control mirror against `agent`, replacing any game in progress, and returns
/// its session number. The human sits in seat 0.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri passes command arguments by value.
pub fn start_game(
    app: AppHandle,
    games: State<'_, Games>,
    agent: String,
    seed: Option<u32>,
) -> Result<u32, String> {
    let seed = seed.map_or_else(clock_seed, u64::from);
    let deck: Vec<(u8, String)> = WU_CONTROL
        .iter()
        .map(|&(n, name)| (n, name.to_owned()))
        .collect();
    let start = StartGame {
        seed,
        decks: [deck.clone(), deck],
        human: 0,
        opponent: agent,
        stops: StopsSetting::Smart,
    };
    let mut current = games.current.lock().map_err(|e| e.to_string())?;
    // Stop the old game before the new one can emit.
    current.game = None;
    current.session += 1;
    let session = current.session;
    let replay = replay_path(&app, seed);
    let emitter = app.clone();
    let game = GameSession::start(&CARDS, &start, replay, move |view| {
        if let Err(e) = emitter.emit(UPDATE, GameUpdate { session, view }) {
            eprintln!("beryl: {e}");
        }
    })
    .map_err(|e| e.to_string())?;
    current.game = Some(game);
    Ok(session)
}

/// Answers the human's decision `seq` in game `session` with its candidate `index`.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn choose(games: State<'_, Games>, session: u32, seq: u32, index: usize) -> Result<(), String> {
    with_game(&games, session, |g| g.choose(seq, index))
}

/// Switches the human's priority stops between the smart stops and every priority.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn set_stops(games: State<'_, Games>, session: u32, stops: StopsSetting) -> Result<(), String> {
    with_game(&games, session, |g| g.set_stops(stops))
}

fn with_game(
    games: &Games,
    session: u32,
    f: impl FnOnce(&GameSession) -> Result<(), beryl_session::SessionError>,
) -> Result<(), String> {
    let current = games.current.lock().map_err(|e| e.to_string())?;
    match &current.game {
        Some(game) if current.session == session => f(game).map_err(|e| e.to_string()),
        _ => Err(format!("game {session} is not in progress")),
    }
}

/// Where a game's replay goes: the app's data directory, by start time and seed.
fn replay_path(app: &AppHandle, seed: u64) -> Option<PathBuf> {
    let dir = app.path().app_data_dir().ok()?.join("replays");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("beryl: {}: {e}", dir.display());
        return None;
    }
    Some(dir.join(format!("{}-{seed}.json", now().as_secs())))
}

fn clock_seed() -> u64 {
    now().as_nanos() as u64
}

fn now() -> std::time::Duration {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
}
