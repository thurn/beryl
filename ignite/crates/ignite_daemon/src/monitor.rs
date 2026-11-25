//! `ignite monitor`: a one-shot health check that the login service runs every two minutes, in
//! a program of its own, so it works when the daemon is down or crash-looping.
//!
//! Each run asks the daemon whether it is alive (an `alive` request with a short timeout, the
//! only thing it uses the daemon's socket for; see [`probe_daemon`]), remembers the answer in
//! `monitor.json` in the home folder and, using only files and the login service:
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
//!   ([`justiciar`]);
//! - probes each trees root the daemon uses, and while one does not answer, stops kickstarting,
//!   asks for a repair that names the drive, tells the hub and, where the machine's
//!   `ignite.toml` entry allows it, restarts the machine ([`storage`]).
//!
//! A run gives up after [`RUN_DEADLINE`] ([`with_deadline`]). The login service starts no new run
//! while one is still running, so a run blocked on a helper process would otherwise stop every
//! later one: on 2026-11-25 a Windows run blocked from 06:13Z, and the machine got no restart and
//! no repair session for nearly eight hours.
//!
//! The justiciar turn is the monitor's only agent logic. The monitor's only hub call is the
//! report of a stuck trees drive. Each decision ([`decide`], [`decide_hub`], [`justiciar::decide`]) is a function
//! of what was observed; the observations and actions are the [`Machine`] trait, so the tests
//! run the decisions against a fake. A run reports a problem it handled and exits successfully.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use ignite_core::protocol::client::{Exit, Reply};
use ignite_core::time::{SystemClock, Timestamp};
use ignite_core::traits::Clock;
use serde::{Deserialize, Serialize};

use crate::update::{CrashLoop, Home, StartRecord, detect_crash_loop, roll_back, start_log};

#[path = "monitor_justiciar.rs"]
pub mod justiciar;
#[path = "monitor_native.rs"]
mod native;
#[path = "monitor_storage.rs"]
pub mod storage;

pub use native::{NativeMachine, hub_answers, probe_daemon};

use storage::{RebootPolicy, StuckStorage};

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
/// A run that has not finished after this long gives up, so the next run can start. It is
/// shorter than the two minutes between runs.
pub const RUN_DEADLINE: Duration = Duration::from_secs(100);
/// This many runs in a row without an answer from the hub get it kickstarted.
pub const HUB_FAILURES_BEFORE_KICKSTART: u32 = 2;
/// The reason a repair request gives when the daemon is down and the trees drives answer.
pub const DAEMON_REASON: &str = "the daemon does not answer and kickstarting it did not help";

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
    /// Since when a trees root has not answered; `None` while every root answers.
    pub storage_stuck_since: Option<Timestamp>,
    /// When a repair for a stuck trees root was last asked for.
    pub storage_escalated_at: Option<Timestamp>,
    /// The `since` of the stuck root the hub has a report of.
    pub storage_reported: Option<Timestamp>,
    /// When the monitor last restarted the machine for a stuck trees root. Kept after the root
    /// answers again, so that restarts for this reason are at least [`storage::REBOOT_GAP_S`]
    /// apart.
    pub storage_reboot_at: Option<Timestamp>,
    /// Whether the force command has run since that restart.
    pub storage_forced: bool,
}

/// What one run saw.
#[derive(Clone, Debug)]
pub struct Observation {
    /// Whether the login service has a daemon process.
    pub running: bool,
    /// Whether the daemon answered the probe.
    pub answered: bool,
    pub crash_loop: Option<CrashLoop>,
    /// A trees root that did not answer its probe.
    pub storage: Option<StuckStorage>,
    /// What this machine allows the monitor to do about a stuck root.
    pub reboot: RebootPolicy,
    /// When the machine last started, when known.
    pub booted_at: Option<Timestamp>,
}

