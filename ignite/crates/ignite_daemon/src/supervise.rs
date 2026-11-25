//! Supervisors: one small process per command, its parent, which waits for it, measures it and
//! writes the result to a file, so the daemon can restart while commands run.
//!
//! Each command gets a run directory under the launcher's `runs` folder:
//!
//! - `spec.json`: what to run ([`SpecFile`]), written by the daemon before it starts the
//!   supervisor.
//! - `cores`: for a command run under the scheduler, its grant's current size, which the
//!   scheduler rewrites when it changes and the command finds in `IGNITE_CORES_FILE`.
//! - `jobserver`: for a command run under the scheduler, the FIFO that the Cargo builds it runs
//!   take their job slots from, kept at the size in `cores` (see `jobserver`).
//! - `lock`: held exclusively by the supervisor for as long as it lives. Whoever wants the
//!   result takes a shared lock on it, which blocks until the supervisor exits. This works for
//!   a daemon that started the supervisor and for one that restarted since and is no longer its
//!   parent, with no polling.
//! - `group`: the command's process group ID, written by the supervisor once it has started it.
//! - `output.log`: the command's standard output and standard error, interleaved.
//! - `result.json`: written by the supervisor, by rename, once the command has exited
//!   ([`ResultFile`]).
//! - `collected`: written by the daemon once it has taken the result, so a restarted daemon
//!   skips the run.
//! - `output.keep`: written by whoever judged the run failed, flaky or incomplete ([`keep`]), so
//!   its output is kept for [`KEEP_JUDGED_S`] instead of [`KEEP_S`] ([`SupervisedLauncher::prune`]).
//!
//! When the command's leader exits, the supervisor ends every process still in the command's
//! group, as the job object does on Windows, so a background process a build leaves behind does
//! not outlive it. A supervisor that dies without writing a result leaves its command running;
//! the daemon that waits for it ends the group.
//!
//! A command with a hang limit that runs past it is ended by its supervisor with everything it
//! started, after the supervisor has written the processes still running to its output. Its
//! result says it hung. The timer starts after the command starts, so time spent waiting for
//! cores does not count. Check commands restart the timer when they write to the output log.
//!
//! The supervisor starts in a process group of its own, so ending the daemon (or its group)
//! leaves the command running. The command runs in a second group, led by itself, so that
//! everything it starts can be ended at once and measured together. On Windows the groups are
//! job objects: the supervisor starts apart from the daemon's console, and the command's job,
//! named after its process ID, is what the `group` file identifies.

use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ignite_core::protocol::Usage;
use ignite_core::traits::{CommandSpec, Launcher, RunResult};
use ignite_os::Native;
use ignite_os::proc::{Group, Processes};
use ignite_os::usage::{ProcessUsage, TreeMeter, UsageReader};
use serde::{Deserialize, Serialize};

use crate::sched::Scheduler;

mod begin;
mod cancel;
mod grants;
mod hang;
mod jobserver;
pub(crate) mod peaks;
mod priority;
mod requests;
mod retain;
mod run_verb;

use begin::Launch;
pub use begin::grows_here;
use cancel::{end_when_cancelled, not_started, stopped};
pub use grants::{GrantFile, STOP_GRACE_S};
pub(crate) use hang::HANG_ON_OUTPUT_IDLE;
use hang::{Watchdog, signal_group};
use jobserver::{Jobserver, read_cores};
use peaks::Peaks;
use priority::AgentPriority;
pub use requests::{request_of, requests_in, unfinished_of_in};
pub use retain::{KEEP_JUDGED_S, KEEP_S, keep};
pub use run_verb::{DEFAULT_HANG_LIMIT_S, RunVerb};

/// The line a supervisor prints on its standard output once it holds its run's lock. The
/// launcher reads lines until it sees this one, so other output before it does no harm.
pub const READY: &str = "ignite-supervisor-ready";

