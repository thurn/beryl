//! Tauri commands for the manual table. Its views go to the same event as engine games.

use beryl_session::ipc::{TableActReply, TableActRequest, TableRefused, TableSoloRequest};
use beryl_session::{GameUpdate, SessionId, SessionIds};
use beryl_table_session::TableSessions;
use tauri::{AppHandle, State};

pub fn sessions(app: &AppHandle, ids: SessionIds) -> TableSessions {
    let app = app.clone();
    TableSessions::new(ids, move |update| crate::games::emit_update(&app, update))
}

#[tauri::command(async)]
#[allow(clippy::needless_pass_by_value)] // Tauri passes the managed state by value.
pub fn table_solo(
    tables: State<'_, TableSessions>,
    request: TableSoloRequest,
) -> Result<SessionId, TableRefused> {
    tables.solo(request.deck).map_err(TableRefused::from)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn table_act(tables: State<'_, TableSessions>, request: TableActRequest) -> TableActReply {
    tables.act(request.action).into()
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn table_current(tables: State<'_, TableSessions>) -> Option<GameUpdate> {
    tables.current()
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn table_leave(tables: State<'_, TableSessions>) {
    tables.leave();
}
