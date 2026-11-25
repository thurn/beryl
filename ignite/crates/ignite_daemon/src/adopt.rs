//! Taking over, after a restart, the commands of a verb that the previous daemon was running.
//!
//! When the daemon restarts, every command keeps running under its supervisor, and the verb that
//! started it is run again under the same handle (see `server/resume.rs`). That second run asks
//! for the same commands. [`AdoptingLauncher`] answers each one that the first run had started
//! and that nobody has collected with that run's result, waiting for its supervisor if it still
//! runs, instead of starting the command again: the work is done once and its result is stored
//! once.
//!
//! A command is matched by the handle of the verb that started it, its arguments, its working
//! directory and the commit its checkout had at `HEAD` when it started. The launcher records that
//! commit as `IGNITE_TREE_COMMIT` beside the handle, and a previous command whose commit differs
//! from the checkout's `HEAD` now is ended and started again, never taken over: a landing run
//! again that rebased onto a newer tip in the same tree must not report what an older commit's
//! command found. Its environment may otherwise differ: a check's runner names a new results file for each
//! run, so the files the adopted command wrote to paths its environment named are copied to
//! the paths the second run named ([`carry_outputs`]). The launcher adds the handle to the environment of every command a handled verb
//! starts, as `IGNITE_HANDLE`, which is how a restarted daemon finds it in the run's
//! `spec.json`. Commands of background loops have no handle and are never adopted: a loop starts
//! its round over.
//!
//! A run whose tree a restarted daemon resets (see `crate::trees::reset_at_start`: a tree being
//! prebuilt or reset, or a landing tree whose owner will not run again) has had its files changed
//! under it, so its result says nothing about the commit it was started for. Such a run is ended
//! at start ([`end_runs_in`]) and never adopted.
//!
//! A previous command whose handle no longer runs has nobody left to collect its result: the
//! verb's process died and the verb was not run again, or the run again finished without asking
//! for that command. Such a command is ended ([`AdoptingLauncher::end_orphans`], run at start
//! and then every [`ORPHAN_SWEEP`] by [`sweep_orphans`]), which also releases the cores its
//! adopted grant holds once its supervisor exits (see `supervise/grants.rs`). On cygnus on
//! 2026-11-25 a `cargo nextest run --no-run` left by a landing whose verb process died with a
//! hung daemon kept compiling, and kept its 10-core `land` grant, through two restarts:
//! `ignite status` showed every core granted at 19% CPU, and the machine took no tickets until an
//! incident commander ended the process.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use ignite_core::ids::{Commit, Handle};
use ignite_core::traits::{CommandSpec, Launcher, RunResult};

use crate::supervise::{Run, SpecFile, SupervisedLauncher};
use crate::trees::head;

/// The environment variable that names the handle of the verb a command was started for.
pub const HANDLE_VARIABLE: &str = "IGNITE_HANDLE";

/// The environment variable that names the commit a handled command's checkout had at `HEAD`
/// when the command started. Absent outside a checkout.
pub const TREE_COMMIT_VARIABLE: &str = "IGNITE_TREE_COMMIT";

/// How often an adopted run's wait looks whether its caller wants it ended.
const CANCEL_POLL: Duration = Duration::from_millis(250);

/// A command a previous daemon started for a handle, which a run of the same verb can take over.
pub struct Previous {
    pub handle: Handle,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// The variables the command was started with.
    pub env: Vec<(String, String)>,
    /// Blocks until the command has finished and returns its result.
    pub wait: Box<dyn FnOnce() -> RunResult + Send>,
    /// Ends the command and everything it started.
    pub end: Box<dyn Fn() + Send + Sync>,
}

impl Previous {
    /// The commit the command's checkout had at `HEAD` when it started, as recorded in its
    /// environment.
    fn tree_commit(&self) -> Option<&str> {
        self.env
            .iter()
            .find(|(name, _)| name == TREE_COMMIT_VARIABLE)
            .map(|(_, value)| value.as_str())
    }

    /// The command `run` holds, if a handled verb started it.
    pub fn of_run(run: &Run) -> Option<Previous> {
        let spec = spec_of(run)?;
        let handle = spec
            .env
            .iter()
            .find(|(name, _)| name == HANDLE_VARIABLE)
            .and_then(|(_, value)| Handle::new(value.as_str()).ok())?;
        let (waited, ended) = (run.clone(), run.clone());
        let marked = spec.clone();
        Some(Previous {
            handle,
            args: spec.args,
            cwd: spec.cwd,
            env: spec.env,
            wait: Box::new(move || waited.wait()),
            end: Box::new(move || {
                // A supervisor that has exited holds no process group any more, and its ID may
                // belong to another process by now.
                if !ended.is_running() {
                    return;
                }
                let _ = ended.end();
                // An ended compile leaves its target directory half written (see `end_runs_in`).
                let tree = marked.tree_path.as_deref().unwrap_or(&marked.cwd);
                crate::build::repair::mark_ended(&marked, tree);
            }),
        })
    }
}

fn spec_of(run: &Run) -> Option<SpecFile> {
    serde_json::from_slice(&std::fs::read(run.dir().join("spec.json")).ok()?).ok()
}

/// Ends every run in `runs` whose working directory is inside one of `trees`, and returns the
/// others. The restarted daemon resets those trees, so what the runs would report is about files
/// that are no longer there. A run that may have been compiling marks its target directory, so
/// the next build there first clears what the ended compile left unfinished (see
/// `crate::build::repair`).
pub fn end_runs_in(runs: Vec<Run>, trees: &[PathBuf]) -> Vec<Run> {
    runs.into_iter()
        .filter(|run| {
            let spec = spec_of(run);
            let Some((spec, tree)) = spec.as_ref().and_then(|spec| tree_of(spec, trees)) else {
                return true;
            };
            eprintln!(
                "ignite: ending {}: its tree is reset after the restart",
                run.dir().display()
            );
            let _ = run.end();
            crate::build::repair::mark_ended(spec, tree);
            false
        })
        .collect()
}