const SPEC: &str = "spec.json";
const CORES: &str = "cores";
const LOCK: &str = "lock";
const OUTPUT: &str = "output.log";
const RESULT: &str = "result.json";
const COLLECTED: &str = "collected";
/// Written by [`Run::end`] before it signals the command, so a command that catches `SIGTERM`
/// and exits with a code of its own (`uv run` exits 143) still reads as ended from outside.
const ENDED: &str = "ended";
/// The process group ID of the command, which ends it and everything it started.
const GROUP: &str = "group";
/// The supervisor's own standard error, for a supervisor that fails before it can write a
/// result.
const SUPERVISOR_LOG: &str = "supervisor.log";

/// How often a supervisor reads the command's descendants while it runs. The operating system
/// totals only a direct child exactly, so a descendant is counted up to its last reading, and
/// one that starts and ends between two readings is not counted ("Cost in instructions" in
/// `docs/ignite.md`). A reading lists the group and reads each process's record, which costs
/// well under a millisecond, so it is taken often.
const SAMPLE_EVERY: Duration = Duration::from_millis(250);

/// What a supervisor runs: the `spec.json` of a run directory.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpecFile {
    pub args: Vec<String>,
    pub cwd: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tree_path: Option<PathBuf>,
    /// Variables added to the supervisor's environment, the grant's thread counts included.
    pub env: Vec<(String, String)>,
    /// Seconds the command waited for its grant, copied into its usage.
    pub queued_s: f64,
    /// The grant's cores file, for a command run under the scheduler: the supervisor runs the
    /// command under a jobserver of its own that follows the file, which every Cargo build the
    /// command starts takes its job slots from.
    #[serde(default)]
    pub jobserver: Option<PathBuf>,
    /// `CARGO_BUILD_JOBS` under the jobserver: the most cores one command may hold. Cargo takes
    /// its job slots from an inherited jobserver whatever this says, but passes it to build
    /// scripts as `NUM_JOBS`, so it must not be the grant's first size. Without a jobserver the
    /// grant's first size in `env` stays.
    #[serde(default)]
    pub build_jobs: Option<u32>,
    /// Seconds after which the supervisor ends the command; `None` for no limit.
    #[serde(default)]
    pub hang_limit_s: Option<u64>,
    /// The grant the command runs under, so a restarted daemon can register it again.
    #[serde(default)]
    pub grant: Option<GrantFile>,
}

/// How a supervised command ended: the `result.json` of a run directory.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResultFile {
    /// The exit code; `None` if a signal ended it.
    pub exit: Option<i32>,
    pub output: PathBuf,
    pub usage: Usage,
    /// The supervisor ended the command at its hang limit.
    #[serde(default)]
    pub hung: bool,
}

impl ResultFile {
    /// Whether the command whose whole output is `output` was ended from outside before its
    /// hang limit, as when the scheduler stops `idle` work for a higher class or a daemon
    /// restart cuts it: [`Run::end`] asked it to stop, or a signal ended it. Such a failure
    /// says nothing about the source it built. `false` when `output` is not in a run directory
    /// or the run wrote no result.
    pub fn ended_from_outside(output: &Path) -> bool {
        let Some(dir) = output.parent() else {
            return false;
        };
        fs::read(dir.join(RESULT))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<ResultFile>(&bytes).ok())
            .is_some_and(|result| {
                !result.hung && (result.exit.is_none() || dir.join(ENDED).exists())
            })
    }
}

/// Runs commands by starting a supervisor for each.
pub struct SupervisedLauncher {
    /// The program and arguments that start a supervisor; the spec file's path is appended.
    supervisor: Vec<String>,
    runs: PathBuf,
    scheduler: Option<Arc<Scheduler>>,
    agent_priority: Option<AgentPriority>,
    next: AtomicU64,
    /// When old run directories were last deleted, in seconds since the Unix epoch.
    pruned: AtomicU64,
    /// The memory each command line usually needs, for the scheduler's memory gate.
    peaks: Peaks,
    /// The CPU milliseconds of each command line's last runs, kept the same way, which tell
    /// a quick command (see `begin::QUICK_CPU_MS`).
    cpu: Peaks,
    /// Seconds an `idle` command has to exit after the scheduler stops it, before it is killed.
    stop_grace_s: f64,
    tree_control: std::sync::OnceLock<crate::trees::TreeBuildControl>,
}

