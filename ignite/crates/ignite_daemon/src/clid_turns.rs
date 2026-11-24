//! The real [`Turns`]: each CLI turn is a supervised command started through `start`'s
//! [`TurnLauncher`], found again among the runs in the Ignite home by the `IGNITE_CLI_TURN`
//! variable in its `spec.json`, and read from that run's `output.log`. Once a daemon finds the
//! run, it remembers its path so the loop can poll it without rereading the archive.
//!
//! A turn's supervisor is detached from the daemon, so a daemon restart leaves it running and
//! the next daemon follows it. A thread of the daemon that started it waits for the supervisor,
//! so it leaves no zombie process behind.
//!
//! Two parts of one daemon can try to start the same turn at once: the ticket hand-off
//! records a turn and starts it, while the `ignite-cli` loop finds the same turn recorded as
//! starting and starts it too. [`SupervisedTurns::start`] looks for the turn's run and starts it
//! under one lock, so the second caller finds the run the first one started. A supervisor
//! started anyway, as by two daemons, finds in the session's folder that the turn has run and
//! exits without running the program (see `start::cli_turn`).
//!
//! This module also holds [`CliDaemon::recover`], which the daemon calls once at start, before
//! its socket answers, for every slot whose turn was recorded as starting.

use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
#[cfg(all(test, unix))]
use std::sync::Arc;
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::UNIX_EPOCH;

use ignite_core::cli::SlotState;
use ignite_core::ids::SessionId;
use ignite_core::time::Timestamp;
use ignite_core::traits::{CommandSpec, Error, Result};

#[cfg(all(test, unix))]
use crate::supervise::Started;
use crate::supervise::{ResultFile, Run, peaks::WorkerPeaks};

use super::start::{Recovery, TurnLauncher, find_run, has_run_lock, recover_run};
use super::{CliDaemon, Turns, slot_of};

/// The name of a run's command output in its directory, as the supervisor writes it.
const OUTPUT: &str = "output.log";
/// The file in a run's directory that holds its command's process group ID, which is also the
/// process ID of the command: the supervisor makes the command the leader of a new group, and
/// `ignite internal cli-turn` replaces itself with the CLI program, keeping its process ID.
const GROUP: &str = "group";
/// The spec of a run, written before its supervisor starts.
const SPEC: &str = "spec.json";
/// Keep enough paths for active workers while bounding memory spent on historical lookups.
const RECENT_RUNS: usize = 128;

#[derive(Default)]
struct RunPaths {
    recent: Mutex<VecDeque<((SessionId, u32), Run)>>,
}

impl RunPaths {
    /// The lookup runs outside the cache lock so a slow search cannot block a cache hit.
    fn get(
        &self,
        session: &SessionId,
        turn: u32,
        locate: impl FnOnce() -> Option<Run>,
    ) -> Option<Run> {
        let key = (session.clone(), turn);
        let cached = {
            let mut recent = super::lock(&self.recent);
            let entry = recent
                .iter()
                .position(|(candidate, _)| *candidate == key)
                .and_then(|index| recent.remove(index));
            entry.map(|entry| {
                let run = entry.1.clone();
                recent.push_front(entry);
                run
            })
        };
        // Pruning can remove a collected run. Fall back to looking it up again in that case.
        if let Some(run) = cached.filter(|run| has_run_lock(run.dir())) {
            return Some(run);
        }
        let run = locate()?;
        self.remember(key, run.clone());
        Some(run)
    }

    fn remember(&self, key: (SessionId, u32), run: Run) {
        let mut recent = super::lock(&self.recent);
        recent.retain(|(candidate, _)| *candidate != key);
        recent.push_front((key, run));
        recent.truncate(RECENT_RUNS);
    }
}

/// CLI turns run as supervised commands under the Ignite home.
pub struct SupervisedTurns {
    launcher: TurnLauncher,
    /// The Ignite home, whose `runs` folder holds the turns' run directories.
    home: PathBuf,
    /// Held while a turn's run is looked for and started, so one daemon starts it once.
    starting: Mutex<()>,
    runs: RunPaths,
    worker_peaks: WorkerPeaks,
    #[cfg(all(test, unix))]
    test_supervisors: Option<Arc<Mutex<Vec<Started>>>>,
}

impl SupervisedTurns {
    pub fn new(launcher: TurnLauncher, home: PathBuf) -> SupervisedTurns {
        SupervisedTurns {
            launcher,
            worker_peaks: WorkerPeaks::load(home.join("runs/worker-peaks.json")),
            home,
            starting: Mutex::new(()),
            runs: RunPaths::default(),
            #[cfg(all(test, unix))]
            test_supervisors: None,
        }
    }

    /// Lets an end-to-end test reap its supervisors, including those started before a restart.
    #[cfg(all(test, unix))]
    pub(super) fn with_test_supervisors(mut self, supervisors: Arc<Mutex<Vec<Started>>>) -> Self {
        self.test_supervisors = Some(supervisors);
        self
    }

    fn run_file(&self, session: &SessionId, turn: u32, name: &str) -> Option<PathBuf> {
        self.run(session, turn).map(|run| run.dir().join(name))
    }

    fn run(&self, session: &SessionId, turn: u32) -> Option<Run> {
        self.runs
            .get(session, turn, || find_run(&self.home, session, turn))
    }
}

/// When the file at `path` was last changed, if it exists.
fn modified_at(path: &std::path::Path) -> Option<Timestamp> {
    let since_epoch = fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?;
    Some(Timestamp::from_millis(
        i64::try_from(since_epoch.as_millis()).ok()?,
    ))
}

