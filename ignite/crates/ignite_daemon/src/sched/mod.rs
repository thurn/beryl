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
//! - Within `agent`, commands for regressions and master-lag justiciar tickets are served
//!   before other agents. The priority changes only the order of queued requests.
//! - One command takes at most two thirds of the cores. Among requests of equal priority,
//!   those that waited [`AGED_SECONDS`] go first, then the smallest. Each gets what it asked
//!   for before the next gets anything (see `ledger/order.rs`).
//! - `tip` and `batch` have derived floors that higher classes cannot take while the class
//!   has work running or waiting.
//! - Each named resource is held by at most its capacity of grants at once. A resource the
//!   machine does not declare has capacity 1.
//! - CPU busy beyond what Ignite uses belongs to processes outside Ignite and is not granted.
//!   Ignite's use is everything the daemon's process tree uses, express runs and verb processes
//!   included, not only its grants' commands (see `Ledger::observe`).
//! - A grant whose command has used well below its cores for [`LEND_WINDOW_SECONDS`] lends the
//!   idle ones to waiting requests, and counts them again within [`RECENT_SECONDS`] of its use
//!   rising; `idle` work is stopped first when the machine is then over.
//! - While swap grows fast or free memory runs out, nothing new starts and no grant grows.
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
//! - A machine's `tip_build_cores` are kept for requests with `Reserve::TipBuild`, the build
//!   checks of a tip round. Agents borrow idle tip and express cores until an owner asks.
//!   Owners still start on their reserve immediately. Tip builds take shared cores before
//!   every class, including `land`, and never shrink for another request.
//! - Classes are enforced by grants only. Nothing here changes a process's priority: work run
//!   at low priority on a loaded machine once got almost no CPU for hours.

mod express;
mod ledger;
mod measured;
pub mod roots;

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

use ignite_core::capacity::{Derived, Floor, MachineFacts, resource_slots};
use ignite_core::config::{Config, Machine};
use ignite_core::ids::TicketId;
use ignite_core::manifest::{Manifest, Platforms};
use ignite_core::protocol::Class;
use ignite_core::traits::Clock;
use ignite_os::hostcpu::HostCpu;
use serde::Serialize;

pub use ledger::{AGED_SECONDS, Ledger, RECLAIM_SECONDS, SMALL_CORES, Step};
pub use measured::{LEND_WINDOW_SECONDS, RECENT_SECONDS};

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
    /// Cores set aside for this kind of command (see [`ignite_core::traits::Reserve`]). A
    /// tip build starts on up to [`Limits::tip_build`] cores, then takes shared cores.
    pub reserve: Option<ignite_core::traits::Reserve>,
    /// The ticket that holds master back, giving this request priority within the agent queue.
    pub priority_ticket: Option<TicketId>,
}

/// How an adopted grant keeps following its command after a daemon restart: the cores file its
/// jobserver reads, the cores it asked for, and whether it gives cores back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Follow {
    pub cores_file: PathBuf,
    pub asked: u32,
    pub shrinks: bool,
}

/// A grant a previous daemon made, as [`Scheduler::adopt_all`] records it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Adoption {
    pub class: Class,
    /// The grant's size: its cores file's number, or its first size without one.
    pub cores: u32,
    pub resources: Vec<String>,
    pub memory_mb: Option<u64>,
    pub follow: Option<Follow>,
    /// The reserve the grant was made from, if any. An adopted `Reserve::TipBuild` grant holds
    /// the tip reserve's cores again until it is released.
    pub reserve: Option<ignite_core::traits::Reserve>,
    /// The ticket whose agent priority the original grant used.
    pub priority_ticket: Option<TicketId>,
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
    /// The ticket whose priority the grant used.
    pub priority_ticket: Option<TicketId>,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Limits {
    /// The shared cores available to all request classes.
    pub cores: u32,
    /// Express and tip reserve cores, counted in [`Limits::machine_cores`] for CPU load.
    pub reserved: u32,
    /// The reserved cores for tip-round build checks.
    pub tip_build: u32,
    pub floor: Floor,
    /// Each resource's capacity.
    pub resources: BTreeMap<String, u32>,
}