impl SupervisedLauncher {
    /// A launcher that starts supervisors with `<ignite> supervise <spec>` and keeps run
    /// directories under `runs`.
    pub fn new(ignite: &Path, runs: PathBuf) -> Self {
        Self::with_command(
            vec![ignite.display().to_string(), "supervise".to_owned()],
            runs,
        )
    }

    /// A launcher whose supervisor is started by `command` followed by the spec file's path.
    pub fn with_command(command: Vec<String>, runs: PathBuf) -> Self {
        Self {
            supervisor: command,
            peaks: Peaks::load(runs.join("peaks.json")),
            cpu: Peaks::load(runs.join("cpu.json")),
            runs,
            scheduler: None,
            agent_priority: None,
            next: AtomicU64::new(0),
            pruned: AtomicU64::new(0),
            stop_grace_s: STOP_GRACE_S,
            tree_control: std::sync::OnceLock::new(),
        }
    }

    pub(crate) fn attach_trees(&self, trees: &crate::trees::Trees) {
        assert!(self.tree_control.set(trees.build_control()).is_ok());
    }

    /// Gives a stopped `idle` command `seconds` (of the scheduler's clock) to exit instead of
    /// [`STOP_GRACE_S`]. For tests of the kill that follows the grace.
    #[must_use]
    pub fn with_stop_grace(mut self, seconds: f64) -> Self {
        self.stop_grace_s = seconds;
        self
    }

    /// Takes cores and resources from `scheduler` before each run, and has it read the machine
    /// about once a second, which is also how often a shrinking grant gives back a core. Without
    /// one, a command gets the cores it asked for.
    #[must_use]
    pub fn with_scheduler(mut self, scheduler: Arc<Scheduler>) -> Self {
        Scheduler::sample_every(&scheduler);
        self.scheduler = Some(scheduler);
        self
    }

    /// Writes the run directory and starts its supervisor, returning once the supervisor holds
    /// the run's lock. `env` is added to the spec's own variables.
    pub fn start(
        &self,
        spec: &CommandSpec,
        env: Vec<(String, String)>,
        queued_s: f64,
    ) -> io::Result<Started> {
        let dir = self.new_run()?;
        self.start_in(dir, spec, env, queued_s, None, None)
    }

    /// Makes a new, empty run directory.
    fn new_run(&self) -> io::Result<PathBuf> {
        let dir = self.runs.join(self.run_name());
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    /// [`Self::start`] in run directory `dir`, made already, with the cores file a jobserver
    /// follows and the `CARGO_BUILD_JOBS` under it, if any.
    fn start_in(
        &self,
        dir: PathBuf,
        spec: &CommandSpec,
        env: Vec<(String, String)>,
        queued_s: f64,
        jobserver: Option<(PathBuf, u32)>,
        grant: Option<GrantFile>,
    ) -> io::Result<Started> {
        let mut all_env = spec.env.clone();
        all_env.extend(env);
        let file = SpecFile {
            args: spec.args.clone(),
            cwd: spec.cwd.clone(),
            tree_path: self
                .tree_control
                .get()
                .and_then(crate::trees::TreeBuildControl::trees)
                .and_then(|trees| trees.tree_at(&spec.cwd).and_then(|tree| trees.path(&tree))),
            env: all_env,
            queued_s,
            jobserver: jobserver.as_ref().map(|(file, _)| file.clone()),
            build_jobs: jobserver.map(|(_, jobs)| jobs),
            hang_limit_s: spec.hang_limit_s,
            grant,
        };
        write_json(&dir.join(SPEC), &file)?;
        let (program, rest) = self
            .supervisor
            .split_first()
            .ok_or_else(|| io::Error::other("no supervisor command"))?;
        let mut command = Command::new(program);
        Native.detach(&mut command);
        let mut child = command
            .args(rest)
            .arg(dir.join(SPEC))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(File::create(dir.join(SUPERVISOR_LOG))?)
            .spawn()?;
        if let Some(stdout) = child.stdout.take() {
            let ready = BufReader::new(stdout)
                .lines()
                .map_while(Result::ok)
                .any(|line| line == READY);
            if !ready {
                // It exited without taking the lock; the run reads as lost.
                let _ = child.wait();
            }
        }
        Ok(Started {
            run: Run { dir },
            child,
        })
    }

    /// The runs whose results nobody has collected: those a previous daemon started. Each one's
    /// [`Run::wait`] returns its result once its command has finished.
    pub fn unfinished(&self) -> Vec<Run> {
        let Ok(entries) = fs::read_dir(&self.runs) else {
            return Vec::new();
        };
        let mut runs: Vec<Run> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|dir| dir.join(SPEC).is_file() && !dir.join(COLLECTED).exists())
            .map(|dir| Run { dir })
            .collect();
        runs.sort_by(|a, b| a.dir.cmp(&b.dir));
        runs
    }

