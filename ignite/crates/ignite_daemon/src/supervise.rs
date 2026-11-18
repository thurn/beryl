//! Supervisors: one small process per command, its parent, which waits for it, measures it and
//! writes the result to a file, so the daemon can restart while commands run.
//!
//! Each command gets a run directory under the launcher's `runs` folder:
//!
//! - `spec.json`: what to run ([`SpecFile`]), written by the daemon before it starts the
//!   supervisor.
//! - `cores`: for a grant that shrinks or grows, its current size, which the scheduler rewrites
//!   when it changes and the command finds in `IGNITE_CORES_FILE`.
//! - `jobserver`: for a Cargo build that only compiles, the FIFO its job slots come from, kept
//!   at the size in `cores` (see `jobserver`).
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
//! result says it hung. The limit counts from the command's start, so time spent waiting for
//! cores does not count.
//!
//! The supervisor starts in a process group of its own, so ending the daemon (or its group)
//! leaves the command running. The command runs in a second group, led by itself, so that
//! everything it starts can be ended at once and measured together. On Windows the groups are
//! job objects: the supervisor starts apart from the daemon's console, and the command's job,
//! named after its process ID, is what the `group` file identifies.

use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ignite_core::ids::TicketId;
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::protocol::{Class, Usage};
use ignite_core::traits::{CommandSpec, Launcher, RunResult};
use ignite_os::Native;
use ignite_os::proc::{Group, Processes};
use ignite_os::usage::{ProcessUsage, TreeMeter, UsageReader};
use serde::{Deserialize, Serialize};

use crate::sched::{Grant, GrantRequest, Scheduler};
use crate::server::{Context, Verb};

mod grants;
mod hang;
mod jobserver;
mod peaks;
mod retain;

pub use grants::{GrantFile, STOP_GRACE_S};
use grants::{GrantGuard, watch_for_stop};
use hang::{Watchdog, signal_group};
use jobserver::{Jobserver, compiles_only, read_cores};
use peaks::Peaks;
pub use retain::{KEEP_JUDGED_S, KEEP_S, keep};

/// The line a supervisor prints on its standard output once it holds its run's lock. The
/// launcher reads lines until it sees this one, so other output before it does no harm.
pub const READY: &str = "ignite-supervisor-ready";

const SPEC: &str = "spec.json";
const CORES: &str = "cores";
const LOCK: &str = "lock";
const OUTPUT: &str = "output.log";
const RESULT: &str = "result.json";
const COLLECTED: &str = "collected";
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
    /// Variables added to the supervisor's environment, the grant's thread counts included.
    pub env: Vec<(String, String)>,
    /// Seconds the command waited for its grant, copied into its usage.
    pub queued_s: f64,
    /// For a Cargo build that only compiles, the grant's cores file: the supervisor runs the
    /// build under a jobserver of its own that follows the file.
    #[serde(default)]
    pub jobserver: Option<PathBuf>,
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

