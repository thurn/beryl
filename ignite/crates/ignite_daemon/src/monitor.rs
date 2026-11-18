//! `ignite monitor`: a one-shot health check that the login service runs every two minutes, in
//! a program of its own, so it works when the daemon is down or crash-looping.
//!
//! Each run asks the daemon whether it is alive (a `status` request with a short timeout, the
//! only thing it uses the daemon's socket for), remembers the answer in `monitor.json` in the
//! home folder and, using only files and the login service:
//!
//! - kickstarts the daemon when its process is gone or three runs in a row got no answer;
//! - goes back to the last-good build when `starts.jsonl` shows the current build crash-looping
//!   (the same decision a starting daemon makes, in [`crate::update::detect_crash_loop`]);
//! - after 15 minutes of failing in spite of kickstarts, writes `justiciar-needed.json` and
//!   posts a notification, because a person or a repair session has to look.
//!
//! It holds no agent logic and calls no hub. The decision ([`decide`]) is a function of what
//! was observed; the observations and actions are the [`Machine`] trait, so the tests run the
//! decision against a fake. A run reports a problem it handled and exits successfully.

use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::time::{SystemClock, Timestamp};
use ignite_core::traits::Clock;
use ignite_os::Native;
use ignite_os::service::Service;
use serde::{Deserialize, Serialize};

use crate::update::{CrashLoop, Home, detect_crash_loop, roll_back, start_log};

/// This many runs in a row without an answer from the daemon get it kickstarted.
pub const FAILURES_BEFORE_KICKSTART: u32 = 3;
/// The daemon is kickstarted at most this often, so that one still starting up is not killed.
pub const KICKSTART_GAP_S: f64 = 300.0;
/// Failing this long, kickstarts included, asks for a repair.
pub const ESCALATE_AFTER_S: f64 = 900.0;
/// A repair request that has gone unanswered is posted again after this long.
pub const RENOTIFY_AFTER_S: f64 = 3600.0;
/// How long the daemon has to answer the probe.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

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
            Action::Kickstart => done.push(kickstart(machine)),
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
}

/// `ignite monitor`: one run against this machine. Never fails: a problem it handled is
/// reported in the text, and so is a panic.
pub fn run() -> Reply {
    let outcome = std::panic::catch_unwind(|| {
        let home = Home::from_env();
        let machine = NativeMachine {
            socket: crate::server::default_socket(),
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
