//! Desktop report actions. The report folder is the durable record even if filing fails.

use beryl_session::SessionError;
#[cfg(desktop)]
use beryl_session::{bugs, journal};
#[cfg(desktop)]
use serde_json::json;

#[cfg(desktop)]
#[tauri::command]
pub async fn export_bugs(app: tauri::AppHandle) -> Result<Option<u32>, SessionError> {
    use tauri_plugin_dialog::DialogExt;

    tauri::async_runtime::spawn_blocking(move || {
        let folders = bugs::archive::active(&bugs::dir()).map_err(export_error)?;
        if folders.is_empty() {
            return Ok(Some(0));
        }
        let Some(path) = app
            .dialog()
            .file()
            .set_title("Export Bug Reports (screenshots, replays, logs)")
            .set_file_name("beryl-bug-reports.zip")
            .add_filter("ZIP archive", &["zip"])
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let path = path.into_path().map_err(export_error)?;
        bugs::archive::save(&folders, &path).map_err(export_error)?;
        Ok(Some(folders.len() as u32))
    })
    .await
    .map_err(export_error)?
}

#[cfg(desktop)]
fn export_error(error: impl std::fmt::Display) -> SessionError {
    let reason = error.to_string();
    journal::warn("bugs", format!("export failed: {reason}"), json!(null));
    SessionError::Io { reason }
}

#[cfg(mobile)]
#[tauri::command]
#[allow(clippy::unnecessary_wraps)]
pub fn export_bugs() -> Result<Option<u32>, SessionError> {
    Err(SessionError::Io {
        reason: "Export is available on desktop.".to_owned(),
    })
}
