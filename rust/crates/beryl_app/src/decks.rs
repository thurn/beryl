//! Tauri commands for the profile's deck library and the bundled card store.

use beryl_session::SessionError;
use beryl_session::ipc::decks::{
    CardSearchHit, CardSearchRequest, CardTokenLink, CardTokensRequest, DeckDeleteRequest,
    DeckImportReply, DeckImportRequest, DeckOpenReply, SavedDeck,
};
use beryl_table_session::decks::{self, Decks};
use tauri::AppHandle;

pub(crate) fn library(app: &AppHandle) -> Result<Decks, SessionError> {
    let path = crate::profile::data_dir(app).map_err(io_error)?;
    Ok(Decks::new(path.join("decks")))
}

fn io_error(error: impl std::fmt::Display) -> SessionError {
    SessionError::Io {
        reason: error.to_string(),
    }
}

#[tauri::command]
pub async fn deck_import(
    app: AppHandle,
    request: DeckImportRequest,
) -> Result<DeckImportReply, SessionError> {
    let library = library(&app)?;
    tauri::async_runtime::spawn_blocking(move || decks::import(&library, request))
        .await
        .map_err(io_error)?
}

#[tauri::command]
pub async fn deck_list(app: AppHandle) -> Result<Vec<SavedDeck>, SessionError> {
    let library = library(&app)?;
    tauri::async_runtime::spawn_blocking(move || decks::list(&library))
        .await
        .map_err(io_error)?
}

#[tauri::command]
pub async fn deck_delete(app: AppHandle, request: DeckDeleteRequest) -> Result<(), SessionError> {
    let library = library(&app)?;
    tauri::async_runtime::spawn_blocking(move || decks::delete(&library, &request))
        .await
        .map_err(io_error)?
}

#[cfg(desktop)]
#[tauri::command]
pub async fn deck_open_file(app: AppHandle) -> Result<DeckOpenReply, SessionError> {
    use tauri_plugin_dialog::DialogExt;

    tauri::async_runtime::spawn_blocking(move || {
        let Some(file) = app
            .dialog()
            .file()
            .add_filter("Deck files", &["txt", "dec", "cod"])
            .blocking_pick_file()
        else {
            return Ok(DeckOpenReply::Canceled);
        };
        let path = file.into_path().map_err(io_error)?;
        decks::read_file(&path)
    })
    .await
    .map_err(io_error)?
}

#[cfg(mobile)]
#[tauri::command]
pub fn deck_open_file() -> DeckOpenReply {
    DeckOpenReply::Unavailable
}

#[tauri::command(async)]
#[allow(clippy::needless_pass_by_value)] // Tauri passes command arguments by value.
pub fn card_search(request: CardSearchRequest) -> Vec<CardSearchHit> {
    decks::search(&request)
}

#[tauri::command(async)]
#[allow(clippy::needless_pass_by_value)]
pub fn card_tokens(request: CardTokensRequest) -> Vec<CardTokenLink> {
    decks::tokens(&request)
}

#[cfg(test)]
mod tests;
