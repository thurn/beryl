//! The Tauri shell: windows, IPC commands, the card-art cache and the smoke mode agents and CI
//! launch the app in. Games run in `beryl_session` ([`games`]). The process journals to
//! `~/Library/Logs/ca.thurn.beryl/beryl.jsonl` ([`beryl_session::journal`]); bug reports go
//! where [`beryl_session::bugs::dir`] says.

mod art;
mod games;
mod snapshot;

use std::time::Duration;

use beryl_session::bugs::{self, BugReport, BugSaved, UiLogRequest};
use beryl_session::ipc::{self, AppInfo};
use beryl_session::{SessionError, Sessions, journal};
use serde_json::json;
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

/// The UI calls this once it has rendered; in smoke mode that ends the run, successfully once
/// the window snapshot a bug report freezes works too.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri passes command arguments by value.
fn ui_ready(app: tauri::AppHandle) {
    if smoke_seconds().is_none() {
        return;
    }
    println!("beryl: ui ready");
    tauri::async_runtime::spawn(async move {
        match snapshot::capture(&app).await {
            Ok(png) if png.starts_with(b"\x89PNG") => {
                println!("beryl: window snapshot, {} bytes", png.len());
                app.exit(0);
            }
            Ok(_) => {
                eprintln!("beryl: the window snapshot is not a PNG");
                app.exit(1);
            }
            Err(e) => {
                eprintln!("beryl: no window snapshot: {e}");
                app.exit(1);
            }
        }
    });
}

fn smoke_seconds() -> Option<u64> {
    std::env::var(SMOKE_ENV)
        .ok()
        .map(|v| v.parse().unwrap_or(20))
}

/// The window as drawn now, a PNG in base64 ([`ipc::CAPTURE_WINDOW`]).
#[tauri::command]
async fn capture_window(app: tauri::AppHandle) -> Result<String, SessionError> {
    match snapshot::capture(&app).await {
        Ok(png) => Ok(bugs::encode_base64(&png)),
        Err(reason) => {
            journal::warn("app", format!("no window snapshot: {reason}"), json!(null));
            Err(SessionError::Io { reason })
        }
    }
}

/// Writes a bug report ([`ipc::REPORT_BUG`]), off the main thread.
#[tauri::command]
async fn report_bug(app: tauri::AppHandle, request: BugReport) -> Result<BugSaved, SessionError> {
    let sessions = app.state::<Sessions>();
    bugs::write(&bugs::dir(), &request, &sessions).inspect_err(|e| {
        journal::error("bugs", format!("report not written: {e}"), json!(null));
    })
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn ui_log(request: UiLogRequest) {
    bugs::ui_log(request);
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
    journal::install_panic_hook();
    tauri::Builder::default()
        .invoke_handler(handlers![
            app_info = ipc::APP_INFO,
            ui_ready = ipc::UI_READY,
            games::list_agents = ipc::LIST_AGENTS,
            games::start_game = ipc::START_GAME,
            games::choose = ipc::CHOOSE,
            games::set_stops = ipc::SET_STOPS,
            games::cancel = ipc::CANCEL,
            games::undo = ipc::UNDO,
            games::pass_on = ipc::PASS_ON,
            games::concede = ipc::CONCEDE,
            games::current_game = ipc::CURRENT_GAME,
            capture_window = ipc::CAPTURE_WINDOW,
            report_bug = ipc::REPORT_BUG,
            ui_log = ipc::UI_LOG,
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
            match app.path().app_log_dir() {
                Ok(dir) => {
                    if let Err(e) = journal::open(&dir.join("beryl.jsonl")) {
                        journal::warn("app", format!("no journal file: {e}"), json!(null));
                    }
                }
                Err(e) => journal::warn("app", format!("no log directory: {e}"), json!(null)),
            }
            journal::info(
                "app",
                "started",
                json!({ "smoke": smoke_seconds().is_some(), "bugs": bugs::dir() }),
            );
            app.manage(games::sessions(app.handle()));
            let dir = app.path().app_cache_dir()?.join("art");
            app.manage(ArtCache::new(dir, art::SCRYFALL));
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
                journal::error(
                    "app",
                    format!("the ui did not report ready within {seconds} s"),
                    json!(null),
                );
                std::process::exit(1);
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the Tauri application");
}
