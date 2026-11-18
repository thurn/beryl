//! Taking over, after a restart, the commands of a verb that the previous daemon was running.
//!
//! When the daemon restarts, every command keeps running under its supervisor, and the verb that
//! started it is run again under the same handle (see `server/resume.rs`). That second run asks
//! for the same commands. [`AdoptingLauncher`] answers each one that the first run had started
//! and that nobody has collected with that run's result, waiting for its supervisor if it still
//! runs, instead of starting the command again: the work is done once and its result is stored
//! once.
//!
//! A command is matched by the handle of the verb that started it, its arguments and its working
//! directory. The launcher adds the handle to the environment of every command a handled verb
//! starts, as `IGNITE_HANDLE`, which is how a restarted daemon finds it in the run's
//! `spec.json`. Commands of background loops have no handle and are never adopted: a loop starts
//! its round over.
//!
//! A run whose tree a restarted daemon resets (a landing tree, which only the daemon that leased
//! it can free) has had its files changed under it, so its result says nothing about the commit
//! it was started for. Such a run is ended at start ([`end_runs_in`]) and never adopted.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use ignite_core::ids::Handle;
use ignite_core::traits::{CommandSpec, Launcher, RunResult};

use crate::supervise::{Run, SpecFile, SupervisedLauncher};

/// The environment variable that names the handle of the verb a command was started for.
pub const HANDLE_VARIABLE: &str = "IGNITE_HANDLE";

/// How often an adopted run's wait looks whether its caller wants it ended.
const CANCEL_POLL: Duration = Duration::from_millis(250);

/// A command a previous daemon started for a handle, which a run of the same verb can take over.
pub struct Previous {
    pub handle: Handle,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// Blocks until the command has finished and returns its result.
    pub wait: Box<dyn FnOnce() -> RunResult + Send>,
    /// Ends the command and everything it started.
    pub end: Box<dyn Fn() + Send + Sync>,
}

impl Previous {
    /// The command `run` holds, if a handled verb started it.
    pub fn of_run(run: &Run) -> Option<Previous> {
        let spec = spec_of(run)?;
        let handle = spec
            .env
            .iter()
            .find(|(name, _)| name == HANDLE_VARIABLE)
            .and_then(|(_, value)| Handle::new(value.as_str()).ok())?;
        let (waited, ended) = (run.clone(), run.clone());
        Some(Previous {
            handle,
            args: spec.args,
            cwd: spec.cwd,
            wait: Box::new(move || waited.wait()),
            end: Box::new(move || {
                let _ = ended.end();
            }),
        })
    }
}

fn spec_of(run: &Run) -> Option<SpecFile> {
    serde_json::from_slice(&std::fs::read(run.dir().join("spec.json")).ok()?).ok()
}

/// Ends every run in `runs` whose working directory is inside one of `trees`, and returns the
/// others. The restarted daemon resets those trees, so what the runs would report is about files
/// that are no longer there.
pub fn end_runs_in(runs: Vec<Run>, trees: &[PathBuf]) -> Vec<Run> {
    runs.into_iter()
        .filter(|run| {
            let inside = spec_of(run)
                .is_some_and(|spec| trees.iter().any(|tree| spec.cwd.starts_with(tree)));
            if inside {
                eprintln!(
                    "ignite: ending {}: its tree is reset after the restart",
                    run.dir().display()
                );
                let _ = run.end();
            }
            !inside
        })
        .collect()
}

/// A launcher that hands each command a previous daemon started for the same handle back to
/// the verb run again under that handle, and starts every other command with `inner`.
pub struct AdoptingLauncher {
    inner: Arc<dyn Launcher>,
    previous: Mutex<HashMap<Handle, Vec<Previous>>>,
}

impl AdoptingLauncher {
    pub fn new(inner: Arc<dyn Launcher>, previous: Vec<Previous>) -> AdoptingLauncher {
        let mut by_handle: HashMap<Handle, Vec<Previous>> = HashMap::new();
        for run in previous {
            by_handle.entry(run.handle.clone()).or_default().push(run);
        }
        AdoptingLauncher {
            inner,
            previous: Mutex::new(by_handle),
        }
    }

    /// The supervised launcher, with the runs it finds that a previous daemon left for handles,
    /// apart from those in `reset_trees`, which are ended.
    pub fn over_supervised(
        launcher: Arc<SupervisedLauncher>,
        reset_trees: &[PathBuf],
    ) -> AdoptingLauncher {
        let runs = end_runs_in(launcher.unfinished(), reset_trees);
        let previous = runs.iter().filter_map(Previous::of_run).collect();
        AdoptingLauncher::new(launcher, previous)
    }

    /// How many previous commands no run has taken over yet.
    pub fn waiting(&self) -> usize {
        self.lock().values().map(Vec::len).sum()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<Handle, Vec<Previous>>> {
        self.previous.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The previous command of `handle` with `spec`'s arguments and directory, taken out so no
    /// other run takes it as well.
    fn take(&self, handle: &Handle, spec: &CommandSpec) -> Option<Previous> {
        let mut previous = self.lock();
        let runs = previous.get_mut(handle)?;
        let at = runs
            .iter()
            .position(|run| run.args == spec.args && same_dir(&run.cwd, &spec.cwd))?;
        let run = runs.remove(at);
        if runs.is_empty() {
            previous.remove(handle);
        }
        Some(run)
    }

    fn run_with(
        &self,
        spec: &CommandSpec,
        cancelled: Option<&(dyn Fn() -> bool + Sync)>,
    ) -> RunResult {
        let activity = crate::steps::current();
        let Some(handle) = activity.as_ref().and_then(|a| a.handle()) else {
            return match cancelled {
                Some(cancelled) => self.inner.run_until(spec, cancelled),
                None => self.inner.run(spec),
            };
        };
        if let Some(previous) = self.take(&handle, spec) {
            let stop = || {
                cancelled.is_some_and(|c| c())
                    || activity.as_ref().is_some_and(|a| a.stop_requested())
            };
            return wait_adopted(previous, &stop);
        }
        let mut tagged = spec.clone();
        tagged
            .env
            .push((HANDLE_VARIABLE.to_owned(), handle.to_string()));
        match cancelled {
            Some(cancelled) => self.inner.run_until(&tagged, cancelled),
            None => self.inner.run(&tagged),
        }
    }
}

fn same_dir(a: &Path, b: &Path) -> bool {
    a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

/// Waits for an adopted command, ending it once `stop` turns true, as the supervised launcher
/// does for a command it started.
fn wait_adopted(previous: Previous, stop: &dyn Fn() -> bool) -> RunResult {
    let Previous { wait, end, .. } = previous;
    let (done, result) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = done.send(wait());
    });
    let mut ended = false;
    loop {
        match result.recv_timeout(CANCEL_POLL) {
            Ok(result) => return result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if !ended && stop() {
                    end();
                    ended = true;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return RunResult {
                    exit: None,
                    output: PathBuf::new(),
                    usage: ignite_core::protocol::Usage::default(),
                    hung: false,
                };
            }
        }
    }
}

impl Launcher for AdoptingLauncher {
    fn run(&self, spec: &CommandSpec) -> RunResult {
        self.run_with(spec, None)
    }

    fn run_until(&self, spec: &CommandSpec, cancelled: &(dyn Fn() -> bool + Sync)) -> RunResult {
        self.run_with(spec, Some(cancelled))
    }
}

#[cfg(test)]
#[path = "adopt_tests.rs"]
mod tests;
