//! Tauri commands for the manual table. Its views go to the same event as engine games.

use beryl_session::ipc::{
    TableActReply, TableActRequest, TableRefusalReason, TableRefused, TableSoloRequest,
};
use beryl_session::{GameUpdate, SessionId, SessionIds};
use beryl_table_session::{TableEvent, TableSessions};
use tauri::{AppHandle, Emitter, Manager, State};

pub fn sessions(app: &AppHandle, ids: SessionIds) -> TableSessions {
    let game = app.clone();
    let app = app.clone();
    TableSessions::new(ids, move |update| crate::games::emit_update(&game, update)).with_events(
        move |event| {
            let _ = match event {
                TableEvent::Link(state) => app.emit("table-link", state),
                TableEvent::Error(reason) => app.emit(
                    "table-link",
                    serde_json::json!({ "kind": "error", "reason": reason }),
                ),
                TableEvent::Lobby(lobby) => app.emit("table-lobby", lobby),
            };
        },
    )
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
pub async fn table_act(app: AppHandle, request: TableActRequest) -> TableActReply {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<TableSessions>().act(request.action).into()
    })
    .await
    .unwrap_or(TableActReply::Refused(TableRefused {
        refused: TableRefusalReason::Stale,
        blocked_by: None,
    }))
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn table_current(tables: State<'_, TableSessions>) -> Option<GameUpdate> {
    tables.current()
}

#[tauri::command(async)]
#[allow(clippy::needless_pass_by_value)]
pub fn table_leave(tables: State<'_, TableSessions>) {
    tables.leave();
}
