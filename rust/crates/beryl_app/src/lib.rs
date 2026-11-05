//! The Tauri shell: windows, IPC commands, the card-art cache and the smoke mode agents and CI
//! launch the app in. Games run in `beryl_session` ([`games`]).

mod art;
mod games;

use std::time::Duration;

use serde::Serialize;
use tauri::Manager;
use tauri::http::{Response, StatusCode};

use crate::art::ArtCache;

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
        .manage(games::Games::default())
        .invoke_handler(tauri::generate_handler![
            app_info,
            ui_ready,
            games::list_agents,
            games::start_game,
            games::choose,
            games::set_stops,
        ])
        .register_asynchronous_uri_scheme_protocol(art::SCHEME, |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let path = request.uri().path().to_owned();
            // A miss waits on the download queue: off the main thread.
            tauri::async_runtime::spawn_blocking(move || {
                // `setup` manages the cache before the window's page can ask for art.
                let response = match app.try_state::<ArtCache>() {
                    Some(cache) => cache.respond(&path),
                    None => Response::builder()
                        .status(StatusCode::SERVICE_UNAVAILABLE)
                        .body(Vec::new())
                        .expect("a static response is valid"),
                };
                responder.respond(response);
            });
        })
        .setup(|app| {
            let dir = app.path().app_cache_dir()?.join("art");
            app.manage(ArtCache::new(dir, art::SCRYFALL_IMAGES));
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
