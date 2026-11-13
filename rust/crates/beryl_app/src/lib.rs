//! The Tauri shell: windows, IPC commands, the card-art cache and the smoke mode agents and CI
//! launch the app in. Games run in `beryl_session` ([`games`]). The process journals to
//! `~/Library/Logs/ca.thurn.beryl/beryl.jsonl` ([`beryl_session::journal`]); bug reports go
//! where [`beryl_session::bugs::dir`] says.

mod art;
mod bug_reports;
mod games;
mod links;
mod snapshot;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use beryl_session::bugs::{self, BugReport, BugSaved, Images, UiLogRequest};
use beryl_session::card_records::{CardRecords, CardRecordsRequest};
use beryl_session::ipc::{self, AppInfo, Platform};
use beryl_session::{SessionError, Sessions, journal};
use serde_json::json;
use tauri::Manager;
use tauri::http::{Response, StatusCode};

use crate::art::ArtCache;
use crate::snapshot::Snapshots;

/// Set by `just smoke`: run without a visible window or Dock icon and exit once the UI reports
/// that it loaded. The number of seconds is only a hang limit: the app exits with an error if
/// the UI has not reported by then.
const SMOKE_ENV: &str = "BERYL_SMOKE";

/// An Android app inherits no environment: `just android-smoke` sets this system property
/// instead of [`SMOKE_ENV`].
#[cfg(target_os = "android")]
const SMOKE_PROPERTY: &str = "debug.beryl.smoke";

struct BugDirectory(PathBuf);

#[cfg_attr(desktop, allow(clippy::unused_async))] // Only phones await a native plugin.
async fn report_environment(_app: &tauri::AppHandle) -> bugs::Environment {
    let environment = bugs::Environment::current();
    #[cfg(mobile)]
    let environment = match _app
        .state::<beryl_mobile::Mobile<tauri::Wry>>()
        .device()
        .await
    {
        Ok(device) => bugs::Environment {
            device: Some(bugs::Device {
                os: device.os,
                model: device.model,
                simulator: device.simulator,
            }),
            ..environment
        },
        Err(e) => {
            journal::warn("app", format!("no device details: {e}"), json!(null));
            environment
        }
    };
    environment
}

#[tauri::command]
async fn app_info(app: tauri::AppHandle) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        smoke: smoke_seconds().is_some(),
        platform: Platform::current(),
        simulator: report_environment(&app)
            .await
            .device
            .is_some_and(|device| device.simulator),
    }
}

/// The UI calls this once it has rendered; in smoke mode that ends the run, successfully once
/// the window snapshot a bug report freezes works too, timed.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri passes command arguments by value.
fn ui_ready(app: tauri::AppHandle) {
    if smoke_seconds().is_none() {
        return;
    }
    println!("beryl: ui ready");
    tauri::async_runtime::spawn(async move {
        let started = Instant::now();
        let result = match snapshot::capture(&app).await {
            Ok(id) => {
                let captured = started.elapsed();
                app.state::<Snapshots>()
                    .pngs(id, None, 1.0)
                    .map(|images| (captured, images))
            }
            Err(e) => Err(e),
        };
        let status = match result {
            Ok((
                captured,
                Images {
                    window: Some(png), ..
                },
            )) if png.starts_with(b"\x89PNG") => {
                let message = format!(
                    "window snapshot in {} ms, its PNG ({} bytes) by {} ms",
                    captured.as_millis(),
                    png.len(),
                    started.elapsed().as_millis()
                );
                println!("beryl: {message}");
                journal::info("app", message, json!(null));
                journal::info("app", "ui ready", json!(null));
                0
            }
            failure => {
                let message = match failure {
                    Ok(_) => "the window snapshot made no PNG".to_owned(),
                    Err(e) => format!("no window snapshot: {e}"),
                };
                eprintln!("beryl: {message}");
                journal::error("app", message, json!(null));
                1
            }
        };
        // iOS does not support app.exit: tao panics, so the process ends itself.
        #[cfg(mobile)]
        std::process::exit(status);
        #[cfg(desktop)]
        app.exit(status);
    });
}

fn smoke_seconds() -> Option<u64> {
    let value = std::env::var(SMOKE_ENV).ok();
    #[cfg(target_os = "android")]
    let value = value.or_else(|| {
        let out = std::process::Command::new("/system/bin/getprop")
            .arg(SMOKE_PROPERTY)
            .output()
            .ok()?;
        let value = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        (!value.is_empty()).then_some(value)
    });
    value.map(|v| v.parse().unwrap_or(300))
}