    /// The memory in megabytes that runs of `args` usually peak at, for the scheduler.
    fn usual_memory(&self, args: &[String]) -> Option<u64> {
        self.peaks.usual(&peaks::key(args))
    }

    /// A run directory name that sorts by start time and is unique across daemon restarts.
    fn run_name(&self) -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let count = self.next.fetch_add(1, Ordering::Relaxed);
        format!("{nanos:020}-{}-{count}", std::process::id())
    }
}

impl Launcher for SupervisedLauncher {
    fn run(&self, spec: &CommandSpec) -> RunResult {
        self.run_with(spec, None)
    }

    fn run_until(&self, spec: &CommandSpec, cancelled: &(dyn Fn() -> bool + Sync)) -> RunResult {
        self.run_with(spec, Some(cancelled))
    }
}

impl SupervisedLauncher {
    /// [`Launcher::run`], with the command ended when `cancelled`, if given, turns true.
    fn run_with(
        &self,
        spec: &CommandSpec,
        cancelled: Option<&(dyn Fn() -> bool + Sync)>,
    ) -> RunResult {
        // The work this command is part of (see `crate::steps`). Once someone asks that work to
        // stop, or its owner finishes, its commands are ended and no new one starts.
        let activity = crate::steps::current();
        let either =
            || cancelled.is_some_and(|c| c()) || activity.as_ref().is_some_and(|a| a.cancelled());
        let cancelled: Option<&(dyn Fn() -> bool + Sync)> =
            (cancelled.is_some() || activity.is_some()).then_some(&either);
        if either() {
            return stopped(spec);
        }
        let result = match self.launch(spec, None, &either) {
            Launch::Started(started, held) => {
                let run = started.run().clone();
                let (done, finished) = mpsc::channel::<()>();
                let result = thread::scope(|scope| {
                    if let Some(cancelled) = cancelled {
                        let run = &run;
                        scope.spawn(move || end_when_cancelled(run, cancelled, &finished));
                    }
                    let result = started.wait();
                    // Wakes the thread that watches for the caller to leave.
                    drop(done);
                    result
                });
                drop(held);
                result
            }
            Launch::Cancelled => return stopped(spec),
            Launch::Failed(error) => not_started(spec, &error),
        };
        self.finished(&spec.args, &result);
        result
    }
}

/// A run this daemon started; its supervisor is this process's child.
#[derive(Debug)]
pub struct Started {
    run: Run,
    child: Child,
}

impl Started {
    pub fn run(&self) -> &Run {
        &self.run
    }

    /// Waits for the supervisor and returns the command's result.
    pub fn wait(mut self) -> RunResult {
        let _ = self.child.wait();
        self.run.wait()
    }

    #[cfg(test)]
    pub(crate) fn supervisor_pid(&self) -> u32 {
        self.child.id()
    }

    /// Simulates the supervisor dying before it can save a result.
    #[cfg(test)]
    pub(crate) fn kill_supervisor_for_test(&mut self) {
        self.child.kill().unwrap();
        self.child.wait().unwrap();
    }

    /// Ends a test command that might still be waiting and reaps its supervisor.
    #[cfg(test)]
    pub(crate) fn stop_for_test(mut self) -> io::Result<()> {
        if self.child.try_wait()?.is_none() && !self.run.dir.join(RESULT).exists() {
            self.run.kill()?;
        }
        self.child.wait()?;
        self.run.wait();
        Ok(())
    }
}

/// One run directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    dir: PathBuf,
}

