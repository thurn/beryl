//! The app instance's folders: `beryl_shell`'s [`Dirs`] for the profile `BERYL_PROFILE` names,
//! under Tauri's data and log folders.

use beryl_shell::{Dirs, Profile};
use tauri::{AppHandle, Manager};

/// The profile's data, log and bug report folders.
pub fn dirs(app: &AppHandle) -> tauri::Result<Dirs> {
    let base = app.path().app_data_dir()?;
    // Tauri finds both from the same home folder, so the fallback is for a platform that has
    // no log folder.
    let logs = app
        .path()
        .app_log_dir()
        .unwrap_or_else(|_| base.join("logs"));
    Ok(Dirs::new(&base, logs, Profile::current()))
}

/// Opens the profile's journal, sends bug reports to its bugs folder, and returns its folders.
pub fn setup(app: &AppHandle) -> tauri::Result<Dirs> {
    let dirs = dirs(app)?;
    dirs.open();
    Ok(dirs)
}