/// The run's spec with the tree of `trees` its working directory is inside, if any.
fn tree_of<'a>(spec: &'a SpecFile, trees: &'a [PathBuf]) -> Option<(&'a SpecFile, &'a Path)> {
    trees
        .iter()
        .find(|tree| spec.cwd.starts_with(tree))
        .map(|tree| (spec, tree.as_path()))
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

    /// Ends every previous command whose handle `running` says no longer runs, and returns how
    /// many it ended. Nobody can collect such a command's result: its verb's process died and
    /// the verb was not run again (its handle ended `incomplete`), or the verb's run again
    /// finished without asking for it. A command whose handle runs is kept, whether a live verb
    /// process waits for it or a verb run again under the same handle may still ask for it.
    pub fn end_orphans(&self, running: &dyn Fn(&Handle) -> bool) -> usize {
        let orphans: Vec<Previous> = {
            let mut previous = self.lock();
            let gone: Vec<Handle> = previous
                .keys()
                .filter(|handle| !running(handle))
                .cloned()
                .collect();
            gone.iter()
                .filter_map(|handle| previous.remove(handle))
                .flatten()
                .collect()
        };
        for orphan in &orphans {
            eprintln!(
                "ignite: ending `{}` in {}: {} no longer runs, so nobody collects its result",
                orphan.args.join(" "),
                orphan.cwd.display(),
                orphan.handle,
            );
            (orphan.end)();
        }
        orphans.len()
    }

    /// The previous command of `handle` with `spec`'s arguments and directory, taken out so no
    /// other run takes it as well. One that started at another commit than `at`, the checkout's
    /// `HEAD` now, is taken out too but ended, and `None` is returned so the command starts again.
    fn take(&self, handle: &Handle, spec: &CommandSpec, at: Option<&Commit>) -> Option<Previous> {
        let run = {
            let mut previous = self.lock();
            let runs = previous.get_mut(handle)?;
            let found = runs
                .iter()
                .position(|run| run.args == spec.args && same_dir(&run.cwd, &spec.cwd))?;
            let run = runs.remove(found);
            if runs.is_empty() {
                previous.remove(handle);
            }
            run
        };
        let now = at.map(Commit::as_str);
        if run.tree_commit() == now {
            return Some(run);
        }
        eprintln!(
            "ignite: starting `{}` for {handle} again: it ran at commit {}, and {} is now at {}",
            spec.args.join(" "),
            run.tree_commit().unwrap_or("(none recorded)"),
            spec.cwd.display(),
            now.unwrap_or("(no commit)"),
        );
        (run.end)();
        None
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
        let at = head::commit_at(&spec.cwd);
        if let Some(previous) = self.take(&handle, spec, at.as_ref()) {
            let stop = || {
                cancelled.is_some_and(|c| c())
                    || activity.as_ref().is_some_and(|a| a.stop_requested())
            };
            let env = previous.env.clone();
            let result = wait_adopted(previous, &stop);
            carry_outputs(&env, &spec.env);
            return result;
        }
        let mut tagged = spec.clone();
        tagged
            .env
            .push((HANDLE_VARIABLE.to_owned(), handle.to_string()));
        if let Some(at) = at {
            tagged
                .env
                .push((TREE_COMMIT_VARIABLE.to_owned(), at.to_string()));
        }
        match cancelled {
            Some(cancelled) => self.inner.run_until(&tagged, cancelled),
            None => self.inner.run(&tagged),
        }
    }
}

/// How often [`sweep_orphans`] looks again for previous commands whose handle has ended.
pub const ORPHAN_SWEEP: Duration = Duration::from_secs(30);

/// Ends the previous commands of handles that no longer run now, after the server has run again
/// or ended every handle the previous daemon left running, and then every [`ORPHAN_SWEEP`] on a
/// thread of its own, which stops once no previous command is left.
pub fn sweep_orphans(launcher: Arc<AdoptingLauncher>, handles: Arc<crate::handles::Handles>) {
    let running = move |handle: &Handle| handles.is_running(handle);
    launcher.end_orphans(&running);
    if launcher.waiting() == 0 {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("ignite-orphan-sweep".to_owned())
        .spawn(move || {
            while launcher.waiting() > 0 {
                std::thread::sleep(ORPHAN_SWEEP);
                launcher.end_orphans(&running);
            }
        });
    if let Err(error) = spawned {
        eprintln!("ignite: cannot start the thread that ends orphaned runs: {error}");
    }
}

/// Copies the files an adopted command wrote to paths its environment named, such as the results
/// file of a check (`IGNITE_RESULTS`), to the paths the second run's environment names for the
/// same variables, where the caller looks for them. A variable whose value is the same in both,
/// or whose first value is not a file, is left alone.
fn carry_outputs(previous: &[(String, String)], wanted: &[(String, String)]) {
    for (name, path) in wanted {
        let Some((_, written)) = previous.iter().find(|(n, _)| n == name) else {
            continue;
        };
        if written == path || !Path::new(written).is_file() {
            continue;
        }
        if let Err(error) = std::fs::copy(written, path) {
            eprintln!("ignite: {written} not copied to {path} for an adopted command: {error}");
        }
    }
}

fn same_dir(a: &Path, b: &Path) -> bool {
    a == b
        || matches!((ignite_os::paths::canonicalize(a), ignite_os::paths::canonicalize(b)), (Ok(a), Ok(b)) if a == b)
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
