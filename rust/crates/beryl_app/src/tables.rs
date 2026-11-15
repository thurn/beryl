//! Tauri commands for the manual table. Its views go to the same event as engine games.

use std::path::PathBuf;

use beryl_session::ipc::{
    Platform, SavedTable, TableActReply, TableActRequest, TableConnectionError, TableHostReply,
    TableHostRequest, TableJoinRequest, TableRefusalReason, TableRefused, TableResumeRequest,
    TableSettings, TableSoloRequest,
};
use beryl_session::{GameUpdate, SessionId, SessionIds};
use beryl_table_session::connections::TableConnections;
use tauri::{AppHandle, Emitter, Manager, State};

pub fn sessions(app: &AppHandle, ids: SessionIds, data_dir: PathBuf) -> TableConnections {
    let game = app.clone();
    let app = app.clone();
    TableConnections::new(
        ids,
        data_dir,
        move |update| crate::games::emit_update(&game, update),
        move |event| {
            let (name, payload) = event.into_message();
            let _ = app.emit(name, payload);
        },
    )
}

#[tauri::command(async)]
#[allow(clippy::needless_pass_by_value)] // Tauri passes the managed state by value.
pub fn table_solo(
    tables: State<'_, TableConnections>,
    request: TableSoloRequest,
) -> Result<SessionId, TableRefused> {
    tables.solo(request)
}

#[tauri::command]
pub async fn table_act(app: AppHandle, request: TableActRequest) -> TableActReply {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<TableConnections>().act(request.action)
    })
    .await
    .unwrap_or(TableActReply::Refused(TableRefused {
        refused: TableRefusalReason::Stale,
        blocked_by: None,
    }))
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn table_current(tables: State<'_, TableConnections>) -> Option<GameUpdate> {
    tables.current()
}

#[tauri::command]
pub async fn table_leave(app: AppHandle) {
    let _ =
        tauri::async_runtime::spawn_blocking(move || app.state::<TableConnections>().leave()).await;
}

#[tauri::command]
pub async fn table_host(
    app: AppHandle,
    request: TableHostRequest,
) -> Result<TableHostReply, TableConnectionError> {
    run(app, move |tables| tables.host(Platform::current(), request)).await
}

#[tauri::command]
pub async fn table_join(
    app: AppHandle,
    request: TableJoinRequest,
) -> Result<SessionId, TableConnectionError> {
    run(app, move |tables| tables.join(request)).await
}

#[tauri::command]
pub async fn table_resume(
    app: AppHandle,
    request: TableResumeRequest,
) -> Result<TableHostReply, TableConnectionError> {
    run(app, move |tables| {
        tables.resume(Platform::current(), &request)
    })
    .await
}

#[tauri::command]
pub async fn table_saved(app: AppHandle) -> Result<Vec<SavedTable>, TableConnectionError> {
    run(app, TableConnections::saved).await
}

#[tauri::command]
pub async fn table_settings_get(app: AppHandle) -> Result<TableSettings, TableConnectionError> {
    run(app, TableConnections::settings).await
}

#[tauri::command]
pub async fn table_settings_set(
    app: AppHandle,
    request: TableSettings,
) -> Result<(), TableConnectionError> {
    run(app, move |tables| tables.set_settings(&request)).await
}

async fn run<T: Send + 'static>(
    app: AppHandle,
    operation: impl FnOnce(&TableConnections) -> Result<T, TableConnectionError> + Send + 'static,
) -> Result<T, TableConnectionError> {
    tauri::async_runtime::spawn_blocking(move || operation(&app.state::<TableConnections>()))
        .await
        .map_err(TableConnectionError::from_error)?
}