impl Turns for SupervisedTurns {
    /// Starts the turn unless a run of it exists already. The command's working directory and
    /// variables come from the launcher, which sets the same ones [`CliDaemon`] asks for.
    fn start(&self, session: &SessionId, turn: u32, command: &CommandSpec) -> Result<()> {
        let _held = self.starting.lock().unwrap_or_else(PoisonError::into_inner);
        let (_, started) = self
            .launcher
            .ensure(session, turn, &command.args)
            .map_err(|e| Error::Io(format!("turn {turn} of {session} did not start: {e}")))?;
        if let Some(started) = started {
            if has_run_lock(started.run().dir()) {
                self.runs
                    .remember((session.clone(), turn), started.run().clone());
            }
            #[cfg(all(test, unix))]
            if let Some(supervisors) = &self.test_supervisors {
                super::lock(supervisors).push(started);
                return Ok(());
            }
            thread::spawn(move || started.wait());
        }
        Ok(())
    }

    fn recover(&self, session: &SessionId, turn: u32) -> Recovery {
        self.run(session, turn)
            .as_ref()
            .map_or(Recovery::NotStarted, recover_run)
    }

    fn output(&self, session: &SessionId, turn: u32) -> Option<String> {
        let path = self.run_file(session, turn, OUTPUT)?;
        Some(fs::read_to_string(path).unwrap_or_default())
    }

    fn grew_at(&self, session: &SessionId, turn: u32) -> Option<Timestamp> {
        let run = self.run(session, turn)?;
        modified_at(&run.dir().join(OUTPUT)).or_else(|| modified_at(&run.dir().join(SPEC)))
    }

    fn process(&self, session: &SessionId, turn: u32) -> Option<u32> {
        let path = self.run_file(session, turn, GROUP)?;
        fs::read_to_string(path).ok()?.trim().parse().ok()
    }

    fn record_worker_memory(&self, session: &SessionId, turn: u32) {
        if let Some(result) = self
            .run_file(session, turn, "result.json")
            .and_then(|path| fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<ResultFile>(&bytes).ok())
        {
            self.worker_peaks
                .record(format!("{session}:{turn}"), result.usage.peak_mb);
        }
    }

    fn worker_memory_mb(&self) -> u64 {
        self.worker_peaks.usual()
    }

    fn end(&self, session: &SessionId, turn: u32) -> Result<()> {
        match self.run(session, turn) {
            Some(run) => run.end().map_err(|e| Error::Io(e.to_string())),
            None => Ok(()),
        }
    }
}

impl CliDaemon {
    /// Makes sure every turn recorded as starting has been started once: a turn whose run
    /// exists is marked running, and one with no run, recorded by a daemon that died before
    /// starting it, is started now, unless its app is on a CLI hold. Turns that run or ended
    /// are left to [`CliDaemon::step`]. Returns how many turns with no run this call found and
    /// asked to start. A slot that fails is reported after every other slot was looked at.
    pub fn recover(&self) -> Result<u32> {
        self.database(|conn| ignite_store::local::cli_slots::clear_empty(conn))?;
        let mut started = 0;
        let mut first = None;
        for row in self.rows()? {
            let slot = slot_of(&row);
            let Some(session) = slot.session.clone() else {
                continue;
            };
            if slot.state != SlotState::Starting {
                continue;
            }
            let outcome = if self.turns.recover(&session, slot.turn) == Recovery::NotStarted {
                self.launch(&row).map(|()| started += 1)
            } else {
                self.mark_running(&row)
            };
            if let Err(error) = outcome {
                first.get_or_insert(error);
            }
        }
        first.map_or(Ok(started), Err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clid::start::TURN_VARIABLE;
    use crate::supervise::SupervisedLauncher;
    use std::path::Path;
    use std::sync::Arc;

    fn write_run(home: &Path, name: &str, session: &SessionId, output: &str) -> PathBuf {
        let dir = home.join("runs").join(name);
        fs::create_dir_all(&dir).unwrap();
        let spec = serde_json::json!({
            "args": ["fake-cli"],
            "cwd": home,
            "env": [[TURN_VARIABLE, format!("{session}:1")]],
            "queued_s": 0.0,
        });
        fs::write(dir.join(SPEC), spec.to_string()).unwrap();
        fs::write(dir.join("lock"), "").unwrap();
        fs::write(dir.join(OUTPUT), output).unwrap();
        dir
    }

    #[test]
    fn polling_remembers_the_run_found_after_a_restart_until_it_is_pruned() {
        let home = ignite_core::test_files::temp_dir().join("cli-turn-path");
        let launcher = Arc::new(SupervisedLauncher::with_command(
            Vec::new(),
            home.join("runs"),
        ));
        let turns = SupervisedTurns::new(
            TurnLauncher::with_command(launcher, Vec::new(), home.clone(), home.clone()),
            home.clone(),
        );
        let session = SessionId::new("cli-fake-0a1b2c3d").unwrap();
        assert_eq!(turns.output(&session, 1), None);

        let first = write_run(&home, "01", &session, "first output");
        write_run(&home, "02", &session, "duplicate output");
        assert_eq!(turns.output(&session, 1).as_deref(), Some("first output"));

        // Once the supervisor is identified, poll its files without reading its spec again.
        fs::remove_file(first.join(SPEC)).unwrap();
        assert_eq!(turns.output(&session, 1).as_deref(), Some("first output"));
        assert!(turns.grew_at(&session, 1).is_some());
        assert_eq!(turns.output(&session, 2), None);

        fs::remove_dir_all(first).unwrap();
        assert_eq!(
            turns.output(&session, 1).as_deref(),
            Some("duplicate output")
        );
    }
}
