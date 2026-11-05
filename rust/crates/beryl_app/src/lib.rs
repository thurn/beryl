//! The Tauri shell: windows, IPC commands and the smoke mode agents and CI launch the app in.

use std::time::Duration;

use serde::Serialize;
use tauri::Manager;

/// Set by `just smoke`: run without a visible window or Dock icon and exit once the UI reports
/// that it loaded, or with an error after this many seconds.
const SMOKE_ENV: &str = "BERYL_SMOKE";

#[derive(Debug, Serialize)]
struct AppInfo {
    version: &'static str,
    smoke: bool,
}

#[tauri::command]
fn app_info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        smoke: smoke_seconds().is_some(),
    }
}

/// The UI calls this once it has rendered; in smoke mode that ends the run successfully.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri passes command arguments by value.
fn ui_ready(app: tauri::AppHandle) {
    if smoke_seconds().is_some() {
        println!("beryl: ui ready");
        app.exit(0);
    }
}

fn smoke_seconds() -> Option<u64> {
    std::env::var(SMOKE_ENV)
        .ok()
        .map(|v| v.parse().unwrap_or(20))
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![app_info, ui_ready])
        .setup(|app| {
            // The main window starts hidden (tauri.conf.json) so a smoke run never shows it.
            let Some(seconds) = smoke_seconds() else {
                if let Some(window) = app.get_webview_window("main") {
                    window.show()?;
                }
                return Ok(());
            };
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Prohibited);
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(seconds));
                eprintln!("beryl: the ui did not report ready within {seconds} s");
                std::process::exit(1);
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the Tauri application");
}
