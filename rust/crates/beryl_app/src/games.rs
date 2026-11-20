//! The game commands: Tauri wrappers around [`Sessions`]. Views reach the UI as
//! [`ipc::GAME_UPDATE`] events.

use beryl_session::ipc::{
    CancelRequest, ChooseRequest, ConcedeRequest, PassOnRequest, SetStopsRequest, UndoRequest,
};
use beryl_session::prompt::{ClickReply, ClickRequest};
use beryl_session::{AgentView, GameUpdate, SessionError, SessionId, Sessions, StartRequest, ipc};
use beryl_shell::Dirs;
use serde_json::json;
use tauri::{AppHandle, Emitter, State};

/// The sessions, writing replays and the ledger into the profile's data folder and emitting
/// views to the UI.
pub fn sessions(app: &AppHandle, dirs: &Dirs) -> Sessions {
    let app = app.clone();
    beryl_shell::sessions(&beryl_cards::CARDS, dirs, move |update| {
        emit_update(&app, update)
    })
}

pub(super) fn emit_update(app: &AppHandle, update: GameUpdate) {
    if let Err(e) = app.emit(ipc::GAME_UPDATE, update) {
        beryl_session::journal::error("app", format!("view not sent: {e}"), json!(null));
    }
}

#[tauri::command]
pub fn list_agents() -> Vec<AgentView> {
    beryl_session::agents()
}

/// The timing of the steps the stage makes itself while a prompt is open.
#[tauri::command]
pub fn stage_timings() -> beryl_session::script::OwnTimings {
    beryl_session::script::own_timings()
}

// Async, so it runs off the main thread: a fair deal's search takes seconds and would
// otherwise freeze the window.
#[tauri::command(async)]
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
    sessions.choose_request(request)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn click(
    sessions: State<'_, Sessions>,
    request: ClickRequest,
) -> Result<ClickReply, SessionError> {
    sessions.click(request)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn cancel(sessions: State<'_, Sessions>, request: CancelRequest) -> Result<(), SessionError> {
    sessions.cancel(request.decision)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn undo(sessions: State<'_, Sessions>, request: UndoRequest) -> Result<(), SessionError> {
    sessions.undo(request.decision)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn pass_on(sessions: State<'_, Sessions>, request: PassOnRequest) -> Result<(), SessionError> {
    sessions.pass_on(request.decision)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn concede(sessions: State<'_, Sessions>, request: ConcedeRequest) -> Result<(), SessionError> {
    sessions.concede(request.session)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn current_game(sessions: State<'_, Sessions>) -> Option<GameUpdate> {
    sessions.current()
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn resume_game(sessions: State<'_, Sessions>) -> Option<SessionId> {
    beryl_shell::resume_latest(&sessions)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn set_stops(
    sessions: State<'_, Sessions>,
    request: SetStopsRequest,
) -> Result<(), SessionError> {
    sessions.set_stops(request.session, request.stops)
}
