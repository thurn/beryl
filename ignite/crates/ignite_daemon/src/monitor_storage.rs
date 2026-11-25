//! The monitor's trees-drive check: whether each trees root this machine uses still answers a
//! small metadata read, and what to do when one does not.
//!
//! On 2026-11-25 the drive that held a Windows machine's trees (a Dev Drive inside a virtual
//! disk file) stopped answering I/O after four `vhdmp` resets. Every process that touched it blocked in the
//! kernel and could not be killed. The monitor went on kickstarting the daemon every few
//! minutes, and each new daemon blocked on the drive too, so each kickstart only added a stuck
//! process. A graceful restart hung as well, and the machine had to be power-cycled.
//!
//! So each monitor run:
//!
//! - reads the trees roots the daemon recorded in `trees-roots.json` in the Ignite home (the
//!   root its fleet settings name, and the folder of every tree it still has), which the daemon
//!   rewrites after each heartbeat ([`record_roots`]);
//! - probes each one in a child process, `ignite internal probe-storage`, that reads the root's
//!   metadata and lists the root and its first entry ([`run_probe`]). A read that fails counts as
//!   an answer: only a read that has not returned after [`PROBE_DEADLINE`] counts as stuck. The
//!   probe runs in a child so that the monitor never touches the drive itself: a monitor blocked
//!   in the kernel could not exit, and the login service starts no new monitor while one runs.
//!   The child holds a lock file while it reads, and a run that finds the lock still held by an
//!   earlier child counts the root as stuck without starting another, so a stuck drive gets at
//!   most one stuck probe per root;
//! - while a root is stuck, does not kickstart the daemon or roll back its build, since a new
//!   daemon would block too. It asks for a repair at once, with a reason that names the root and
//!   says the machine needs a restart, and reports the root to the hub, which files the
//!   machine's ticket for a person at once ([`decide`]);
//! - restarts the machine once the root has been stuck for the machine's
//!   `stuck_storage_reboot_after_s` and no restart for this reason happened in the last
//!   [`REBOOT_GAP_S`] seconds, and, if the machine has not started again [`FORCE_AFTER_S`]
//!   seconds after that restart, runs its `stuck_storage_force_command`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ignite_core::config::Config;
use ignite_core::protocol::machine_state::StorageStuck;
use ignite_core::time::Timestamp;
use serde::{Deserialize, Serialize};

use super::{Action, MonitorState, RENOTIFY_AFTER_S, seconds_since};
use crate::update::Home;

/// How long a probe of one trees root may take before the root counts as stuck.
pub const PROBE_DEADLINE: Duration = Duration::from_secs(20);
/// After restarting the machine for a stuck root, the monitor does not do so again for this
/// long, so that a drive that is still stuck after the restart cannot restart it in a loop.
pub const REBOOT_GAP_S: f64 = 6.0 * 3600.0;
/// A machine still running this long after its ordinary restart gets the force command.
pub const FORCE_AFTER_S: f64 = 600.0;

/// A trees root that did not answer, and since when it has not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StuckStorage {
    pub root: PathBuf,
    pub since: Timestamp,
}

impl StuckStorage {
    /// The reason a repair request, a notification and the hub's ticket give.
    pub fn reason(&self) -> String {
        format!(
            "the trees drive at {} has not answered a metadata read since {}. Processes that \
             touch it block in the kernel, so kickstarting the daemon does not help: the \
             machine needs a restart",
            self.root.display(),
            self.since,
        )
    }
}

/// What this machine's `[machine.<name>]` in `ignite.toml` allows the monitor to do about a
/// stuck root. The default never restarts the machine.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RebootPolicy {
    /// `stuck_storage_reboot_after_s`.
    pub after_s: Option<u32>,
    /// `stuck_storage_force_command`.
    pub force: Option<Vec<String>>,
}

impl RebootPolicy {
    pub fn of(config: &Config, hostname: &str) -> RebootPolicy {
        config
            .machine(hostname)
            .map(|machine| RebootPolicy {
                after_s: machine.stuck_storage_reboot_after_s,
                force: machine.stuck_storage_force_command.clone(),
            })
            .unwrap_or_default()
    }
}

/// Updates `state` for a run at `now` that found `stuck` and says what to do. `booted_at` is
/// when the machine last started, when known. The state already counts the returned repair
/// request, restart and force command as done; [`Action::ReportStorage`] is counted by the
/// caller once the hub has the report.
pub fn decide(
    state: &mut MonitorState,
    stuck: &StuckStorage,
    policy: &RebootPolicy,
    booted_at: Option<Timestamp>,
    now: Timestamp,
) -> Vec<Action> {
    let mut actions = Vec::new();
    state.storage_stuck_since = Some(stuck.since);
    if state
        .storage_escalated_at
        .is_none_or(|at| seconds_since(at, now) >= RENOTIFY_AFTER_S)
    {
        state.storage_escalated_at = Some(now);
        actions.push(Action::AskForStorageRepair(stuck.clone()));
    }
    if state.storage_reported != Some(stuck.since) {
        actions.push(Action::ReportStorage(stuck.clone()));
    }
    let stuck_long = policy
        .after_s
        .is_some_and(|after| seconds_since(stuck.since, now) >= f64::from(after));
    let rested = state
        .storage_reboot_at
        .is_none_or(|at| seconds_since(at, now) >= REBOOT_GAP_S);
    if stuck_long && rested {
        state.storage_reboot_at = Some(now);
        state.storage_forced = false;
        actions.push(Action::Restart);
        return actions;
    }
    if let (Some(at), Some(command)) = (state.storage_reboot_at, &policy.force) {
        // Only the restart this incident asked for, and only while the machine has not
        // started since: a machine that restarted and found the drive stuck again waits out
        // `REBOOT_GAP_S` like any other.
        let this_incident = at >= stuck.since;
        let still_up = booted_at.is_none_or(|booted| booted < at);
        if this_incident
            && still_up
            && !state.storage_forced
            && seconds_since(at, now) >= FORCE_AFTER_S
        {
            state.storage_forced = true;
            actions.push(Action::ForceRestart(command.clone()));
        }
    }
    actions
}