/// What one run does about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Go back to the last-good build, then kickstart the daemon into it.
    RollBack(CrashLoop),
    Kickstart,
    /// Write `justiciar-needed.json` and notify.
    AskForRepair,
    /// Write `justiciar-needed.json` naming the stuck trees root, and notify.
    AskForStorageRepair(StuckStorage),
    /// Tell the hub that the trees root is stuck.
    ReportStorage(StuckStorage),
    /// Restart the machine ([`Service::restart_machine`]).
    Restart,
    /// Run the operator's force command, program first.
    ForceRestart(Vec<String>),
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
    if let Some(stuck) = &seen.storage {
        // A new daemon, or a rollback's restart, would block on the drive like the old one.
        return storage::decide(state, stuck, &seen.reboot, seen.booted_at, now);
    }
    if state.storage_stuck_since.take().is_some() && !(seen.running && seen.answered) {
        // The daemon's own failures count from when the drive answered again: until then no
        // kickstart could have helped.
        state.failing_since = Some(now);
        state.consecutive_failures = 1;
        state.kickstarts = 0;
        state.escalated_at = None;
    }
    state.storage_escalated_at = None;
    state.storage_reported = None;
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
    /// Whether the daemon answers an `alive` request in time.
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
    /// The trees roots the daemon uses ([`storage::roots`]).
    fn trees_roots(&self) -> Vec<PathBuf>;
    /// Whether the trees root `root` answers a metadata read within [`storage::PROBE_DEADLINE`].
    fn probe_storage(&self, root: &Path) -> bool;
    /// What this machine's `ignite.toml` entry allows the monitor to do about a stuck root.
    fn reboot_policy(&self) -> RebootPolicy;
    /// When the machine last started, when it can say.
    fn booted_at(&self) -> Option<Timestamp>;
    /// Reports the stuck root to the hub (`POST /machines/storage-stuck`).
    fn report_storage(&self, stuck: &StuckStorage) -> Result<(), String>;
    /// Restarts the machine now ([`Service::restart_machine`]).
    fn restart(&self) -> std::io::Result<()>;
    /// Runs `command`, program first, and waits for it.
    fn run_command(&self, command: &[String]) -> std::io::Result<()>;
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
        ..observe_storage(machine, &state, now)
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
                ask_for_repair(
                    home,
                    machine,
                    "Ignite needs a repair",
                    DAEMON_REASON,
                    &evidence,
                    now,
                    &mut done,
                );
            }
            storage_action => {
                let evidence = evidence(&state, &seen, home, now);
                act_on_storage(
                    home,
                    machine,
                    &mut state,
                    storage_action,
                    &evidence,
                    now,
                    &mut done,
                );
            }
        }
    }
    if seen.running && seen.answered && seen.storage.is_none() {
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

/// The stuck trees root, if any, with the policy and boot time a stuck root needs, and the rest
/// of an [`Observation`] left for the caller to fill in.
fn observe_storage(machine: &dyn Machine, state: &MonitorState, now: Timestamp) -> Observation {
    let storage = machine
        .trees_roots()
        .into_iter()
        .find(|root| !machine.probe_storage(root))
        .map(|root| StuckStorage {
            root,
            since: state.storage_stuck_since.unwrap_or(now),
        });
    let stuck = storage.is_some();
    Observation {
        running: false,
        answered: false,
        crash_loop: None,
        storage,
        reboot: if stuck {
            machine.reboot_policy()
        } else {
            RebootPolicy::default()
        },
        booted_at: if stuck { machine.booted_at() } else { None },
    }
}

/// Carries out one of the actions for a stuck trees root.
fn act_on_storage(
    home: &Home,
    machine: &dyn Machine,
    state: &mut MonitorState,
    action: Action,
    evidence: &str,
    now: Timestamp,
    done: &mut Vec<String>,
) {
    match action {
        Action::AskForStorageRepair(stuck) => {
            let reason = stuck.reason();
            ask_for_repair(
                home,
                machine,
                "Ignite needs a restart",
                &reason,
                evidence,
                now,
                done,
            );
        }
        Action::ReportStorage(stuck) => match machine.report_storage(&stuck) {
            Ok(()) => {
                state.storage_reported = Some(stuck.since);
                done.push("told the hub that a trees drive is stuck".to_owned());
            }
            Err(error) => done.push(format!(
                "cannot tell the hub that a trees drive is stuck: {error}"
            )),
        },
        Action::Restart => done.push(match machine.restart() {
            Ok(()) => "restarting the machine: a trees drive is stuck".to_owned(),
            Err(error) => format!("cannot restart the machine: {error}"),
        }),
        Action::ForceRestart(command) => done.push(match machine.run_command(&command) {
            Ok(()) => format!("still up after the restart; ran {}", command.join(" ")),
            Err(error) => format!(
                "still up after the restart; cannot run {}: {error}",
                command.join(" ")
            ),
        }),
        Action::RollBack(_) | Action::Kickstart | Action::AskForRepair => {}
    }
}

/// Writes `justiciar-needed.json` with `reason` and `evidence`, and posts a notification.
fn ask_for_repair(
    home: &Home,
    machine: &dyn Machine,
    title: &str,
    reason: &str,
    evidence: &str,
    now: Timestamp,
    done: &mut Vec<String>,
) {
    let record = serde_json::json!({"at": now, "reason": reason, "evidence": evidence});
    let written = serde_json::to_vec_pretty(&record)
        .map_err(std::io::Error::other)
        .and_then(|bytes| fs::write(home.root.join("justiciar-needed.json"), bytes));
    if let Err(error) = written {
        done.push(format!("cannot write justiciar-needed.json: {error}"));
    }
    machine.notify(title, reason);
    done.push(format!("asked for a repair: {reason}"));
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

/// The processes a kickstart ends: every daemon in `starts` that did not stop on purpose, newest
/// first. Ending only the newest is not enough. When a kickstarted daemon hangs as well, the
/// daemon it replaced may still be alive and hold `daemon.lock`, and every later start is then
/// refused. On 2026-11-25 daemon 24900 hung and outlived three kickstarts, which each ended only
/// the daemon started just before. A process that has already exited, or whose ID now belongs
/// to a program outside Ignite's builds, is left alone by `terminate_under`.
pub fn daemons_to_end(starts: &[StartRecord]) -> Vec<u32> {
    let mut pids = Vec::new();
    for start in starts.iter().rev().filter(|start| start.stopped.is_none()) {
        if !pids.contains(&start.pid) {
            pids.push(start.pid);
        }
    }
    pids
}

/// Runs `work` on a thread of its own and returns its result, or `None` when it has not finished
/// within `deadline`. A thread that is still blocked is left behind and ends with the process.
pub fn with_deadline<T: Send + 'static>(
    deadline: Duration,
    work: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(work());
    });
    receiver.recv_timeout(deadline).ok()
}

/// `ignite monitor`: one run against this machine. Never fails: a problem it handled is
/// reported in the text, and so is a panic.
pub fn run() -> Reply {
    let outcome = with_deadline(RUN_DEADLINE, || {
        std::panic::catch_unwind(|| {
            let home = Home::from_env();
            let machine = NativeMachine {
                socket: crate::server::default_socket(),
                home: home.root.clone(),
            };
            run_once(&home, &machine, SystemClock.now())
        })
    });
    let text = match outcome {
        Some(Ok(done)) if done.is_empty() => "monitor: nothing to do".to_owned(),
        Some(Ok(done)) => done.join("\n"),
        Some(Err(_)) => "monitor: stopped by an internal error; will run again".to_owned(),
        None => format!(
            "monitor: a step did not finish within {} s; gave up so that the next run can start",
            RUN_DEADLINE.as_secs()
        ),
    };
    Reply::text(Exit::Success, text)
}

#[cfg(test)]
#[path = "monitor_tests.rs"]
mod tests;
