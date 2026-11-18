//! The scheduler: the only place that decides which heavy command runs now. It grants cores by
//! class, holds shared resources to their capacity, keeps the `tip` and `batch` floors, and
//! counts load from outside Ignite (docs/ignite.md, "Cores, memory and shared resources").
//!
//! [`Ledger`] is the state machine and holds every rule. [`Scheduler`] wraps it for threads:
//! [`Scheduler::acquire`] blocks until a request is granted, and [`Scheduler::sample_every`]
//! reads the machine's CPU and memory about once a second ([`SAMPLE_EVERY`]).
//!
//! The rules:
//!
//! - Five classes in strict order (`land`, `agent`, `tip`, `batch`, `idle`). A lower class starts
//!   nothing while a higher one has a request waiting for cores.
//! - One command takes at most two thirds of the cores. Waiting requests of one class share
//!   what is free evenly.
//! - `tip` and `batch` have floors (`floor` in a machine's table of `ignite.toml`) that higher
//!   classes cannot take while the class has work running or waiting.
//! - Each named resource is held by at most its capacity of grants at once. A resource the
//!   machine does not declare has capacity 1.
//! - CPU busy beyond what Ignite granted belongs to processes outside Ignite and is not granted.
//! - `idle` work starts only while the machine is under 70% busy, and is the first thing stopped
//!   when a higher class waits for cores or for a resource the idle work holds. Stopping ends
//!   the command, so idle work must be safe to restart.
//! - `land`, and a class with a floor, always get one core while they hold none, even when
//!   processes outside Ignite fill the machine: their work must make progress.
//! - A request whose family usually needs more memory than is free waits while something else
//!   runs.
//! - A grant can change while its command runs. One that shrinks (a Cargo build that only
//!   compiles) gives a core back on each step, down to one, while a higher class waits. One with
//!   a cores file takes cores as they free up, to what it asked for, and the scheduler writes
//!   its current size to the file (`IGNITE_CORES_FILE`), which the command's jobserver follows
//!   and which a command that can use more threads rereads.
//! - A request that would start short of what it wants, while lower classes hold cores that
//!   shrink, waits up to [`RECLAIM_SECONDS`] for them before it takes what is free. A short
//!   growing grant of `land` counts as a waiter, so freed cores go to it first.
//! - Classes are enforced by grants only. Nothing here changes a process's priority: work run
//!   at low priority on a loaded machine once got almost no CPU for hours.

mod ledger;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};
use std::thread;
use std::time::Duration;

use ignite_core::config::{Floor, Machine};
use ignite_core::protocol::Class;
use ignite_core::traits::Clock;
use ignite_os::hostcpu::HostCpu;
use serde::Serialize;

pub use ledger::{Ledger, RECLAIM_SECONDS, Step};

/// How often [`Scheduler::sample_every`] reads the machine. Each reading is also a step, and a
/// shrinking grant gives back one core per step, so a waiting landing gets a 12-core build's
/// cores within about 12 s.
pub const SAMPLE_EVERY: Duration = Duration::from_secs(1);

/// How often a request waiting for its grant asks whether it was cancelled.
pub const CANCEL_RECHECK: Duration = Duration::from_secs(1);
/// How long [`Scheduler::wait_released`] sleeps between looks at the clock.
const RECHECK: Duration = Duration::from_millis(100);

/// What a command asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrantRequest {
    pub class: Class,
    /// Cores wanted; the grant may be smaller. Zero is taken as one.
    pub cores: u32,
    pub resources: Vec<String>,
    /// The family's usual peak memory, if known.
    pub memory_mb: Option<u64>,
    /// The command can run on fewer cores than it started with, and gives them back while a
    /// higher class waits: a Cargo build that only compiles, whose jobs end within seconds.
    pub shrinks: bool,
    /// The command rereads its cores file while it runs and starts more threads as its grant
    /// grows, so it starts at once on what is free instead of waiting for shrinking grants.
    pub grows: bool,
    /// The file the scheduler keeps the grant's current size in, as a plain integer, from the
    /// moment it is granted. Named to the command by `IGNITE_CORES_FILE`. A grant with one grows
    /// as cores free up, to what it asked for: the command's jobserver follows the file.
    pub cores_file: Option<PathBuf>,
}