impl Limits {
    /// Derives the shared pool and both reserves. At least one core stays shared. On small
    /// machines the express lane gets its reserve before tip builds do.
    pub fn for_machine(
        config: &Config,
        machine: &Machine,
        manifest: &Manifest,
        facts: &MachineFacts,
        derived: &Derived,
    ) -> Self {
        let cores = facts.logical_cores;
        let handed_out = cores
            .saturating_sub(derived.express_cores)
            .saturating_sub(derived.tip_build_cores)
            .max(1);
        let tip_build = derived.tip_build_cores.min(
            cores
                .saturating_sub(derived.express_cores)
                .saturating_sub(handed_out),
        );
        let mut resources = machine.resources.clone();
        for (name, resource) in &config.resources {
            resources.remove(name);
            if manifest.families.iter().any(|(family_name, declared)| {
                let family = &declared.family;
                family.resources.contains(name)
                    && machine
                        .families
                        .as_ref()
                        .is_none_or(|list| list.contains(family_name))
                    && match &family.platforms {
                        Platforms::Any(_) => true,
                        Platforms::Each(platforms) => platforms.contains(&facts.os),
                    }
            }) {
                resources.insert(name.clone(), resource_slots(cores, resource.per_core));
            }
        }
        Self {
            cores: handed_out,
            reserved: cores.saturating_sub(handed_out),
            tip_build,
            floor: derived.floor,
            resources,
        }
    }

    /// Every logical core of the machine, the express lane's and the tip reserve's included.
    pub fn machine_cores(&self) -> u32 {
        self.cores + self.reserved
    }

    fn express(&self) -> u32 {
        self.reserved - self.tip_build
    }

    /// The most cores one command may hold: two thirds of the machine, and at least one.
    pub fn max_per_command(&self) -> u32 {
        (self.cores * 2 / 3).max(1)
    }