impl Run {
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// A pre-upgrade supervisor that never acquired the tree command lock.
    pub(crate) fn legacy_cwd(&self) -> Option<PathBuf> {
        let spec: SpecFile = serde_json::from_slice(&fs::read(self.dir.join(SPEC)).ok()?).ok()?;
        (spec.tree_path.is_none() && self.is_running()).then_some(spec.cwd)
    }

    pub(crate) fn is_running(&self) -> bool {
        File::open(self.dir.join(LOCK))
            .is_ok_and(|guard| matches!(guard.try_lock_shared(), Err(fs::TryLockError::WouldBlock)))
    }

    /// Ends the command and every process in its group, with `SIGTERM` (on Windows, by ending
    /// its job object, which stops its processes at once). Its supervisor then writes a result
    /// with no exit code, or with the code of a command that caught the signal. Either way the
    /// run is marked ended first, for [`ResultFile::ended_from_outside`]. Does nothing to a run
    /// whose command has not started.
    pub fn end(&self) -> io::Result<()> {
        if self.dir.join(GROUP).exists() {
            fs::write(self.dir.join(ENDED), b"")?;
        }
        self.signal("TERM")
    }

    /// Kills the command and every process in its group at once, with `SIGKILL`. Does nothing
    /// to a run whose command has not started.
    pub fn kill(&self) -> io::Result<()> {
        self.signal("KILL")
    }

    fn signal(&self, signal: &str) -> io::Result<()> {
        let Ok(group) = fs::read_to_string(self.dir.join(GROUP)) else {
            return Ok(());
        };
        let group: u64 = group
            .trim()
            .parse()
            .map_err(|_| io::Error::other("the group file holds no process group ID"))?;
        signal_group(group, signal)
    }

    /// Blocks until the run's supervisor has exited, then returns the result it wrote and marks
    /// the run collected. A supervisor that died without writing one gives a result with no
    /// exit code, whose output names the supervisor's own log; its command, which nobody
    /// supervises any more, is killed with everything it started.
    pub fn wait(&self) -> RunResult {
        if let Ok(lock) = File::open(self.dir.join(LOCK)) {
            let _ = lock.lock_shared();
        }
        let result = fs::read(self.dir.join(RESULT))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<ResultFile>(&bytes).ok());
        let _ = File::create(self.dir.join(COLLECTED));
        if let Some(file) = result {
            return RunResult {
                exit: file.exit,
                output: file.output,
                usage: file.usage,
                hung: file.hung,
            };
        }
        let log = fs::read_to_string(self.dir.join(SUPERVISOR_LOG)).unwrap_or_default();
        if log.contains(RESULT_NOT_WRITTEN) {
            // The supervisor was alive and the command had finished, but the result could not
            // be written. The command's own output is the best answer there is.
            eprintln!(
                "ignite: the supervisor of {} could not record its result; the exit code is lost",
                self.dir.display()
            );
            return RunResult {
                exit: None,
                output: self.dir.join(OUTPUT),
                usage: Usage::default(),
                hung: false,
            };
        }
        // The supervisor died without a result, so nothing supervises its command any more.
        eprintln!(
            "ignite: the supervisor of {} ended without a result; ending its command",
            self.dir.display()
        );
        let _ = self.kill();
        RunResult {
            exit: None,
            output: self.dir.join(SUPERVISOR_LOG),
            usage: Usage::default(),
            hung: false,
        }
    }
}

/// The hidden `ignite supervise <spec file>` subcommand; returns the process's exit code.
pub fn supervise(spec_file: &Path) -> i32 {
    match supervise_run(spec_file) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("ignite supervise {}: {error}", spec_file.display());
            1
        }
    }
}

