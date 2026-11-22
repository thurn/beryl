//! The daemon's side of the host protocol (`ignite_core::protocol::host`): the calls a verb
//! process makes to the daemon of its machine.
//!
//! A long verb, such as `land`, runs in a process of its own (a verb process) that outlives
//! restarts of the daemon. It asks the daemon for what needs one authority on the machine, over
//! the same socket clients use, one connection per call. Each call is a verb of the daemon whose
//! name starts with `host-`, answered on the connection's thread ([`Host::register`]):
//!
//! - `host-hello` registers the verb process. The daemon keeps an [`Activity`] for its handle
//!   in the daemon's steps registry (see [`crate::steps`]), as it does for a verb running on one
//!   of its own threads, so `ignite wait`, `status`, the client's progress lines, `ignite stop`
//!   and the release of a tree see the verb the same way. Any other call of a handle the daemon
//!   has no activity for yet creates it as well, so the order of calls after a restart does not
//!   matter.
//! - `host-run` waits for a grant of cores, starts the command under a supervisor and answers
//!   with the run directory (`host/runs.rs`). The command's environment, and so its run's
//!   `spec.json`, names the call's request ID (`IGNITE_REQUEST`), the handle (`IGNITE_HANDLE`)
//!   and the commit its checkout was at (`IGNITE_TREE_COMMIT`). A call repeated with the same
//!   request ID, by the same daemon or by one that restarted since, finds that run and starts
//!   nothing. A call that waits answers `waiting` after at most `MAX_BLOCK_S` seconds while the
//!   request stays queued, and a request first sent to an earlier daemon is queued at the moment
//!   it was first asked for (`first_asked_at`), so it keeps its place.
//! - `host-build` makes a product current in a tree with the daemon's build manager, joining a
//!   build already running for it, under the same 30-second rule (`host/builds.rs`).
//! - `host-selected` answers which packages the build of a product selects for the packages a
//!   verb process wants, since only the build manager compares their features with the whole
//!   workspace's (`host/builds.rs`).
//! - `host-lease-landing` and `host-release-landing` lease and free a landing tree, owned by the
//!   handle, so a restarted daemon keeps it (`host/landing.rs`).
//! - `host-borrow-cores` and `host-return-cores` lend a verb process the scheduler cores that are
//!   free now, for a command it runs itself (`host/cores.rs`). A grant the process never returns
//!   is released when its handle ends.
//! - `host-step`, `host-refuse-stop`, `host-allow-stop` and `host-poll` keep the activity's step,
//!   waits and stop state (`host/activity.rs`). Whether a stop was asked, and the reason stopping
//!   is refused while it is, are also kept in the local database (`handle_stops`), so a
//!   restarted daemon gives the same answers.
//! - `host-finished` says the verb process wrote its reply into the handle's row: the daemon
//!   wakes whoever waits on the handle and ends the activity.
//!
//! `ignite stop` of a verb running in a process of its own asks its activity to stop, as for
//! any verb, and is refused with the same reason while the verb refuses stopping. Once a stop
//! is asked, the daemon ends the handle's commands itself (every unfinished run whose request ID
//! belongs to the handle) with `SIGTERM`, and with `SIGKILL` ten seconds later; a `host-run`
//! still waiting for cores is withdrawn, and a new one is refused. The verb process sees the stop
//! on its next `host-poll`, unwinds, writes its reply and sends `host-finished`. A verb process
//! that ends without `host-finished` is noticed within a second: its handle's row is no longer
//! `running`.

mod activity;
mod builds;
mod cores;
mod landing;
mod runs;
#[cfg(test)]
mod tests;
mod verbs;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use rusqlite::Connection;

use ignite_core::ids::{Commit, Handle, TreeId};
use ignite_core::protocol::host::{BuildReply, Hello, MAX_BLOCK_S, RequestId, RunReply};
use ignite_core::time::Timestamp;
use ignite_core::traits::{Builds, Clock, CommandSpec, Result};

use crate::handles::Handles;
use crate::server::Registry;
use crate::steps::{Activity, Steps};
use crate::trees::TreeLease;

pub use landing::TreesLanding;
pub use runs::SupervisedStarter;

