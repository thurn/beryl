//! The Tauri shell: windows, IPC commands, the card-art cache and the smoke mode agents and CI
//! launch the app in. Games run in `beryl_session` ([`games`]).

mod art;
mod games;

use std::time::Duration;

use beryl_session::ipc::{self, AppInfo};
use tauri::Manager;
use tauri::http::{Response, StatusCode};

use crate::art::ArtCache;

/// Set by `just smoke`: run without a visible window or Dock icon and exit once the UI reports
/// that it loaded, or with an error after this many seconds.
const SMOKE_ENV: &str = "BERYL_SMOKE";

#[tauri::command]
fn app_info() -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").to_owned(),
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

/// The Tauri handler for the commands, each `function = name`: Tauri names a command for its
/// function, so each must be named as `beryl_session::ipc` (and so `ui/src/ipc.ts`) calls it.
macro_rules! handlers {
    ($($function:path = $name:expr),* $(,)?) => {{
        $(const _: () = assert!(
            names(stringify!($function), $name),
            "a command's function is named as beryl_session::ipc names the command"
        );)*
        tauri::generate_handler![$($function),*]
    }};
}

/// Whether the function at `path` is named `name`: its last segment.
const fn names(path: &str, name: &str) -> bool {
    let (path, name) = (path.as_bytes(), name.as_bytes());
    if name.len() > path.len() {
        return false;
    }
    let start = path.len() - name.len();
    let mut i = 0;
    while i < name.len() {
        if path[start + i] != name[i] {
            return false;
        }
        i += 1;
    }
    start == 0 || path[start - 1] == b':' || path[start - 1] == b' '
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(handlers![
            app_info = ipc::APP_INFO,
            ui_ready = ipc::UI_READY,
            games::list_agents = ipc::LIST_AGENTS,
            games::start_game = ipc::START_GAME,
            games::choose = ipc::CHOOSE,
            games::set_stops = ipc::SET_STOPS,
            games::cancel = ipc::CANCEL,
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
            app.manage(games::sessions(app.handle()));
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