fn supervise_run(spec_file: &Path) -> io::Result<()> {
    let dir = spec_file
        .parent()
        .ok_or_else(|| io::Error::other("the spec file has no directory"))?;
    let spec: SpecFile = serde_json::from_slice(&fs::read(spec_file)?)?;
    let lock = File::create(dir.join(LOCK))?;
    lock.try_lock()
        .map_err(|_| io::Error::other("another supervisor holds this run's lock"))?;
    let output_path = dir.join(OUTPUT);
    let output = File::create(&output_path)?;
    let tree_lock = spec
        .tree_path
        .as_deref()
        .map(crate::trees::command_lock)
        .transpose()?;
    if let Some(lock) = &tree_lock {
        lock.lock_shared()?;
    }
    let started = Instant::now();
    let jobserver = spec
        .jobserver
        .as_deref()
        .and_then(|cores_file| open_jobserver(&spec, cores_file, dir, &output));
    let group = start_command(&spec, jobserver.as_ref().map(Jobserver::env), &output, dir);
    // Dropping the sender stops the thread that keeps the jobserver at the grant's size.
    let _follow = jobserver
        .zip(spec.jobserver.clone())
        .map(|(jobserver, cores_file)| jobserver::follow(jobserver, cores_file));
    // Announced once the command has started (or failed to), so when the launcher's `start`
    // returns, the lock is held and the group file exists. Written straight to the file
    // descriptor, so that a test harness capturing `print!` cannot hold it back.
    let mut stdout = io::stdout().lock();
    // The launcher may have stopped reading already. The command goes on regardless, and its
    // result is waited for by whoever finds the run directory.
    let _ = writeln!(stdout, "{READY}").and_then(|()| stdout.flush());
    drop(stdout);

    let output_idle = spec
        .env
        .iter()
        .any(|(name, value)| name == HANG_ON_OUTPUT_IDLE && value == "1");
    let watchdog = match (&group, spec.hang_limit_s) {
        (Ok(group), Some(limit)) => {
            match output.try_clone().or_else(|_| append_to(&output_path)) {
                Ok(copy) => Some(Watchdog::start(group.id, limit, copy, output_idle)),
                Err(error) => {
                    // Not worth giving up the command's result for: it runs without a limit.
                    let _ = writeln!(
                        &output,
                        "ignite: its hang limit is not enforced: cannot open the output: {error}"
                    );
                    None
                }
            }
        }
        _ => None,
    };
    let measured = group.and_then(wait_measured);
    let hung = watchdog.is_some_and(Watchdog::finish);
    let (exit, usage) = match measured {
        Ok((status, usage)) => (exit_code(status), usage),
        Err(error) => {
            let _ = writeln!(&output, "ignite: cannot run {:?}: {error}", spec.args);
            (Some(127), ProcessUsage::default())
        }
    };
    let result = ResultFile {
        exit,
        output: output_path,
        usage: Usage {
            queued_s: spec.queued_s,
            wall_s: started.elapsed().as_secs_f64(),
            cpu_s: usage.cpu_s,
            instructions: usage.instructions,
            peak_mb: usage.peak_bytes / (1024 * 1024),
        },
        hung,
    };
    if let Err(error) = write_result(dir, &result, RESULT_RETRY_PAUSE) {
        // The command has finished, so its output says how, for whoever reads it.
        let _ = writeln!(
            &output,
            "ignite: the command {}, but its result could not be recorded: {error}",
            match result.exit {
                Some(code) => format!("exited with code {code}"),
                None => "was ended by a signal".to_owned(),
            }
        );
        return Err(io::Error::other(format!("{RESULT_NOT_WRITTEN}: {error}")));
    }
    // The lock is released when the process exits, after the result exists.
    drop(lock);
    Ok(())
}

/// How many times the supervisor tries to write the result, and how long it waits between
/// tries: a full disk may have room again a moment later.
const RESULT_ATTEMPTS: u32 = 5;
const RESULT_RETRY_PAUSE: Duration = Duration::from_secs(2);
/// What the supervisor's log says when it could not write the result, so that a reader can
/// tell that apart from a supervisor that was killed.
const RESULT_NOT_WRITTEN: &str = "could not write result.json";

/// Writes `result` into `dir`, trying again after `pause` if the write fails.
fn write_result(dir: &Path, result: &ResultFile, pause: Duration) -> io::Result<()> {
    let mut attempt = 1;
    loop {
        match write_json(&dir.join(RESULT), result) {
            Ok(()) => return Ok(()),
            Err(error) if attempt >= RESULT_ATTEMPTS => return Err(error),
            Err(_) => {
                // The half-written file may be what filled the disk.
                let _ = fs::remove_file(dir.join(RESULT).with_extension("partial"));
                thread::sleep(pause);
                attempt += 1;
            }
        }
    }
}