/// The report that tells the hub `stuck` is stuck on the machine whose hostname is `host`. The
/// hub files that machine's "stopped reporting" ticket with it, naming the drive. A second report
/// of the same incident adds nothing.
pub fn hub_report(stuck: &StuckStorage, host: &str) -> StorageStuck {
    StorageStuck {
        host: host.to_owned(),
        root: stuck.root.display().to_string(),
        since: stuck.since,
    }
}

/// What the daemon records for the monitor: `<home>/trees-roots.json`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Roots {
    pub roots: Vec<PathBuf>,
}

fn roots_file(home: &Home) -> PathBuf {
    home.root.join("trees-roots.json")
}

/// The trees roots the daemon last recorded, or none before it has recorded any.
pub fn roots(home: &Home) -> Vec<PathBuf> {
    fs::read(roots_file(home))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Roots>(&bytes).ok())
        .map(|recorded| recorded.roots)
        .unwrap_or_default()
}

/// Records `roots` for the monitor, writing only when they changed.
pub fn record_roots(home: &Home, mut roots: Vec<PathBuf>) -> io::Result<()> {
    roots.sort();
    roots.dedup();
    let recorded = Roots { roots };
    if fs::read(roots_file(home))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Roots>(&bytes).ok())
        .as_ref()
        == Some(&recorded)
    {
        return Ok(());
    }
    let bytes = serde_json::to_vec_pretty(&recorded).map_err(io::Error::other)?;
    fs::write(roots_file(home), bytes)
}

/// Records the roots `trees` uses for the monitor, which probes them without the daemon, since
/// the daemon may be what is stuck. Called after each heartbeat.
pub fn record_roots_of(home: &Home, trees: &crate::trees::Trees) {
    if let Err(error) = record_roots(home, trees.roots_in_use()) {
        eprintln!("ignite: trees roots not recorded for the monitor: {error}");
    }
}

/// `monitor/storage-<root>.lock`: held by the probe of `root` while it reads.
pub fn lock_path(home: &Home, root: &Path) -> PathBuf {
    let name: String = root
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    home.root
        .join("monitor")
        .join(format!("storage-{name}.lock"))
}

/// `ignite internal probe-storage <lock> <root>`: takes `lock`, then reads `root`'s metadata and
/// lists `root` and its first entry. Whether the reads succeed does not matter, only that they
/// return. Returns the exit code: 0 once the reads returned, 3 when another probe holds `lock`.
pub fn run_probe(lock: &Path, root: &Path) -> i32 {
    let file = match fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock)
    {
        Ok(file) => file,
        Err(error) => {
            eprintln!("ignite: cannot open {}: {error}", lock.display());
            return 1;
        }
    };
    if file.try_lock().is_err() {
        eprintln!("ignite: another probe holds {}", lock.display());
        return 3;
    }
    let _ = fs::metadata(root);
    let first = fs::read_dir(root)
        .ok()
        .and_then(|mut entries| entries.next())
        .and_then(Result::ok);
    if let Some(entry) = first {
        let _ = fs::read_dir(entry.path()).map(|mut entries| entries.next());
    }
    0
}

/// Whether `root` answers: starts `ignite internal probe-storage` for it, detached, and waits
/// up to [`PROBE_DEADLINE`] for it to exit. A probe that has not exited by then is left
/// running, since a process blocked on the drive cannot be ended, and while it holds its lock
/// later runs count the root as stuck without starting another.
pub fn probe_detached(home: &Home, root: &Path) -> bool {
    let lock = lock_path(home, root);
    if let Some(folder) = lock.parent() {
        let _ = fs::create_dir_all(folder);
    }
    if super::justiciar::lock_held(&lock) {
        return false;
    }
    let Ok(executable) = std::env::current_exe() else {
        return true;
    };
    let mut command = std::process::Command::new(executable);
    command
        .args(["internal", "probe-storage"])
        .arg(&lock)
        .arg(root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        /// `CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS`.
        const DETACHED: u32 = 0x0000_0200 | 0x0000_0008;
        command.creation_flags(DETACHED);
    }
    // A probe that cannot start says nothing about the drive.
    let Ok(mut child) = command.spawn() else {
        return true;
    };
    let started = std::time::Instant::now();
    while started.elapsed() < PROBE_DEADLINE {
        match child.try_wait() {
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Ok(Some(_)) | Err(_) => return true,
        }
    }
    false
}

#[cfg(test)]
#[path = "monitor_storage_tests.rs"]
mod tests;
