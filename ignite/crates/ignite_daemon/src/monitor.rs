//! `ignite monitor`: a one-shot health check that the login service runs every two minutes, in
//! a program of its own, so it works when the daemon is down or crash-looping.
//!
//! Each run asks the daemon whether it is alive (a `status` request with a short timeout, the
//! only thing it uses the daemon's socket for), remembers the answer in `monitor.json` in the
//! home folder and, using only files and the login service:
//!
//! - kickstarts the daemon when its process is gone or three runs in a row got no answer, and
//!   marks a run that did not answer hung in `starts.jsonl`, so that it still counts as a crash
//!   although the kickstart stops it with `SIGTERM`;
//! - goes back to the last-good build when `starts.jsonl` shows the current build crash-looping
//!   (the same decision a starting daemon makes, in [`crate::update::detect_crash_loop`]);
//! - after 15 minutes of failing in spite of kickstarts, writes `justiciar-needed.json` and
//!   posts a notification, because a person or a repair session has to look;
//! - on the hub's machine, where the hub is a login service of its own, kickstarts the hub when
//!   two runs in a row got no HTTP answer from it ([`decide_hub`]). The login service already
//!   restarts a hub that exits; this catches one that runs and does not answer;
//! - rewrites the daemon's login service when it was written by an older build
//!   ([`Service::refresh_daemon`]), on a run where the daemon answered and the monitor runs the
//!   daemon's own build. An update replaces the daemon's executable but not its login service:
//!   on 2026-11-18 the daemon's commands still ran on the slower cores hours after a release
//!   changed the service to put them on the faster ones, because nobody had run `ignite install`.
//!
//! - while the daemon stays down after that, on a machine that runs an app in CLI mode, starts
//!   and then resumes a headless justiciar turn, a repair session with no person watching
//!   ([`justiciar`]).
//!
//! The justiciar turn is the monitor's only agent logic, and it calls the hub only to see that
//! it answers. Each decision ([`decide`], [`decide_hub`], [`justiciar::decide`]) is a function
//! of what was observed; the observations and actions are the [`Machine`] trait, so the tests
//! run the decisions against a fake. A run reports a problem it handled and exits successfully.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::time::{SystemClock, Timestamp};
use ignite_core::traits::Clock;
use ignite_os::Native;
use ignite_os::service::Service;
use serde::{Deserialize, Serialize};

use crate::update::{CrashLoop, Home, detect_crash_loop, roll_back, start_log};

#[path = "monitor_justiciar.rs"]
pub mod justiciar;

/// This many runs in a row without an answer from the daemon get it kickstarted.
pub const FAILURES_BEFORE_KICKSTART: u32 = 3;
/// The daemon is kickstarted at most this often, so that one still starting up is not killed.
pub const KICKSTART_GAP_S: f64 = 300.0;
/// Failing this long, kickstarts included, asks for a repair.
pub const ESCALATE_AFTER_S: f64 = 900.0;
/// A repair request that has gone unanswered is posted again after this long.
pub const RENOTIFY_AFTER_S: f64 = 3600.0;
/// How long the daemon, or the hub, has to answer the probe.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// This many runs in a row without an answer from the hub get it kickstarted.
pub const HUB_FAILURES_BEFORE_KICKSTART: u32 = 2;

/// What the monitor remembers between runs: `<home>/monitor.json`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MonitorState {
    /// The last run at which the daemon answered.
    pub last_ok: Option<Timestamp>,
    pub consecutive_failures: u32,
    /// The first run of the current run of failures.
    pub failing_since: Option<Timestamp>,
    pub last_kickstart: Option<Timestamp>,
    /// Kickstarts since the failures began.
    pub kickstarts: u32,
    /// When a repair was last asked for.
    pub escalated_at: Option<Timestamp>,
    /// Runs in a row in which the hub, on the hub's machine, did not answer.
    pub hub_failures: u32,
    pub last_hub_kickstart: Option<Timestamp>,
}

/// What one run saw.
#[derive(Clone, Debug)]
pub struct Observation {
    /// Whether the login service has a daemon process.
    pub running: bool,
    /// Whether the daemon answered the probe.
    pub answered: bool,
    pub crash_loop: Option<CrashLoop>,
}

/// What one run does about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Go back to the last-good build, then kickstart the daemon into it.
    RollBack(CrashLoop),
    Kickstart,
    /// Write `justiciar-needed.json` and notify.
    AskForRepair,
}

fn seconds_since(then: Timestamp, now: Timestamp) -> f64 {
    (now.millis() - then.millis()) as f64 / 1000.0
}