fn append_to(path: &Path) -> io::Result<File> {
    fs::OpenOptions::new().append(true).open(path)
}

/// The command's exit code, or `None` if it was ended from outside: by a signal on macOS, or
/// on Windows with [`ignite_os::proc::ENDED_EXIT_CODE`].
fn exit_code(status: ExitStatus) -> Option<i32> {
    #[cfg(windows)]
    if status.code() == Some(ignite_os::proc::ENDED_EXIT_CODE as i32) {
        return None;
    }
    status.code()
}

/// The jobserver for a command, with room for the grant's size when it started: the number
/// in its cores file, or else `IGNITE_CORES`. Where none can be made the command runs on the
/// grant's first size in `CARGO_BUILD_JOBS`, and the reason is in its output.
fn open_jobserver(
    spec: &SpecFile,
    cores_file: &Path,
    dir: &Path,
    output: &File,
) -> Option<Jobserver> {
    let cores = read_cores(cores_file)
        .or_else(|| {
            spec.env
                .iter()
                .find(|(name, _)| name == "IGNITE_CORES")
                .and_then(|(_, value)| value.parse().ok())
        })
        .unwrap_or(1);
    match Jobserver::open(dir, cores) {
        Ok(jobserver) => Some(jobserver),
        Err(error) => {
            let _ = writeln!(
                &*output,
                "ignite: running without a jobserver, on the grant's first size: {error}"
            );
            None
        }
    }
}

/// Starts the command as the leader of a group of its own and records the group's ID in `dir`.
/// Under a jobserver, `CARGO_BUILD_JOBS` is the spec's `build_jobs`, so that the jobserver, not
/// the grant's first size, limits the build.
fn start_command(
    spec: &SpecFile,
    jobserver: Option<(String, String)>,
    output: &File,
    dir: &Path,
) -> io::Result<Group> {
    let (program, rest) = spec
        .args
        .split_first()
        .ok_or_else(|| io::Error::other("empty command"))?;
    let mut command = Command::new(program);
    command
        .args(rest)
        .current_dir(&spec.cwd)
        .envs(spec.env.iter().map(|(name, value)| (name, value)))
        .env_remove(HANG_ON_OUTPUT_IDLE)
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(output.try_clone()?);
    if let Some((name, value)) = jobserver {
        command.env(name, value);
        match spec.build_jobs {
            Some(jobs) => command.env("CARGO_BUILD_JOBS", jobs.to_string()),
            None => command.env_remove("CARGO_BUILD_JOBS"),
        };
    }
    let group = Native.spawn_group(&mut command)?;
    fs::write(dir.join(GROUP), group.id.to_string())?;
    Ok(group)
}

/// Waits for the command, sampling its group about once a second, and returns its exit status
/// with the whole group's use.
fn wait_measured(mut group: Group) -> io::Result<(ExitStatus, ProcessUsage)> {
    let meter = Arc::new(Mutex::new(TreeMeter::new(group.id, group.child.id())));
    let (stop, stopped) = mpsc::channel::<()>();
    let sampler = {
        let meter = Arc::clone(&meter);
        thread::spawn(move || {
            while let Err(mpsc::RecvTimeoutError::Timeout) = stopped.recv_timeout(SAMPLE_EVERY) {
                let _ = meter
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .sample();
            }
        })
    };
    let reaped = Native.reap(&mut group.child);
    drop(stop);
    let _ = sampler.join();
    // The leader has exited. Read what else is in its group one last time, then end it, as the
    // job object does on Windows: a process the command left running in the background must not
    // outlive it (and keep its files and cores busy).
    let _ = meter
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .sample();
    let _ = Native.end_group(&mut group);
    let (status, leader) = reaped?;
    let total = meter
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .finish(leader);
    Ok((status, total))
}

/// Writes `value` as JSON next to `path` and renames it into place, so a reader never sees a
/// partial file.
fn write_json<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let partial = path.with_extension("partial");
    fs::write(&partial, serde_json::to_vec(value)?)?;
    fs::rename(partial, path)
}

#[cfg(test)]
mod result_tests;