/// How the daemon starts the commands of verb processes: the supervised launcher, or a fake in
/// tests.
pub trait RunStarter: Send + Sync {
    /// Waits for the command's grant, queued as first asked for at `since`, starts it and
    /// returns its run directory once its supervisor runs; `None` when `cancelled` turned true
    /// first. The grant is held until the command finishes.
    fn begin(
        &self,
        spec: &CommandSpec,
        since: Timestamp,
        cancelled: &dyn Fn() -> bool,
    ) -> std::io::Result<Option<PathBuf>>;
    /// The run directory a `host-run` call with this request ID started, by this daemon or an
    /// earlier one, finished or not.
    fn find(&self, request: &RequestId) -> Option<PathBuf>;
    /// The run directories of `handle`'s commands that have not finished.
    fn runs_of(&self, handle: &Handle) -> Vec<PathBuf>;
    /// Ends a run's command and everything it started, with `SIGTERM`.
    fn end(&self, run: &Path);
    /// Kills a run's command and everything it started, with `SIGKILL`.
    fn kill(&self, run: &Path);
    /// What a command waiting for its grant waits for, such as `8 cores for land work`.
    fn waiting_for(&self, spec: &CommandSpec) -> String;
    /// Grants up to `cores` of what is free now without waiting, for a command the verb process
    /// runs itself: the grant's ID and size, or `None` when nothing is free or the starter has
    /// no scheduler.
    fn borrow_cores(&self, _cores: u32) -> Option<(u64, u32)> {
        None
    }
    /// Gives back a grant from [`Self::borrow_cores`].
    fn return_cores(&self, _grant: u64) {}
}

/// How the daemon leases and frees landing trees for verb processes: the machine's trees, or a
/// fake in tests.
pub trait LandingTrees: Send + Sync {
    /// Leases a landing tree at `at` to `owner` under `holder`, while more than `keep_free`
    /// others stay free. The tree `owner` already holds is returned, untouched when it is at
    /// `at`. `None` when none is free.
    fn lease(
        &self,
        holder: &str,
        owner: &Handle,
        at: &Commit,
        keep_free: usize,
    ) -> Result<Option<TreeLease>>;
    /// Frees `tree` if `owner` still holds it; a tree already freed is left alone.
    fn release(&self, owner: &Handle, tree: &TreeId, built: Option<&Commit>) -> Result<()>;
}

/// What the host is built from.
pub struct HostParts {
    pub headless_dispatch: Option<Arc<crate::dispatchd::HeadlessDispatch>>,
    pub starter: Arc<dyn RunStarter>,
    pub builds: Arc<dyn Builds>,
    pub landing: Arc<dyn LandingTrees>,
    /// The daemon's local database: handles and their stop state.
    pub db: Arc<Mutex<Connection>>,
    /// The registry the server lists its verbs in, where verb processes are listed too.
    pub steps: Arc<Steps>,
    pub clock: Arc<dyn Clock>,
}

/// How long calls wait; shorter in tests.
#[derive(Clone, Copy, Debug)]
pub struct Timing {
    /// The longest a `host-run` or `host-build` call holds its connection.
    pub block: Duration,
    /// A request waiting for cores is withdrawn when its verb process has not asked about it
    /// for this long, as when the process died.
    pub abandon: Duration,
    /// How often the daemon looks whether a verb process's handle has ended.
    pub watch: Duration,
    /// How long a command has after `SIGTERM` before `SIGKILL`.
    pub grace: Duration,
}

impl Default for Timing {
    fn default() -> Timing {
        let block = Duration::from_secs(u64::from(MAX_BLOCK_S));
        Timing {
            block,
            abandon: block * 4,
            watch: Duration::from_secs(1),
            grace: Duration::from_secs(10),
        }
    }
}

/// The daemon's side of the host protocol. See the module documentation.
pub struct Host {
    headless_dispatch: Option<Arc<crate::dispatchd::HeadlessDispatch>>,
    starter: Arc<dyn RunStarter>,
    builds: Arc<dyn Builds>,
    landing: Arc<dyn LandingTrees>,
    db: Arc<Mutex<Connection>>,
    steps: Arc<Steps>,
    clock: Arc<dyn Clock>,
    timing: Timing,
    /// The server's handles, set once the server exists, for waking waiters.
    handles: OnceLock<Arc<Handles>>,
    /// The activity of each verb process the daemon knows is running.
    remotes: Mutex<HashMap<Handle, Remote>>,
    /// Handles whose verb process has ended and whose verb the daemon now runs itself (see
    /// [`Host::hand_back`]). The host keeps no activity for them and ends none of their
    /// commands.
    handed_back: Mutex<HashSet<Handle>>,
    runs: Mutex<HashMap<RequestId, Arc<Pending<RunReply>>>>,
    built: Mutex<HashMap<RequestId, Arc<Pending<BuildReply>>>>,
    /// The cores granted to each verb process by `host-borrow-cores` and not yet returned.
    borrowed: Mutex<HashMap<Handle, Vec<u64>>>,
}

/// One verb process.
struct Remote {
    activity: Arc<Activity>,
    /// Its last `host-hello`, if this daemon has had one.
    hello: Option<Hello>,
}

/// A `host-run` or `host-build` whose answer a thread of the daemon is working out.
struct Pending<T> {
    state: Mutex<PendingState<T>>,
    changed: std::sync::Condvar,
}

struct PendingState<T> {
    outcome: Option<T>,
    /// When the verb process last asked about it.
    asked: std::time::Instant,
    /// When the outcome was set.
    settled: Option<std::time::Instant>,
}

/// How long a settled `host-run` or `host-build` answer is kept for a repeated call.
const KEEP_SETTLED: Duration = Duration::from_hours(1);

