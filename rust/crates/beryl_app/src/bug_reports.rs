//! Report export and sharing. The report folder is the durable record even if filing fails.

use beryl_session::SessionError;
#[cfg(desktop)]
use beryl_session::bugs;
#[cfg(desktop)]
use beryl_session::journal;
#[cfg(desktop)]
use serde_json::json;

#[cfg(desktop)]
#[tauri::command]
pub async fn export_bugs(app: tauri::AppHandle) -> Result<Option<u32>, SessionError> {
    use tauri::Manager;
    use tauri_plugin_dialog::DialogExt;

    tauri::async_runtime::spawn_blocking(move || {
        let folders =
            bugs::archive::active(&app.state::<crate::BugDirectory>().0).map_err(export_error)?;
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

#[cfg(mobile)]
#[tauri::command]
pub async fn share_bug(
    app: tauri::AppHandle,
    request: beryl_session::ipc::ShareBugRequest,
) -> Result<(), SessionError> {
    use tauri::Manager;

    let root = app.state::<crate::BugDirectory>().0.clone();
    let share = app
        .path()
        .app_cache_dir()
        .map_err(share_error)?
        .join("share");
    let path = tauri::async_runtime::spawn_blocking(move || {
        beryl_shell::reports::share_archive(&root, &request.id, &share)
    })
    .await
    .map_err(share_error)??;
    app.state::<beryl_mobile::Mobile<tauri::Wry>>()
        .share(path)
        .await
        .map_err(share_error)
}

#[cfg(mobile)]
fn share_error(error: impl std::fmt::Display) -> SessionError {
    SessionError::Io {
        reason: error.to_string(),
    }
}

#[cfg(desktop)]
#[tauri::command]
#[allow(clippy::unnecessary_wraps)]
pub fn share_bug(_request: beryl_session::ipc::ShareBugRequest) -> Result<(), SessionError> {
    Err(SessionError::Io {
        reason: "Share is available on a phone.".to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(desktop)]
    fn desktop_cannot_open_a_phone_share_sheet() {
        let error = share_bug(beryl_session::ipc::ShareBugRequest {
            id: "20261113-102030-report".to_owned(),
        })
        .unwrap_err();
        assert!(error.to_string().contains("Share is available on a phone."));
    }
}
