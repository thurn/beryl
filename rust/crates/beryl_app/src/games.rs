//! The game commands: Tauri wrappers around [`Sessions`]. Views reach the UI as
//! [`ipc::GAME_UPDATE`] events.

use beryl_session::ipc::{CancelRequest, ChooseRequest, SetStopsRequest};
use beryl_session::{AgentView, SessionError, SessionId, Sessions, StartRequest, ipc};
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
pub fn choose(sessions: State<'_, Sessions>, request: ChooseRequest) -> Result<(), SessionError> {
    sessions.choose(request.decision, request.index)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn cancel(sessions: State<'_, Sessions>, request: CancelRequest) -> Result<(), SessionError> {
    sessions.cancel(request.decision)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn set_stops(
    sessions: State<'_, Sessions>,
    request: SetStopsRequest,
) -> Result<(), SessionError> {
    sessions.set_stops(request.session, request.stops)
}