/// Updates `state` for a run at `now` and returns what to do. The state already counts the
/// returned kickstarts and repair request as done.
pub fn decide(state: &mut MonitorState, seen: &Observation, now: Timestamp) -> Vec<Action> {
    let mut actions = Vec::new();
    if seen.running && seen.answered {
        state.last_ok = Some(now);
        state.consecutive_failures = 0;
        state.failing_since = None;
        state.kickstarts = 0;
        state.escalated_at = None;
    } else {
        state.consecutive_failures += 1;
        state.failing_since.get_or_insert(now);
    }
    if let Some(looped) = &seen.crash_loop {
        // The rollback restarts the daemon, which starts counting afresh.
        state.consecutive_failures = 0;
        state.last_kickstart = Some(now);
        state.kickstarts += 1;
        actions.push(Action::RollBack(looped.clone()));
        return actions;
    }
    let due = !seen.running || state.consecutive_failures >= FAILURES_BEFORE_KICKSTART;
    let rested = state
        .last_kickstart
        .is_none_or(|last| seconds_since(last, now) >= KICKSTART_GAP_S);
    if due && rested {
        state.last_kickstart = Some(now);
        state.kickstarts += 1;
        actions.push(Action::Kickstart);
    }
    let failing_long = state
        .failing_since
        .is_some_and(|since| seconds_since(since, now) >= ESCALATE_AFTER_S);
    let unasked = state
        .escalated_at
        .is_none_or(|at| seconds_since(at, now) >= RENOTIFY_AFTER_S);
    if state.kickstarts > 0 && failing_long && unasked {
        state.escalated_at = Some(now);
        actions.push(Action::AskForRepair);
    }
    actions
}

/// Updates `state` for a run at `now` that saw whether this machine has the hub's login service
/// (`here`) and whether the hub answered, and says whether to kickstart the hub. Like the
/// daemon, the hub is kickstarted at most every [`KICKSTART_GAP_S`] seconds, so one that is
/// still starting is not killed.
pub fn decide_hub(state: &mut MonitorState, here: bool, answered: bool, now: Timestamp) -> bool {
    if !here || answered {
        state.hub_failures = 0;
        return false;
    }
    state.hub_failures += 1;
    let rested = state
        .last_hub_kickstart
        .is_none_or(|last| seconds_since(last, now) >= KICKSTART_GAP_S);
    if state.hub_failures >= HUB_FAILURES_BEFORE_KICKSTART && rested {
        state.last_hub_kickstart = Some(now);
        return true;
    }
    false
}

/// What the monitor observes and does outside the home folder's files.
pub trait Machine {
    /// Whether the login service has a daemon process now.
    fn daemon_running(&self) -> bool;
    /// Whether the daemon answers a `status` request in time.
    fn probe(&self) -> bool;
    /// Kills the daemon and starts it again through the login service.
    fn kickstart(&self) -> std::io::Result<()>;
    /// Posts a desktop notification.
    fn notify(&self, title: &str, text: &str);
    /// Whether this machine has the hub's login service.
    fn hub_here(&self) -> bool;
    /// Whether the hub answers an HTTP request in time.
    fn probe_hub(&self) -> bool;
    /// Kills the hub and starts it again through its login service.
    fn kickstart_hub(&self) -> std::io::Result<()>;
    /// Rewrites and reloads the daemon's login service if an older build wrote it, and says
    /// whether it did ([`Service::refresh_daemon`]).
    fn refresh_daemon(&self) -> std::io::Result<bool>;
    /// The first app this machine runs in CLI mode, with what a turn of it needs, or `None` on a
    /// machine with none ([`justiciar::first_cli_app`]).
    fn justiciar_app(&self) -> Option<justiciar::JusticiarApp>;
    /// Whether a process holds the lock file `lock`.
    fn locked(&self, lock: &Path) -> bool;
    /// Starts a headless justiciar turn as a detached process that holds its lock while the CLI
    /// runs.
    fn start_justiciar(&self, turn: &justiciar::JusticiarTurn) -> std::io::Result<()>;
    /// The last `lines` lines of the daemon's log, or an empty text when it cannot be read.
    fn daemon_log_tail(&self, lines: usize) -> String;
}

fn state_file(home: &Home) -> PathBuf {
    home.root.join("monitor.json")
}