/// How an adopted grant keeps following its command after a daemon restart: the cores file its
/// jobserver reads, the cores it asked for, and whether it gives cores back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Follow {
    pub cores_file: PathBuf,
    pub asked: u32,
    pub shrinks: bool,
}

/// Cores and resources held until released with [`Scheduler::release`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    pub id: u64,
    /// The size when granted. A grant that shrinks or grows keeps its current size in its
    /// cores file.
    pub cores: u32,
    pub resources: Vec<String>,
    /// Seconds spent waiting for the grant, recorded apart from the command's own time.
    pub queued_s: u64,
    pub cores_file: Option<PathBuf>,
}

impl Grant {
    /// The variables that tell a command how many threads it may run.
    pub fn env(&self) -> Vec<(String, String)> {
        let cores = self.cores.to_string();
        [
            "CARGO_BUILD_JOBS",
            "NEXTEST_TEST_THREADS",
            "RUST_TEST_THREADS",
            "IGNITE_CORES",
        ]
        .into_iter()
        .map(|name| (name.to_owned(), cores.clone()))
        .chain(
            self.cores_file
                .iter()
                .map(|file| ("IGNITE_CORES_FILE".to_owned(), file.display().to_string())),
        )
        .collect()
    }
}

/// What one machine may hand out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Logical cores.
    pub cores: u32,
    pub floor: Floor,
    /// Each resource's capacity.
    pub resources: BTreeMap<String, u32>,
}

impl Limits {
    /// The limits of a machine declared in `ignite.toml` with `cores` logical cores.
    pub fn for_machine(machine: &Machine, cores: u32) -> Self {
        Self {
            cores,
            floor: machine.floor,
            resources: machine.resources.clone(),
        }
    }

    /// The most cores one command may hold: two thirds of the machine, and at least one.
    pub fn max_per_command(&self) -> u32 {
        (self.cores * 2 / 3).max(1)
    }

    /// A resource's capacity; one for a resource the machine does not declare, so a family that
    /// names an unknown resource runs one command at a time rather than never.
    pub fn capacity(&self, name: &str) -> u32 {
        self.resources.get(name).copied().unwrap_or(1)
    }
}