/// Snapshots the window as drawn now and returns its number ([`ipc::CAPTURE_WINDOW`]).
#[tauri::command]
async fn capture_window(app: tauri::AppHandle) -> Result<u32, SessionError> {
    let started = Instant::now();
    match snapshot::capture(&app).await {
        Ok(id) => {
            journal::debug(
                "app",
                "window snapshot",
                json!({ "id": id, "ms": started.elapsed().as_millis() }),
            );
            Ok(id)
        }
        Err(reason) => {
            journal::warn("app", format!("no window snapshot: {reason}"), json!(null));
            Err(SessionError::Io { reason })
        }
    }
}

/// Writes a bug report ([`ipc::REPORT_BUG`]), off the main thread, its images made from the
/// snapshot it names.
#[tauri::command]
async fn report_bug(app: tauri::AppHandle, request: BugReport) -> Result<BugSaved, SessionError> {
    let started = Instant::now();
    let images = match request.snapshot {
        Some(id) => app
            .state::<Snapshots>()
            .pngs(id, request.region, request.page.width)
            .unwrap_or_else(|e| {
                journal::warn("bugs", format!("no images: {e}"), json!(null));
                Images::default()
            }),
        None => Images::default(),
    };
    let encoded = started.elapsed();
    let environment = report_environment(&app).await;
    let sessions = app.state::<Sessions>();
    let root = app.state::<BugDirectory>();
    let saved =
        bugs::write(&root.0, &request, &images, &sessions, &environment).inspect_err(|e| {
            journal::error("bugs", format!("report not written: {e}"), json!(null));
        })?;
    journal::debug(
        "bugs",
        "report timing",
        json!({ "pngMs": encoded.as_millis(), "totalMs": started.elapsed().as_millis() }),
    );
    #[cfg(desktop)]
    {
        // A local bundle carries its filing tools so Finder can launch it without a shell.
        let bundled = match (
            option_env!("BERYL_BUNDLE_BUGS_REPO"),
            option_env!("BERYL_BUNDLE_BUGS_UV"),
            option_env!("BERYL_BUNDLE_BUGS_PATH"),
        ) {
            (Some(repo), Some(uv), Some(path)) => Some((repo, uv, path)),
            _ => None,
        };
        bugs::filing::enqueue(&root.0, &saved.id, bundled);
    }
    Ok(saved)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn ui_log(request: UiLogRequest) {
    bugs::ui_log(request);
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
fn card_records(request: CardRecordsRequest) -> CardRecords {
    beryl_session::card_records::card_records(&request)
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

/// The app's entry point: `main.rs`'s on desktop, and on iOS the one the Xcode project
/// (`gen/apple`) calls.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    journal::install_panic_hook();
    bugs::set_build_commit(env!("BERYL_COMMIT"));
    let builder = tauri::Builder::default().plugin(links::plugin());
    #[cfg(mobile)]
    let builder = builder.plugin(beryl_mobile::init());
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_dialog::init());
    builder
        .invoke_handler(handlers![
            app_info = ipc::APP_INFO,
            ui_ready = ipc::UI_READY,
            games::list_agents = ipc::LIST_AGENTS,
            games::start_game = ipc::START_GAME,
            games::choose = ipc::CHOOSE,
            games::click = ipc::CLICK,
            games::set_stops = ipc::SET_STOPS,
            games::cancel = ipc::CANCEL,
            games::undo = ipc::UNDO,
            games::pass_on = ipc::PASS_ON,
            games::concede = ipc::CONCEDE,
            games::current_game = ipc::CURRENT_GAME,
            games::resume_game = ipc::RESUME_GAME,
            capture_window = ipc::CAPTURE_WINDOW,
            report_bug = ipc::REPORT_BUG,
            bug_reports::export_bugs = ipc::EXPORT_BUGS,
            bug_reports::share_bug = ipc::SHARE_BUG,
            ui_log = ipc::UI_LOG,
            card_records = ipc::CARD_RECORDS,
        ])
        .register_asynchronous_uri_scheme_protocol(art::SCHEME, |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let path = request.uri().path().to_owned();
            // A miss waits on the download queue: off the main thread.
            tauri::async_runtime::spawn_blocking(move || {
                // The bug report's frozen frame (snapshot.rs) shares the scheme.
                if path.starts_with(snapshot::PATH)
                    && let Some(snapshots) = app.try_state::<Snapshots>()
                {
                    responder.respond(snapshots.respond(&path));
                    return;
                }
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
            // A phone's data directory has no fixed path: reports go in its own.
            #[cfg(mobile)]
            let bug_dir = {
                let default = app.path().app_data_dir()?.join("bugs");
                bugs::set_dir(default.clone());
                std::env::var_os("BERYL_BUGS").map_or(default, PathBuf::from)
            };
            #[cfg(desktop)]
            let bug_dir = bugs::dir();
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
                json!({ "smoke": smoke_seconds().is_some(), "bugs": bug_dir }),
            );
            app.manage(games::sessions(app.handle()));
            app.manage(Snapshots::default());
            app.manage(BugDirectory(bug_dir));
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
