//! The game commands: Tauri wrappers around [`Sessions`]. Views reach the UI as
//! [`ipc::GAME_UPDATE`] events.

use beryl_session::{
    AgentView, CandidateIndex, DecisionId, SessionError, SessionId, Sessions, StartRequest,
    StopsSetting, ipc,
};
use tauri::{AppHandle, Emitter, Manager, State};

/// The sessions, writing replays into the app's data directory and emitting views to the UI.
pub fn sessions(app: &AppHandle) -> Sessions {
    let replays = app.path().app_data_dir().ok().map(|d| d.join("replays"));
    let app = app.clone();
    Sessions::new(replays, move |update| {
        if let Err(e) = app.emit(ipc::GAME_UPDATE, update) {
            eprintln!("beryl: {e}");
        }
    })
}

#[tauri::command]
pub fn list_agents() -> Vec<AgentView> {
    beryl_session::agents()
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri passes command arguments by value.
pub fn start_game(
    sessions: State<'_, Sessions>,
    request: StartRequest,
) -> Result<SessionId, SessionError> {
    sessions.start(&request)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn choose(
    sessions: State<'_, Sessions>,
    decision: DecisionId,
    index: CandidateIndex,
) -> Result<(), SessionError> {
    sessions.choose(decision, index)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn set_stops(
    sessions: State<'_, Sessions>,
    session: SessionId,
    stops: StopsSetting,
) -> Result<(), SessionError> {
    sessions.set_stops(session, stops)
}