/// The data behind `ignite status` for cores and resources.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Status {
    pub cores: u32,
    /// Cores busy with processes outside Ignite at the last reading.
    pub outside: u32,
    /// Cores held by Ignite's commands.
    pub granted: u32,
    pub max_per_command: u32,
    pub free_memory_mb: Option<u64>,
    pub classes: Vec<ClassStatus>,
    pub resources: Vec<ResourceStatus>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ClassStatus {
    pub class: Class,
    /// Commands holding a grant.
    pub running: u32,
    pub cores: u32,
    /// Requests not granted yet.
    pub waiting: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ResourceStatus {
    pub name: String,
    pub capacity: u32,
    pub held: u32,
    pub waiting: u32,
}

/// The thread-safe scheduler the daemon shares among its request threads.
pub struct Scheduler {
    state: Mutex<State>,
    changed: Condvar,
    clock: Arc<dyn Clock>,
    host: Arc<dyn HostCpu + Send + Sync>,
    /// A sampler thread runs. One host reading serves every sample, and two samplers would each
    /// see the counters microseconds after the other's reading.
    sampling: AtomicBool,
}

struct State {
    ledger: Ledger,
    /// Grants decided but not yet collected by the thread that asked.
    ready: BTreeMap<u64, Grant>,
    /// Grants asked to stop and not yet released.
    stop: BTreeSet<u64>,
    /// The cores file of each grant that has one, until it is released.
    files: BTreeMap<u64, PathBuf>,
}

impl Scheduler {
    pub fn new(
        limits: Limits,
        clock: Arc<dyn Clock>,
        host: Arc<dyn HostCpu + Send + Sync>,
    ) -> Self {
        Self {
            state: Mutex::new(State {
                ledger: Ledger::new(limits),
                ready: BTreeMap::new(),
                stop: BTreeSet::new(),
                files: BTreeMap::new(),
            }),
            changed: Condvar::new(),
            clock,
            host,
            sampling: AtomicBool::new(false),
        }
    }

    /// Blocks until the request can be granted.
    pub fn acquire(&self, request: &GrantRequest) -> Grant {
        self.acquire_until(request, &|| false)
            .expect("a request that is never cancelled is granted")
    }

    /// Blocks until the request can be granted, or until `cancelled` returns true, which is
    /// asked about once every [`CANCEL_RECHECK`]; a cancelled request is withdrawn and gets
    /// `None`. While it waits, the calling thread's step (see [`crate::steps`]) says what for.
    pub fn acquire_until(
        &self,
        request: &GrantRequest,
        cancelled: &dyn Fn() -> bool,
    ) -> Option<Grant> {
        self.acquire_since(request, self.clock.now(), cancelled)
    }

    /// [`Self::acquire_until`] for a request first asked for at `since`, which places it in the
    /// queue (see [`Ledger::enqueue`]) and counts toward its queued time. A verb process that
    /// asks again for a command after the daemon restarted passes the moment it first asked, so
    /// it keeps its place.
    pub fn acquire_since(
        &self,
        request: &GrantRequest,
        since: ignite_core::time::Timestamp,
        cancelled: &dyn Fn() -> bool,
    ) -> Option<Grant> {
        let mut state = self.lock();
        let id = state.ledger.enqueue(request.clone(), since);
        self.advance(&mut state);
        let mut waiting = None;
        loop {
            if let Some(grant) = state.ready.remove(&id) {
                return Some(grant);
            }
            if cancelled() {
                state.ledger.release(id);
                self.advance(&mut state);
                return None;
            }
            if waiting.is_none() {
                waiting = Some(crate::steps::waiting(waiting_for(
                    request,
                    &state.ledger.status(),
                )));
            }
            state = self
                .changed
                .wait_timeout(state, CANCEL_RECHECK)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Frees a grant's cores and resources, and starts whatever can start now.
    pub fn release(&self, grant: &Grant) {
        let mut state = self.lock();
        state.ledger.release(grant.id);
        state.stop.remove(&grant.id);
        state.files.remove(&grant.id);
        self.advance(&mut state);
    }

    /// Reads the machine's CPU use and free memory, and starts or stops what that changes. A
    /// failed reading keeps the previous one.
    pub fn sample(&self) {
        let busy = self.host.busy().ok();
        let memory = self.host.free_memory_mb().ok();
        let mut state = self.lock();
        state.ledger.observe(busy, memory);
        self.advance(&mut state);
    }

    /// Starts a thread that calls [`Self::sample`] every [`SAMPLE_EVERY`] for as long as the
    /// scheduler exists. A scheduler has one such thread: a second call does nothing.
    pub fn sample_every(scheduler: &Arc<Self>) {
        if scheduler.sampling.swap(true, Ordering::SeqCst) {
            return;
        }
        let weak: Weak<Self> = Arc::downgrade(scheduler);
        thread::spawn(move || {
            loop {
                thread::sleep(SAMPLE_EVERY);
                match weak.upgrade() {
                    Some(scheduler) => scheduler.sample(),
                    None => break,
                }
            }
        });
    }

    /// Blocks until grant `id` is asked to stop (true) or is released (false). The supervisor of
    /// an `idle` command waits on this and ends the command when it returns true.
    pub fn wait_stop(&self, id: u64) -> bool {
        let mut state = self.lock();
        loop {
            if state.stop.contains(&id) {
                return true;
            }
            if !state.ledger.holds(id) && !state.ready.contains_key(&id) {
                return false;
            }
            state = self.wait(state);
        }
    }

    /// Blocks until grant `id` is released (true) or `grace_s` seconds of the scheduler's clock
    /// have passed since the call (false). The supervisor of a stopped `idle` command waits on
    /// this after asking its command to exit, and kills it when the grace runs out.
    pub fn wait_released(&self, id: u64, grace_s: f64) -> bool {
        let start = self.clock.now();
        let mut state = self.lock();
        loop {
            if !state.ledger.holds(id) {
                return true;
            }
            if self.clock.now().seconds_since(start) >= grace_s {
                return false;
            }
            state = self
                .changed
                .wait_timeout(state, RECHECK)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Records a grant a previous daemon made to a command that still runs (see
    /// [`Ledger::adopt`]). Release it with [`Self::release`] when the command ends.
    pub fn adopt(
        &self,
        class: Class,
        cores: u32,
        resources: Vec<String>,
        memory_mb: Option<u64>,
        follow: Option<Follow>,
    ) -> Grant {
        let mut state = self.lock();
        let grows_to = follow
            .as_ref()
            .map(|f| f.asked.clamp(1, state.ledger.limits().max_per_command()));
        let shrinks = follow.as_ref().is_some_and(|f| f.shrinks);
        let id = state.ledger.adopt(
            class,
            cores,
            resources.clone(),
            memory_mb,
            grows_to,
            shrinks,
        );
        let cores_file = follow.map(|f| f.cores_file);
        if let Some(file) = &cores_file {
            state.files.insert(id, file.clone());
        }
        self.advance(&mut state);
        Grant {
            id,
            cores: cores.max(1),
            resources,
            queued_s: 0,
            cores_file,
        }
    }

    /// While draining, no `idle` work starts and what runs is told to stop, so that an update
    /// waiting for the daemon's commands does not wait for it.
    pub fn set_draining(&self, draining: bool) {
        let mut state = self.lock();
        state.ledger.set_draining(draining);
        self.advance(&mut state);
    }

    pub fn status(&self) -> Status {
        self.lock().ledger.status()
    }

    /// What `request` would wait for now, such as `8 cores for land work (2 land commands
    /// running, 3 waiting)`.
    pub fn describe(&self, request: &GrantRequest) -> String {
        waiting_for(request, &self.status())
    }

    fn advance(&self, state: &mut State) {
        let step = state.ledger.step(self.clock.now());
        if step.granted.is_empty() && step.stop.is_empty() && step.resized.is_empty() {
            return;
        }
        for grant in step.granted {
            if let Some(file) = &grant.cores_file {
                write_cores(file, grant.cores);
                state.files.insert(grant.id, file.clone());
            }
            state.ready.insert(grant.id, grant);
        }
        for (id, cores) in step.resized {
            if let Some(grant) = state.ready.get_mut(&id) {
                grant.cores = cores;
            }
            if let Some(file) = state.files.get(&id) {
                write_cores(file, cores);
            }
        }
        state.stop.extend(step.stop);
        self.changed.notify_all();
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn wait<'a>(&self, guard: MutexGuard<'a, State>) -> MutexGuard<'a, State> {
        self.changed
            .wait(guard)
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// What a waiting request waits for, such as `8 cores for tip work (2 tip commands running, 3
/// waiting)`, for the step of the work that asked.
fn waiting_for(request: &GrantRequest, status: &Status) -> String {
    let class = request.class;
    let mut text = format!("{} cores for {} work", request.cores.max(1), class.as_str());
    if let Some(c) = status.classes.iter().find(|c| c.class == class) {
        let _ = write!(
            text,
            " ({} {} commands running, {} waiting)",
            c.running,
            class.as_str(),
            c.waiting
        );
    }
    if !request.resources.is_empty() {
        let _ = write!(text, " and {}", request.resources.join(", "));
    }
    text
}

/// Writes a grant's size to its cores file by rename, so the command never reads a partial
/// number. A failed write leaves the command on its previous size, which is reported.
fn write_cores(path: &Path, cores: u32) {
    let partial = path.with_extension("partial");
    let written: io::Result<()> =
        fs::write(&partial, format!("{cores}\n")).and_then(|()| fs::rename(&partial, path));
    if let Err(error) = written {
        eprintln!(
            "ignite: cannot write the grant size to {}: {error}",
            path.display()
        );
    }
}