impl Host {
    pub fn new(parts: HostParts) -> Arc<Host> {
        Self::with_timing(parts, Timing::default())
    }

    pub fn with_timing(parts: HostParts, timing: Timing) -> Arc<Host> {
        Arc::new(Host {
            headless_dispatch: parts.headless_dispatch,
            starter: parts.starter,
            builds: parts.builds,
            landing: parts.landing,
            db: parts.db,
            steps: parts.steps,
            clock: parts.clock,
            timing,
            handles: OnceLock::new(),
            remotes: Mutex::new(HashMap::new()),
            handed_back: Mutex::new(HashSet::new()),
            runs: Mutex::new(HashMap::new()),
            built: Mutex::new(HashMap::new()),
            borrowed: Mutex::new(HashMap::new()),
        })
    }

    /// Gives the host the server's handles, so `host-finished` wakes whoever waits on one.
    pub fn set_handles(&self, handles: Arc<Handles>) {
        let _ = self.handles.set(handles);
    }

    /// Registers a verb for every call of the host protocol.
    pub fn register(self: &Arc<Self>, registry: &mut Registry) {
        verbs::register(self, registry);
    }

    /// Lists `handle`'s verb process in the steps registry now, without waiting for its first
    /// call, as a restarted daemon does for each verb process it finds alive, so that `ignite
    /// stop` and `ignite status` see it at once. False when the handle is not running.
    pub fn track(self: &Arc<Self>, ctx: &crate::server::Context, handle: &Handle) -> bool {
        self.remote(ctx, handle).is_some()
    }

    /// Ends, with `SIGTERM`, every unfinished command of `handle`'s verb process, as when the
    /// process died and the daemon runs the verb again itself.
    pub fn end_runs_of(&self, handle: &Handle) {
        for run in self.starter.runs_of(handle) {
            self.starter.end(&run);
        }
    }

    /// The verb process of `handle` has ended and the daemon runs its verb again itself, under
    /// the same handle: returns the cores the process borrowed and forgets its activity, its
    /// stop state and its watch, so that a stop asked of the process does not end the commands
    /// the new run starts.
    pub fn hand_back(&self, handle: &Handle) {
        crate::handles::lock(&self.handed_back).insert(handle.clone());
        if let Some(remote) = crate::handles::lock(&self.remotes).remove(handle) {
            remote.activity.finish();
        }
        self.return_all_cores(handle);
        if let Err(error) = ignite_store::local::handle_stops::set(
            &crate::handles::lock(&self.db),
            &ignite_store::local::rows::HandleStop {
                handle: handle.to_string(),
                asked: false,
                refused: None,
                at: self.clock.now().to_string(),
            },
        ) {
            eprintln!("ignite: cannot clear the stop state of {handle}: {error}");
        }
    }

    /// A new verb process runs `handle`'s verb again after [`Host::hand_back`]: the host serves
    /// its calls and keeps an activity for it again. The earlier process's watch ends, since its
    /// activity is no longer the handle's.
    pub fn take_on(&self, handle: &Handle) {
        crate::handles::lock(&self.handed_back).remove(handle);
    }

    fn is_handed_back(&self, handle: &Handle) -> bool {
        crate::handles::lock(&self.handed_back).contains(handle)
    }

    /// The last `host-hello` of the verb process running `handle`, such as for the executable
    /// it runs under.
    pub fn hello_of(&self, handle: &Handle) -> Option<Hello> {
        crate::handles::lock(&self.remotes)
            .get(handle)
            .and_then(|remote| remote.hello.clone())
    }
}

impl<T: Clone> Pending<T> {
    fn new(now: std::time::Instant) -> Arc<Pending<T>> {
        Arc::new(Pending {
            state: Mutex::new(PendingState {
                outcome: None,
                asked: now,
                settled: None,
            }),
            changed: std::sync::Condvar::new(),
        })
    }

    /// Waits at most `block` for the outcome, recording that the verb process asked.
    fn wait(&self, block: Duration) -> Option<T> {
        let mut state = crate::handles::lock(&self.state);
        state.asked = std::time::Instant::now();
        let (state, _) = self
            .changed
            .wait_timeout_while(state, block, |s| s.outcome.is_none())
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.outcome.clone()
    }

    fn settle(&self, outcome: T) {
        let mut state = crate::handles::lock(&self.state);
        state.outcome = Some(outcome);
        state.settled = Some(std::time::Instant::now());
        drop(state);
        self.changed.notify_all();
    }

    /// The verb process has not asked about it for `abandon`.
    fn abandoned(&self, abandon: Duration) -> bool {
        crate::handles::lock(&self.state).asked.elapsed() > abandon
    }

    /// Settled long enough ago to be forgotten.
    fn stale(&self) -> bool {
        crate::handles::lock(&self.state)
            .settled
            .is_some_and(|at| at.elapsed() > KEEP_SETTLED)
    }
}