fn load(home: &Home) -> MonitorState {
    fs::read(state_file(home))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save(home: &Home, state: &MonitorState) {
    if let Ok(bytes) = serde_json::to_vec_pretty(state) {
        let _ = fs::write(state_file(home), bytes);
    }
}

/// One run of the monitor at `now`. Returns what it did, one line per action.
pub fn run_once(home: &Home, machine: &dyn Machine, now: Timestamp) -> Vec<String> {
    let store = home.store();
    let mut state = load(home);
    let running = machine.daemon_running();
    let current = store.current_commit();
    let crash_loop = current.as_ref().and_then(|commit| {
        detect_crash_loop(&start_log(home).read(), commit, store.last_good(), now)
    });
    let seen = Observation {
        running,
        answered: running && machine.probe(),
        crash_loop,
    };
    let actions = decide(&mut state, &seen, now);
    let mut done = Vec::new();
    for action in actions {
        match action {
            Action::RollBack(looped) => {
                match roll_back(home, &looped, now) {
                    Ok(()) => done.push(format!(
                        "{} crash-looped; went back to {}",
                        looped.bad, looped.to
                    )),
                    Err(error) => done.push(format!("cannot go back to {}: {error}", looped.to)),
                }
                done.push(kickstart(machine));
            }
            Action::Kickstart => {
                if seen.running {
                    // A daemon that runs and does not answer is hung: its run counts towards a
                    // crash loop even though the kickstart's `SIGTERM` asks it to stop.
                    let _ = start_log(home).mark_hung();
                }
                done.push(kickstart(machine));
            }
            Action::AskForRepair => {
                let evidence = evidence(&state, &seen, home, now);
                let reason = "the daemon does not answer and kickstarting it did not help";
                let record = serde_json::json!({"at": now, "reason": reason, "evidence": evidence});
                let written = serde_json::to_vec_pretty(&record)
                    .map_err(std::io::Error::other)
                    .and_then(|bytes| fs::write(home.root.join("justiciar-needed.json"), bytes));
                if let Err(error) = written {
                    done.push(format!("cannot write justiciar-needed.json: {error}"));
                }
                machine.notify("Ignite needs a repair", reason);
                done.push(format!("asked for a repair: {reason}"));
            }
        }
    }
    if seen.running && seen.answered {
        let _ = fs::remove_file(home.root.join("justiciar-needed.json"));
    }
    // The monitor runs the last-good build, and writes the service as that build would. While it
    // is older than the current build, rewriting would undo what the current build's install
    // wrote, so this waits until the current build is marked good.
    if seen.running
        && seen.answered
        && done.is_empty()
        && current.is_some()
        && current == store.last_good()
    {
        match machine.refresh_daemon() {
            Ok(true) => done.push(
                "an older build wrote the daemon's login service; rewrote and reloaded it"
                    .to_owned(),
            ),
            Ok(false) => {}
            Err(error) => done.push(format!(
                "cannot rewrite the daemon's login service: {error}"
            )),
        }
    }
    done.extend(justiciar::run(
        home,
        machine,
        seen.running && seen.answered,
        now,
    ));
    let hub_here = machine.hub_here();
    if decide_hub(&mut state, hub_here, hub_here && machine.probe_hub(), now) {
        done.push(match machine.kickstart_hub() {
            Ok(()) => "the hub did not answer; kickstarted it".to_owned(),
            Err(error) => format!("the hub did not answer; cannot kickstart it: {error}"),
        });
    }
    save(home, &state);
    done
}

fn kickstart(machine: &dyn Machine) -> String {
    match machine.kickstart() {
        Ok(()) => "kickstarted the daemon".to_owned(),
        Err(error) => format!("cannot kickstart the daemon: {error}"),
    }
}

/// The facts a person repairing the daemon starts from.
fn evidence(state: &MonitorState, seen: &Observation, home: &Home, now: Timestamp) -> String {
    let store = home.store();
    let show = |commit: Option<ignite_core::ids::Commit>| {
        commit.map_or_else(|| "none".to_owned(), |c| c.to_string())
    };
    let recent = start_log(home)
        .read()
        .iter()
        .filter(|start| seconds_since(start.at, now) < crate::update::CRASH_WINDOW_S)
        .count();
    format!(
        "daemon process running: {}; {} runs in a row without an answer since {}; last answer \
         {}; {} kickstarts; current build {}; last-good build {}; {recent} daemon starts in \
         the last 5 minutes",
        seen.running,
        state.consecutive_failures,
        state
            .failing_since
            .map_or_else(|| "unknown".to_owned(), |t| t.to_string()),
        state
            .last_ok
            .map_or_else(|| "never".to_owned(), |t| t.to_string()),
        state.kickstarts,
        show(store.current_commit()),
        show(store.last_good()),
    )
}

/// The real machine: the login service for the process and the kickstart, the daemon's socket
/// for the probe and the desktop for notifications.
pub struct NativeMachine {
    pub socket: PathBuf,
    /// The Ignite home, which records the main checkout.
    pub home: PathBuf,
}

impl Machine for NativeMachine {
    fn daemon_running(&self) -> bool {
        // When the login service cannot say, the probe decides.
        Native.daemon_running().unwrap_or(true)
    }

    fn probe(&self) -> bool {
        let socket = self.socket.to_string_lossy().into_owned();
        let request = Request {
            verb: "status".to_owned(),
            args: Vec::new(),
            cwd: self.socket.parent().map(PathBuf::from).unwrap_or_default(),
            session: None,
            wait_s: PROBE_TIMEOUT.as_secs() as u32,
            pid: Some(std::process::id()),
        };
        let (sender, receiver) = mpsc::channel();
        // A daemon that accepts the connection and never answers leaves this thread behind,
        // which ends with the process.
        std::thread::spawn(move || {
            let _ = sender.send(crate::server::send(&socket, &request).is_ok());
        });
        receiver.recv_timeout(PROBE_TIMEOUT).unwrap_or(false)
    }

    fn kickstart(&self) -> std::io::Result<()> {
        Native.kickstart_daemon()
    }

    fn notify(&self, title: &str, text: &str) {
        ignite_os::notify::post(title, text);
    }

    fn hub_here(&self) -> bool {
        Native.hub_installed().unwrap_or(false)
    }

    fn probe_hub(&self) -> bool {
        // The address the hub's service listens on; a service that names none is not probed.
        let Ok(Some(listen)) = Native.recorded_variable(ignite_os::service::HUB_LISTEN_VARIABLE)
        else {
            return true;
        };
        hub_answers(&listen)
    }

    fn kickstart_hub(&self) -> std::io::Result<()> {
        Native.kickstart_hub()
    }

    fn refresh_daemon(&self) -> std::io::Result<bool> {
        Native.refresh_daemon()
    }

    fn justiciar_app(&self) -> Option<justiciar::JusticiarApp> {
        let home = Home {
            root: self.home.clone(),
        };
        let checkout = PathBuf::from(fs::read_to_string(home.checkout_file()).ok()?.trim());
        let config = crate::main::read_config(&checkout).ok()?;
        justiciar::first_cli_app(&config, &checkout)
    }

    fn locked(&self, lock: &Path) -> bool {
        justiciar::lock_held(lock)
    }

    fn start_justiciar(&self, turn: &justiciar::JusticiarTurn) -> std::io::Result<()> {
        justiciar::spawn_detached(turn)
    }

    fn daemon_log_tail(&self, lines: usize) -> String {
        // The log file the daemon's login service writes on macOS (`ignite_os::service`).
        let Some(user_home) = ignite_core::config::home_dir(|name| std::env::var_os(name)) else {
            return String::new();
        };
        let log = user_home
            .join("Library")
            .join("Logs")
            .join("ignite.daemon.log");
        fs::read_to_string(log).map_or_else(
            |_| String::new(),
            |text| {
                let all: Vec<&str> = text.lines().collect();
                all[all.len().saturating_sub(lines)..].join("\n")
            },
        )
    }
}

/// Whether the hub listening at `listen` answers an HTTP request within [`PROBE_TIMEOUT`]. Any
/// answer counts, whatever its status: it shows the hub is serving.
pub fn hub_answers(listen: &str) -> bool {
    // An address that listens everywhere is reached through the loopback address.
    let address = listen
        .replace("0.0.0.0:", "127.0.0.1:")
        .replace("[::]:", "[::1]:");
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(PROBE_TIMEOUT))
        .proxy(None)
        .build()
        .into();
    agent.get(&format!("http://{address}/")).call().is_ok()
}

/// `ignite monitor`: one run against this machine. Never fails: a problem it handled is
/// reported in the text, and so is a panic.
pub fn run() -> Reply {
    let outcome = std::panic::catch_unwind(|| {
        let home = Home::from_env();
        let machine = NativeMachine {
            socket: crate::server::default_socket(),
            home: home.root.clone(),
        };
        run_once(&home, &machine, SystemClock.now())
    });
    let text = match outcome {
        Ok(done) if done.is_empty() => "monitor: nothing to do".to_owned(),
        Ok(done) => done.join("\n"),
        Err(_) => "monitor: stopped by an internal error; will run again".to_owned(),
    };
    Reply::text(Exit::Success, text)
}

#[cfg(test)]
#[path = "monitor_tests.rs"]
mod tests;