    /// A resource's capacity, defaulting to one when undeclared.
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
    /// Of those, the cores counted as in use when deciding what is free: grants whose commands
    /// use fewer than they hold lend the rest to waiting requests.
    pub committed: u32,
    /// Cores Ignite's commands used at the last reading, of the grants whose use is read.
    pub used: u32,
    pub swap_used_mb: Option<u64>,
    /// Swap grew fast or free memory ran out, so nothing new is admitted.
    pub memory_pressed: bool,
    pub max_per_command: u32,
    pub free_memory_mb: Option<u64>,
    /// Cores kept for the build checks of a tip round ([`Limits::tip_build`]), outside
    /// [`Status::cores`].
    pub tip_build: u32,
    /// Of those, the cores that grants with `Reserve::TipBuild` hold.
    pub tip_build_used: u32,
    /// How many cores agents have borrowed from the reserves.
    pub agent_borrowed: u32,
    /// Running commands granted with agent queue priority, by ticket.
    pub priority_grants: BTreeMap<TicketId, u32>,
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

pub struct Scheduler {
    state: Mutex<State>,
    changed: Condvar,
    clock: Arc<dyn Clock>,
    host: Arc<dyn HostCpu + Send + Sync>,
    /// A sampler thread runs. One host reading serves every sample, and two samplers would each
    /// see the counters microseconds after the other's reading.
    sampling: AtomicBool,
    /// Lists the PIDs of Ignite's processes outside the daemon's tree, whose CPU counts as
    /// Ignite's ([`Self::count_as_ignite`]).
    roots: Mutex<Option<Arc<IgniteRoots>>>,
}

/// Lists the PIDs of processes whose trees count as Ignite's beside the daemon's own: the
/// verb processes and, on the hub's machine, the hub. See [`Scheduler::count_as_ignite`].
pub type IgniteRoots = dyn Fn() -> Vec<u32> + Send + Sync;

struct State {
    ledger: Ledger,
    /// When the sampler last obtained a CPU reading for dispatch.
    cpu_sample_at: Option<ignite_core::time::Timestamp>,
    /// Grants decided but not yet collected by the thread that asked.
    ready: BTreeMap<u64, Grant>,
    /// Grants asked to stop and not yet released.
    stop: BTreeSet<u64>,
    /// The cores file of each grant that has one, until it is released.
    files: BTreeMap<u64, PathBuf>,
    /// The file holding the process group ID of each grant's command, whose CPU use each sample
    /// reads ([`Scheduler::watch`]), until the grant is released.
    groups: BTreeMap<u64, PathBuf>,
    /// The grants exceeded the machine's cores at the last step, so the log line that says so
    /// is written once each time they go over.
    over: bool,
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
                cpu_sample_at: None,
                ready: BTreeMap::new(),
                stop: BTreeSet::new(),
                files: BTreeMap::new(),
                groups: BTreeMap::new(),
                over: false,
            }),
            changed: Condvar::new(),
            clock,
            host,
            sampling: AtomicBool::new(false),
            roots: Mutex::new(None),
        }
    }

    /// Counts the CPU of the processes `roots` lists, and of every process they started, as
    /// Ignite's from the next sample on, beside the daemon's own tree. Each sample calls it
    /// once; a PID that no longer runs counts as nothing.
    pub fn count_as_ignite(&self, roots: Arc<IgniteRoots>) {
        *self.roots.lock().unwrap_or_else(PoisonError::into_inner) = Some(roots);
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
                waiting = Some(crate::steps::waiting_for_cores(waiting_for(
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

    /// Asks for `request` and returns what is free now without waiting: `None` when nothing
    /// could be granted at once, and the request is withdrawn. The grant may be smaller than
    /// asked. Release it with [`Self::release`]; like any grant it may be shrunk or stopped for
    /// a higher class.
    pub fn try_acquire(&self, request: &GrantRequest) -> Option<Grant> {
        let mut state = self.lock();
        let id = state.ledger.enqueue(request.clone(), self.clock.now());
        self.advance(&mut state);
        let grant = state.ready.remove(&id);
        if grant.is_none() {
            state.ledger.release(id);
            self.advance(&mut state);
        }
        grant
    }

    /// Frees a grant's cores and resources, and starts whatever can start now.
    pub fn release(&self, grant: &Grant) {
        let mut state = self.lock();
        state.ledger.release(grant.id);
        state.stop.remove(&grant.id);
        state.files.remove(&grant.id);
        state.groups.remove(&grant.id);
        self.advance(&mut state);
    }

    /// [`Self::release`] for a grant known only by its ID, such as one lent to a verb process.
    pub fn release_id(&self, id: u64) {
        let mut state = self.lock();
        state.ledger.release(id);
        state.stop.remove(&id);
        state.files.remove(&id);
        state.groups.remove(&id);
        state.ready.remove(&id);
        self.advance(&mut state);
    }

    /// Measures grant `id`'s command from now on: each sample reads the process group ID in
    /// `group_file` (written once the command starts) and the CPU time of that group and every
    /// process it started, so a grant that leaves cores idle lends them.
    pub fn watch(&self, id: u64, group_file: PathBuf) {
        let mut state = self.lock();
        if state.ledger.holds(id) || state.ready.contains_key(&id) {
            state.groups.insert(id, group_file);
        }
    }

    /// Reads the CPU each watched grant's command has used, the machine's CPU use, free memory
    /// and swap, and starts or stops what that changes. A failed reading keeps the previous one;
    /// a grant whose use cannot be read keeps all its cores counted.
    pub fn sample(&self) {
        let groups: Vec<(u64, PathBuf)> = self
            .lock()
            .groups
            .iter()
            .map(|(&id, file)| (id, file.clone()))
            .collect();
        let used: Vec<(u64, f64)> = groups
            .into_iter()
            .filter_map(|(id, file)| {
                let group = fs::read_to_string(file).ok()?.trim().parse().ok()?;
                Some((id, self.host.tree_cpu_seconds(group).ok()?))
            })
            .collect();
        // The daemon's whole tree (grants' commands, verb processes, express runs and the
        // daemon) first, so a listed process inside it counts under the daemon only.
        let daemon = std::process::id();
        let listed = self
            .roots
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let mut roots = vec![daemon];
        for root in listed.map(|list| list()).unwrap_or_default() {
            if !roots.contains(&root) {
                roots.push(root);
            }
        }
        let trees = self.host.process_trees_cpu_seconds(&roots).ok();
        let busy = self.host.busy().ok();
        let memory = self.host.free_memory_mb().ok();
        let swap = self.host.swap_used_mb().ok();
        let now = self.clock.now();
        let mut state = self.lock();
        for (id, cpu_s) in used {
            state.ledger.observe_use(id, now, cpu_s);
        }
        if let Some(swap) = swap {
            state.ledger.observe_swap(now, swap);
        }
        state.ledger.observe_trees(now, trees.as_deref());
        if busy.is_some() {
            state.cpu_sample_at = Some(now);
        }
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
        let mut grants = self.adopt_all(vec![Adoption {
            class,
            cores,
            resources,
            memory_mb,
            follow,
            reserve: None,
            priority_ticket: None,
        }]);
        grants.pop().expect("one grant for one adoption")
    }

    /// Records every grant a previous daemon made to a command that still runs, in one step,
    /// and returns them in the same order.
    ///
    /// Adopting them one at a time, with a scheduling step after each, granted more cores than
    /// the machine has: the step after the first adoptions grew those grants into cores that
    /// looked free but that commands not yet adopted held, and adopting those commands then
    /// pushed the total past the cores (20 of 18 after a restart on 2026-11-18, with an agent
    /// grant adopted at one core grown to nine before a ten-core one was adopted).
    pub fn adopt_all(&self, adoptions: Vec<Adoption>) -> Vec<Grant> {
        let mut state = self.lock();
        let max = state.ledger.limits().max_per_command();
        let mut grants = Vec::with_capacity(adoptions.len());
        for adoption in adoptions {
            let grows_to = adoption.follow.as_ref().map(|f| f.asked.clamp(1, max));
            let shrinks = adoption.follow.as_ref().is_some_and(|f| f.shrinks);
            let id = state.ledger.adopt(
                adoption.class,
                adoption.cores,
                adoption.resources.clone(),
                adoption.memory_mb,
                grows_to,
                shrinks,
            );
            if adoption.reserve == Some(ignite_core::traits::Reserve::TipBuild) {
                state.ledger.adopt_reserved(id);
            }
            state
                .ledger
                .record_priority(id, adoption.priority_ticket.clone());
            let cores_file = adoption.follow.map(|f| f.cores_file);
            if let Some(file) = &cores_file {
                state.files.insert(id, file.clone());
            }
            grants.push(Grant {
                id,
                cores: adoption.cores.max(1),
                resources: adoption.resources,
                queued_s: 0,
                cores_file,
                priority_ticket: adoption.priority_ticket,
            });
        }
        self.advance(&mut state);
        grants
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

    pub(crate) fn shared_cores(&self) -> u32 {
        self.lock().ledger.limits().cores
    }

    /// What `request` would wait for now, such as `8 cores for land work (2 land commands
    /// running, 3 waiting)`.
    pub fn describe(&self, request: &GrantRequest) -> String {
        waiting_for(request, &self.status())
    }

    fn advance(&self, state: &mut State) {
        let step = state.ledger.step(self.clock.now());
        log_overcommit(state);
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

/// Writes one log line, each time the cores the grants count as using go past the machine's
/// cores, naming every grant held. After a restart on 2026-11-18 the daemon showed 25 of 18
/// cores granted, and nothing recorded which grants made up the 25: ones adopted from the
/// previous daemon or ones this daemon made. The line says, for the next time it happens. The
/// grants alone may exceed the cores without a line, because a grant that leaves cores idle
/// lends them.
fn log_overcommit(state: &mut State) {
    let status = state.ledger.status();
    let over = status.committed > status.cores;
    if over && !state.over {
        let holders: Vec<String> = state
            .ledger
            .holders()
            .iter()
            .map(|(class, cores, adopted)| {
                let from = if *adopted { " adopted" } else { "" };
                format!("{} {cores}{from}", class.as_str())
            })
            .collect();
        eprintln!(
            "ignite: {} of {} cores counted in use ({} granted), past the machine's cores: {}",
            status.committed,
            status.cores,
            status.granted,
            holders.join(", ")
        );
    }
    state.over = over;
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