/// Runs commands by starting a supervisor for each.
pub struct SupervisedLauncher {
    /// The program and arguments that start a supervisor; the spec file's path is appended.
    supervisor: Vec<String>,
    runs: PathBuf,
    scheduler: Option<Arc<Scheduler>>,
    next: AtomicU64,
    /// When old run directories were last deleted, in seconds since the Unix epoch.
    pruned: AtomicU64,
    /// The memory each command line usually needs, for the scheduler's memory gate.
    peaks: Peaks,
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
            runs,
            scheduler: None,
            next: AtomicU64::new(0),
            pruned: AtomicU64::new(0),
        }
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
    /// follows, if any.
    fn start_in(
        &self,
        dir: PathBuf,
        spec: &CommandSpec,
        env: Vec<(String, String)>,
        queued_s: f64,
        jobserver: Option<PathBuf>,
        grant: Option<GrantFile>,
    ) -> io::Result<Started> {
        let mut all_env = spec.env.clone();
        all_env.extend(env);
        let file = SpecFile {
            args: spec.args.clone(),
            cwd: spec.cwd.clone(),
            env: all_env,
            queued_s,
            jobserver,
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

    /// Asks the scheduler for the command's cores. A Cargo build that only compiles shrinks
    /// while a higher class waits and grows back, through its jobserver; a command that rereads
    /// its cores file grows. Either keeps its size in `cores_file`.
    fn grant(&self, spec: &CommandSpec, shrinks: bool, cores_file: Option<PathBuf>) -> Grant {
        match &self.scheduler {
            Some(scheduler) => scheduler.acquire(&GrantRequest {
                class: spec.class,
                cores: spec.cores,
                resources: spec.resources.clone(),
                // The median peak of the command's recent runs, so that the scheduler holds a
                // command back while less than that is free.
                memory_mb: self.usual_memory(&spec.args),
                shrinks,
                grows: cores_file.is_some(),
                cores_file,
            }),
            None => Grant {
                id: 0,
                cores: spec.cores.max(1),
                resources: spec.resources.clone(),
                queued_s: 0,
                cores_file: None,
            },
        }
    }
}

impl Launcher for SupervisedLauncher {
    fn run(&self, spec: &CommandSpec) -> RunResult {
        // The run directory comes before the grant, because a grant that changes keeps its size
        // in a file there from the moment it is granted.
        let dir = match self.new_run() {
            Ok(dir) => dir,
            Err(error) => return not_started(spec, &error),
        };
        let shrinks = compiles_only(&spec.args);
        let cores_file =
            (self.scheduler.is_some() && (shrinks || spec.grows)).then(|| dir.join(CORES));
        // Released when this function ends, however it ends.
        let held = GrantGuard::new(
            self.scheduler.clone(),
            self.grant(spec, shrinks, cores_file.clone()),
        );
        let grant = held.grant();
        let jobserver = cores_file.filter(|_| shrinks);
        let file = GrantFile {
            class: spec.class,
            cores: grant.cores,
            resources: grant.resources.clone(),
        };
        let result = match self.start_in(
            dir,
            spec,
            grant.env(),
            grant.queued_s as f64,
            jobserver,
            Some(file),
        ) {
            Ok(started) => {
                // The scheduler stops `idle` work when a higher class waits or holds what it
                // needs; stopping ends the command. The thread ends when the grant is released.
                if let (Class::Idle, Some(scheduler)) = (spec.class, &self.scheduler) {
                    watch_for_stop(scheduler, grant.id, started.run().clone());
                }
                started.wait()
            }
            Err(error) => not_started(spec, &error),
        };
        drop(held);
        if result.exit.is_some() && !result.hung {
            self.peaks
                .record(&peaks::key(&spec.args), result.usage.peak_mb);
        }
        self.prune_now_and_then();
        result
    }
}

/// The result of a command whose supervisor could not be started.
fn not_started(spec: &CommandSpec, error: &io::Error) -> RunResult {
    eprintln!(
        "ignite: cannot start a supervisor for {:?}: {error}",
        spec.args
    );
    RunResult {
        exit: None,
        output: PathBuf::new(),
        usage: Usage::default(),
        hung: false,
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

    /// Ends the command and every process in its group, with `SIGTERM` (on Windows, by ending
    /// its job object, which stops its processes at once). Its supervisor then
    /// writes a result with no exit code. Does nothing to a run whose command has not started.
    pub fn end(&self) -> io::Result<()> {
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
        // The supervisor died without a result, so nothing supervises its command any more.
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
    writeln!(stdout, "{READY}")?;
    stdout.flush()?;
    drop(stdout);

    let watchdog = match (&group, spec.hang_limit_s) {
        (Ok(group), Some(limit)) => Some(Watchdog::start(group.id, limit, output.try_clone()?)),
        _ => None,
    };
    let measured = group.and_then(wait_measured);
    let hung = watchdog.is_some_and(Watchdog::finish);
    let (exit, usage) = match measured {
        Ok((status, usage)) => (exit_code(status), usage),
        Err(error) => {
            writeln!(&output, "ignite: cannot run {:?}: {error}", spec.args)?;
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
    write_json(&dir.join(RESULT), &result)?;
    // The lock is released when the process exits, after the result exists.
    drop(lock);
    Ok(())
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

/// The jobserver for a Cargo build, with room for the grant's size when it started: the number
/// in its cores file, or else `IGNITE_CORES`. Where none can be made (Windows has no FIFOs) the
/// build runs on `CARGO_BUILD_JOBS`, and the reason is in its output.
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
/// Under a jobserver, `CARGO_BUILD_JOBS` is removed, because Cargo ignores the jobserver while
/// it is set.
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
        .envs(spec.env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(output.try_clone()?);
    if let Some((name, value)) = jobserver {
        command.env_remove("CARGO_BUILD_JOBS").env(name, value);
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

/// `ignite run`: runs any heavy command with cores from the daemon, and records it.
pub struct RunVerb;

/// The lines of a command's output that `ignite run` prints, from its end.
const TAIL_LINES: usize = 30;

/// `ignite run`'s arguments: `[--class C] [--ticket ID] [--cores N] [--grows] -- <command>`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct RunArgs {
    class: Class,
    ticket: Option<TicketId>,
    cores: Option<u32>,
    /// The command rereads `IGNITE_CORES_FILE` while it runs, so its grant may grow.
    grows: bool,
    command: Vec<String>,
}

fn parse_run_args(args: &[String]) -> Result<RunArgs, String> {
    let mut parsed = RunArgs {
        class: Class::Agent,
        ticket: None,
        cores: None,
        grows: false,
        command: Vec::new(),
    };
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut value = || {
            rest.next()
                .cloned()
                .ok_or_else(|| format!("{arg} needs a value"))
        };
        match arg.as_str() {
            "--" => {
                parsed.command = rest.cloned().collect();
                break;
            }
            "--class" => {
                let name = value()?;
                parsed.class = serde_json::from_value(serde_json::Value::String(name.clone()))
                    .map_err(|_| format!("unknown class: {name}"))?;
            }
            "--ticket" => {
                let id = value()?;
                parsed.ticket = Some(TicketId::new(id).map_err(|e| e.to_string())?);
            }
            "--cores" => {
                let n = value()?;
                parsed.cores = Some(n.parse().map_err(|_| format!("not a core count: {n}"))?);
            }
            "--grows" => parsed.grows = true,
            other => return Err(format!("unexpected argument: {other}")),
        }
    }
    if parsed.command.is_empty() {
        return Err(
            "usage: ignite run [--class C] [--ticket ID] [--cores N] [--grows] -- <command>".into(),
        );
    }
    Ok(parsed)
}

impl Verb for RunVerb {
    fn name(&self) -> &'static str {
        "run"
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        let args = match parse_run_args(&req.args) {
            Ok(args) => args,
            Err(message) => return Reply::text(Exit::Usage, message),
        };
        // The scheduler holds a command to two thirds of the machine and shares what is free,
        // so asking for every core means "as many as I may have".
        let cores = args.cores.unwrap_or_else(|| {
            thread::available_parallelism().map_or(1, |n| u32::try_from(n.get()).unwrap_or(1))
        });
        let spec = CommandSpec {
            args: args.command.clone(),
            cwd: req.cwd.clone(),
            env: Vec::new(),
            class: args.class,
            cores,
            resources: Vec::new(),
            grows: args.grows,
            tree: None,
            ticket: args.ticket,
            hang_limit_s: None,
        };
        let result = ctx.launcher.run(&spec);
        render(&args.command, &result)
    }
}

/// The reply for a finished command: the verdict, the end of its output and where the rest is.
fn render(command: &[String], result: &RunResult) -> Reply {
    let shown = command.join(" ");
    let shown = if shown.chars().count() > 60 {
        format!("{}...", shown.chars().take(57).collect::<String>())
    } else {
        shown
    };
    let status = match result.exit {
        Some(code) => format!("exit {code}"),
        None => "ended by a signal".to_owned(),
    };
    let (verdict, exit) = if result.exit == Some(0) {
        ("ran", Exit::Success)
    } else {
        ("failed", Exit::CheckFailed)
    };
    let mut text = format!(
        "{verdict:<8} {shown} in {:.0} s: {status}\n",
        result.usage.wall_s
    );
    for line in tail(&result.output, TAIL_LINES) {
        text.push_str(&line);
        text.push('\n');
    }
    let _ = writeln!(text, "output   {}", result.output.display());
    Reply {
        exit,
        text,
        json: Some(serde_json::json!({
            "exit": result.exit,
            "output": result.output,
            "usage": result.usage,
        })),
        handle: None,
    }
}

/// The last `lines` lines of a file, read from at most its last 64 KiB.
fn tail(path: &Path, lines: usize) -> Vec<String> {
    const WINDOW: u64 = 64 * 1024;
    let Ok(mut file) = File::open(path) else {
        return Vec::new();
    };
    let len = file.metadata().map_or(0, |m| m.len());
    if file
        .seek(SeekFrom::Start(len.saturating_sub(WINDOW)))
        .is_err()
    {
        return Vec::new();
    }
    let mut bytes = Vec::new();
    if file.read_to_end(&mut bytes).is_err() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&bytes);
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..]
        .iter()
        .map(|line| (*line).to_owned())
        .collect()
}

#[cfg(test)]
mod tests;
