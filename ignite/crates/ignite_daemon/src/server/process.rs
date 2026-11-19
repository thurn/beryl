//! Long verbs run in verb processes: `land`, `test`, `check`, `run`, `rebuild`, `audit` and
//! the internal `idle-job` each run from start to finish in a process of their own, `ignite
//! internal verb <handle>` (see [`crate::verbproc`]), which outlives restarts of the daemon.
//! Every other verb runs on a thread of the daemon.
//!
//! The tip loop runs each round of tip verification the same way, as the internal verb
//! `tip-round`, which no client can ask for (see [`crate::tip::TipLoop`]). The loop starts it
//! with [`Server::start_internal`], waits on its handle, and after a restart finds it still
//! running with [`Server::running_internal`].
//!
//! For a request of such a verb, the server gives the verb its handle with the request (as for
//! any verb that does work), starts the verb process from the daemon's own executable in a
//! process group of its own (detached on Windows, as supervisors are started, see
//! [`crate::supervise`]), and waits until the process holds `verbs/<handle>/lock` before it
//! goes on. While the client waits, the server passes on the steps the verb process reports
//! through the host protocol (see [`crate::host`]) as progress lines, and answers with the reply
//! the verb process writes into the handle's row, or with the handle once the client's wait
//! ends.
//!
//! A thread of the daemon watches each verb process until its handle finishes. When the process
//! has ended and its handle is still `running` in the local database, it died (a crash, a kill,
//! a reboot): the daemon ends the commands it left and runs the verb again on a thread of its
//! own under the same handle, as a restarted daemon runs a verb it was cut off in (see
//! `server/resume.rs`). A verb that cannot be run again safely ends `incomplete`.
//!
//! Before restarting into a new build, the daemon stops taking new requests (each is answered
//! with [`restarting_reply`], which the client sends again) and waits up to [`DRAIN_WAIT`] for
//! the verbs running on its own threads. It never waits for verb processes.

use std::collections::BTreeSet;
use std::io;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use rusqlite::Connection;

use ignite_core::ids::Handle;
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::traits::Git;
use ignite_os::Native;
use ignite_os::proc::Processes as _;
use ignite_store::local::handles as handle_store;

use super::{CallerProbe, Context, Server, SharedSlot, Slot, Verb};
use crate::handles::{Handles, StepFeed, lock};
use crate::host::Host;
use crate::steps::Steps;

/// The verbs that run in verb processes, the internal ones included: `idle-job` runs one idle
/// job (see `crate::idle`) and `tip-round` one round of tip verification (see
/// [`crate::tip::TipLoop`]).
pub const PROCESS_VERBS: &[&str] = &[
    "land",
    "test",
    "check",
    "run",
    "rebuild",
    "audit",
    "idle-job",
    crate::tip::ROUND_VERB,
];

/// The verbs among [`PROCESS_VERBS`] that the daemon runs again, on a thread of its own, when
/// their verb process died: those a restarted daemon runs again (see
/// [`super::Registry::register_resumable`]). A landing tree owned by one of them is kept at
/// start (see [`crate::trees::owner_alive`]).
pub const RUN_AGAIN_AFTER_DEATH: &[&str] = &["land", "test", "check"];

/// How long a restart waits for the verbs running on the daemon's own threads.
pub const DRAIN_WAIT: Duration = Duration::from_secs(30);

/// How long the server waits for a new verb process to take its lock.
const LOCK_WAIT: Duration = Duration::from_secs(10);

/// How the daemon starts verb processes: from its own executable, or in-process fakes in tests.
pub trait VerbStarter: Send + Sync {
    /// Starts the verb process of `handle`, whose row holds the request.
    fn start(&self, handle: &Handle) -> io::Result<Box<dyn VerbChild>>;
}

/// A verb process this daemon started.
pub trait VerbChild: Send {
    /// Whether it has exited. Reaps it on the first call that finds it so.
    fn exited(&mut self) -> bool;
}

/// A verb process that is waited for even when nobody looks at it again. The watcher thread
/// stops watching once the verb's reply is delivered, which happens before the process has
/// exited, and a `Child` dropped without `wait` stays a zombie (a `<defunct>` entry that holds a
/// process table slot) until the daemon exits. Dropping this waits for the process on a thread
/// of its own, so a process that is slow to exit never blocks the watcher.
struct ReapedChild(Option<Child>);

impl ReapedChild {
    fn new(child: Child) -> ReapedChild {
        ReapedChild(Some(child))
    }
}

impl VerbChild for ReapedChild {
    fn exited(&mut self) -> bool {
        self.0
            .as_mut()
            .is_none_or(|child| !matches!(child.try_wait(), Ok(None)))
    }
}

impl Drop for ReapedChild {
    fn drop(&mut self) {
        if let Some(child) = self.0.take() {
            reap_in_background(child);
        }
    }
}

/// Waits for `child` on a new thread, so that it does not stay a zombie once it exits. Returns
/// the thread, which ends when the child has been reaped. If no thread can be started the wait
/// happens here instead, since an unreaped child is worse than a short delay.
fn reap_in_background(child: Child) -> Option<std::thread::JoinHandle<()>> {
    let child = Arc::new(Mutex::new(Some(child)));
    let handed = Arc::clone(&child);
    let spawned = std::thread::Builder::new()
        .name("ignite-reap".to_owned())
        .spawn(move || {
            if let Some(mut child) = lock(&handed).take() {
                let _ = child.wait();
            }
        });
    if let Ok(thread) = spawned {
        return Some(thread);
    }
    if let Some(mut child) = lock(&child).take() {
        let _ = child.wait();
    }
    None
}

/// Starts `<executable> internal verb <handle>` from the repository's main checkout, never from
/// a build tree (a request from a tree counts as its session's activity) or the Ignite home.
/// Its standard error goes to `verbs/<handle>/verb.log` in the Ignite home.
pub struct NativeStarter {
    /// The daemon's own executable, `bin/<commit>/ignite`.
    pub executable: PathBuf,
    /// The Ignite home.
    pub home: PathBuf,
    /// The main checkout.
    pub repo: PathBuf,
    /// The machine's name in `ignite.toml`, as the daemon read it, which the verb process
    /// uses too (`IGNITE_HOSTNAME`, see [`crate::verbproc::HOSTNAME_VARIABLE`]).
    pub hostname: String,
}

impl VerbStarter for NativeStarter {
    fn start(&self, handle: &Handle) -> io::Result<Box<dyn VerbChild>> {
        let folder = crate::verbproc::folder(&self.home, handle);
        std::fs::create_dir_all(&folder)?;
        let log = std::fs::File::create(folder.join("verb.log"))?;
        let mut command = Command::new(&self.executable);
        Native.detach(&mut command);
        let child = command
            .args(["internal", "verb", handle.as_str()])
            .current_dir(&self.repo)
            .env("IGNITE_HOME", &self.home)
            .env(crate::verbproc::HOSTNAME_VARIABLE, &self.hostname)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()?;
        Ok(Box::new(ReapedChild::new(child)))
    }
}

/// What the server needs to run verbs in verb processes.
pub struct Processes {
    pub host: Arc<Host>,
    pub starter: Arc<dyn VerbStarter>,
    /// The Ignite home, which holds each verb process's lock.
    pub home: PathBuf,
    /// The verbs that run in verb processes, [`PROCESS_VERBS`] in the daemon.
    pub verbs: BTreeSet<String>,
    /// How often a verb process is looked at.
    pub watch: Duration,
    /// The Ignite commit of the executable new verb processes start from, the daemon's own;
    /// `None` when unknown. A landing whose verb process runs another commit with other
    /// landing code is run again under this one (see `server/outdated.rs`).
    pub version: Option<String>,
    /// Compares the landing code of a landing's build with the daemon's, so that a landing
    /// started under an older build is run again only when the release changed what it runs
    /// (see `server/outdated.rs`). `None` runs every such landing again.
    pub git: Option<Arc<dyn Git>>,
}

impl Processes {
    pub fn new(host: Arc<Host>, starter: Arc<dyn VerbStarter>, home: PathBuf) -> Processes {
        Processes {
            host,
            starter,
            home,
            verbs: PROCESS_VERBS.iter().map(|v| (*v).to_owned()).collect(),
            watch: Duration::from_secs(1),
            version: None,
            git: None,
        }
    }

    /// The processes with `git` to compare a landing's build with the daemon's.
    #[must_use]
    pub fn with_git(self, git: Arc<dyn Git>) -> Processes {
        Processes {
            git: Some(git),
            ..self
        }
    }

    /// The processes with `version` as the commit new verb processes start from.
    #[must_use]
    pub fn with_version(self, version: &str) -> Processes {
        Processes {
            version: Some(version.to_owned()),
            ..self
        }
    }
}

/// What a thread that watches a verb process, or runs its verb again, needs of the server.
#[derive(Clone)]
pub(super) struct Shared {
    pub(super) ctx: Context,
    pub(super) handles: Arc<Handles>,
    pub(super) running: Arc<AtomicUsize>,
    pub(super) steps: Arc<Steps>,
    pub(super) db: Option<Arc<Mutex<Connection>>>,
    pub(super) host: Option<Arc<Host>>,
    pub(super) home: PathBuf,
    pub(super) watch: Duration,
    /// Gone once the server is dropped, as when a test restarts it.
    pub(super) server: Weak<()>,
}

/// The reply to a request sent while the daemon is about to restart: nothing was started, and
/// the client sends it again (see `ignite_cli`'s `reattach`).
pub fn restarting_reply() -> Reply {
    Reply {
        exit: Exit::Unreachable,
        text: "the daemon is restarting into a new build; send the request again".to_owned(),
        json: Some(serde_json::json!({ "restarting": true })),
        handle: None,
    }
}

/// Whether `reply` is [`restarting_reply`].
pub fn is_restarting(reply: &Reply) -> bool {
    reply.exit == Exit::Unreachable
        && reply
            .json
            .as_ref()
            .and_then(|json| json.get("restarting"))
            .and_then(serde_json::Value::as_bool)
            == Some(true)
}

impl Server {
    /// Whether `verb` runs in a verb process.
    pub(super) fn runs_in_process(&self, verb: &str) -> bool {
        self.processes
            .as_ref()
            .is_some_and(|processes| processes.verbs.contains(verb))
    }

    pub(super) fn shared(&self) -> Shared {
        Shared {
            ctx: self.ctx.clone(),
            handles: self.handles.clone(),
            running: self.running.clone(),
            steps: self.steps.clone(),
            db: self.db.clone(),
            host: self.processes.as_ref().map(|p| p.host.clone()),
            home: self
                .processes
                .as_ref()
                .map(|p| p.home.clone())
                .unwrap_or_default(),
            watch: self
                .processes
                .as_ref()
                .map_or(Duration::from_secs(1), |p| p.watch),
            server: Arc::downgrade(&self.token),
        }
    }

    /// Runs `verb` in a verb process (see the module documentation). A verb process that cannot
    /// be started runs the verb on a thread of the daemon instead.
    pub(super) fn run_in_process(
        &self,
        verb: Arc<dyn Verb>,
        ctx: &Context,
        req: &Request,
        caller: Option<CallerProbe>,
        mut progress: Option<&mut dyn FnMut(&str) -> bool>,
        named: Option<&mut dyn FnMut(&Handle)>,
    ) -> Reply {
        let processes = self
            .processes
            .as_ref()
            .expect("only a server with verb processes runs one");
        let handle = self.handles.create_for(
            &req.verb,
            ctx.session.as_ref(),
            ctx.tree.as_ref(),
            Some(req),
        );
        if let Some(named) = named {
            named(&handle);
        }
        let child = match self.start_process(&handle) {
            Ok(child) => child,
            Err(error) => {
                eprintln!(
                    "ignite: cannot start the verb process of {handle}, so `ignite {}` runs in \
                     the daemon: {error}",
                    req.verb
                );
                return self.run_on_thread(verb, ctx, req, caller, progress, Some(handle));
            }
        };
        processes.host.track(ctx, &handle);
        watch(
            self.shared(),
            &handle,
            Some(child),
            self.registry.resumable(&req.verb),
        );

        let wait = Duration::from_secs(u64::from(req.wait_s));
        let deadline = Instant::now() + wait;
        let mut feed = StepFeed::new(self.steps.clone(), handle.clone());
        let slot: SharedSlot = Arc::new((Mutex::new(Slot::Pending), std::sync::Condvar::new()));
        {
            let (slot, handles, waited) = (slot.clone(), self.handles.clone(), handle.clone());
            let spawned = std::thread::Builder::new()
                .name(format!("ignite-wait-{handle}"))
                .spawn(move || {
                    if let crate::handles::Waited::Done(reply) =
                        handles.wait(&waited, req_wait_s(wait), None)
                    {
                        let mut state = lock(&slot.0);
                        if matches!(*state, Slot::Pending) {
                            *state = Slot::Done(reply);
                        }
                    }
                    slot.1.notify_all();
                });
            if let Err(error) = spawned {
                eprintln!("ignite: cannot wait for {handle}: {error}");
            }
        }
        let mut open = progress.is_some();
        loop {
            if !matches!(*lock(&slot.0), Slot::Pending) {
                break;
            }
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                break;
            };
            let lines = feed.next(left.min(Duration::from_millis(200)));
            if let Some(progress) = progress.as_mut() {
                for line in lines {
                    open = open && progress(&line);
                }
            }
        }
        if open && let Some(progress) = progress.as_mut() {
            for line in feed.next(Duration::ZERO) {
                if !progress(&line) {
                    break;
                }
            }
        }
        let mut state = lock(&slot.0);
        if let Slot::Done(reply) = std::mem::replace(&mut *state, Slot::Pending) {
            self.handles.delivered(&handle);
            return reply;
        }
        *state = Slot::HandedOff(handle.clone());
        drop(state);
        let doing = self
            .steps
            .by_handle(&handle)
            .map(|activity| activity.view());
        super::still_running_at(&req.verb, &handle, doing.as_ref())
    }

    /// Starts `handle`'s verb process and waits until it holds its lock, or has exited, or
    /// [`LOCK_WAIT`] has passed.
    fn start_process(&self, handle: &Handle) -> io::Result<Box<dyn VerbChild>> {
        let processes = self
            .processes
            .as_ref()
            .expect("only a server with verb processes starts one");
        // Counted while it starts, so a restart waits until the process holds its lock: a
        // daemon that started in between would take it for a dead one and run it twice.
        self.running.fetch_add(1, Ordering::SeqCst);
        let started = processes.starter.start(handle).map(|mut child| {
            let since = Instant::now();
            while !crate::verbproc::alive(&processes.home, handle)
                && !child.exited()
                && since.elapsed() < LOCK_WAIT
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            child
        });
        self.running.fetch_sub(1, Ordering::SeqCst);
        started
    }

    /// Starts the internal verb that `req` names for a loop of the daemon and returns its handle
    /// at once; the loop waits on the handle (see [`Handles::wait`]). The verb runs in a verb
    /// process when it is one of those, and otherwise, or when no verb process can be started,
    /// on a thread of the daemon as `verb`. An internal verb is never run again: if its verb
    /// process dies, its handle ends `incomplete` and the loop starts afresh.
    pub fn start_internal(&self, verb: Arc<dyn Verb>, req: Request) -> Handle {
        let handle = self.handles.create_for(&req.verb, None, None, Some(&req));
        if self.runs_in_process(&req.verb) {
            match self.start_process(&handle) {
                Ok(child) => {
                    if let Some(processes) = &self.processes {
                        processes.host.track(&self.ctx, &handle);
                    }
                    watch(self.shared(), &handle, Some(child), None);
                    return handle;
                }
                Err(error) => eprintln!(
                    "ignite: cannot start the verb process of {handle}, so `{}` runs in the \
                     daemon: {error}",
                    req.verb
                ),
            }
        }
        let shared = self.shared();
        let name = req.verb.clone();
        super::resume::run_on_daemon(&shared, &handle, &name, verb, req);
        handle
    }

    /// The handle of a running `verb`, such as an internal verb whose verb process outlived the
    /// previous daemon: a restarted loop waits for it instead of starting another.
    pub fn running_internal(&self, verb: &str) -> Option<Handle> {
        let db = self.db.as_ref()?;
        let rows = handle_store::running(&lock(db)).ok()?;
        rows.into_iter()
            .filter(|row| row.verb == verb)
            .find_map(|row| Handle::new(row.id).ok())
    }

    /// Stops taking new requests and waits up to `within` for the verbs running on the daemon's
    /// own threads. Verb processes are not waited for.
    pub fn drain(&self, within: Duration) {
        self.draining
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let since = Instant::now();
        while self.running_verbs() > 0 && since.elapsed() < within {
            std::thread::sleep(Duration::from_millis(50));
        }
        if self.running_verbs() > 0 {
            eprintln!(
                "ignite: restarting with {} verb(s) still running in the daemon",
                self.running_verbs()
            );
        }
    }

    /// Takes requests again, after a restart that failed.
    pub fn undrain(&self) {
        self.draining
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }

    /// Whether the daemon refuses `verb` because it is about to restart. The calls of verb
    /// processes are still answered: a verb process repeats any call a restart cuts off.
    pub(super) fn refused_while_draining(&self, verb: &str) -> bool {
        self.draining.load(std::sync::atomic::Ordering::SeqCst) && !verb.starts_with("host-")
    }

    /// For a handle a restarted daemon found running whose verb runs in a verb process: when the
    /// process is alive (it holds its lock), lists it and watches it, and returns true. A
    /// process that ended after writing its reply has its reply handed on, and also returns
    /// true. False when the process died, or none was started: the caller runs the verb again
    /// or ends it. A landing that runs under an older build than the daemon's is stopped and run
    /// again under the daemon's instead, unless its push has begun (see `server/outdated.rs`).
    pub(super) fn adopt_process(
        &self,
        handle: &Handle,
        verb: &str,
        request: Option<&Request>,
    ) -> bool {
        let Some(processes) = &self.processes else {
            return false;
        };
        let shared = self.shared();
        if crate::verbproc::alive(&processes.home, handle) {
            eprintln!("ignite: `ignite {verb}` ({handle}) still runs in its verb process");
            processes.host.track(&self.ctx, handle);
            if self.restart_if_outdated(handle, verb, request) {
                return true;
            }
            watch(shared, handle, None, self.registry.resumable(verb));
            return true;
        }
        shared.deliver_if_finished(handle)
    }
}

/// What a restart into a new build asks of the server first (see `main/selfupdate.rs`).
pub trait Drain: Send + Sync {
    /// Stops taking new requests and waits up to `within` for the verbs running in the daemon.
    fn drain(&self, within: Duration);
    /// Takes requests again, after a restart that failed.
    fn undrain(&self);
}

impl Drain for Server {
    fn drain(&self, within: Duration) {
        Server::drain(self, within);
    }

    fn undrain(&self) {
        Server::undrain(self);
    }
}

fn req_wait_s(wait: Duration) -> u32 {
    u32::try_from(wait.as_secs()).unwrap_or(u32::MAX)
}

impl Shared {
    pub(super) fn row(&self, handle: &Handle) -> Option<ignite_store::local::rows::Handle> {
        let db = self.db.as_ref()?;
        handle_store::get(&lock(db), handle.as_str()).ok().flatten()
    }

    /// Hands the reply in `handle`'s row to whoever waits, if the row has finished. True if it
    /// had, or the handle is unknown.
    pub(super) fn deliver_if_finished(&self, handle: &Handle) -> bool {
        let Some(row) = self.row(handle) else {
            return true;
        };
        if row.state == "running" {
            return false;
        }
        let reply = row
            .reply
            .as_deref()
            .and_then(|text| serde_json::from_str::<Reply>(text).ok());
        if let Some(reply) = reply {
            match row.state.as_str() {
                "stopped" => self.handles.finish_stopped(handle, reply),
                "incomplete" => self.handles.finish_incomplete(handle, reply),
                _ => self.handles.finish(handle, reply),
            }
        }
        true
    }

    /// Forgets the verb process that `handle`'s row names, once the daemon runs the verb itself,
    /// so the row no longer says a process runs it.
    pub(super) fn forget_process(&self, handle: &Handle) {
        let Some(db) = &self.db else {
            return;
        };
        let db = lock(db);
        if let Ok(Some(mut row)) = handle_store::get(&db, handle.as_str())
            && row.pid.is_some()
        {
            row.pid = None;
            row.executable = None;
            row.protocol = None;
            if let Err(error) = handle_store::upsert(&db, &row) {
                eprintln!("ignite: cannot forget the verb process of {handle}: {error}");
            }
        }
    }

    /// The verb process of `handle` died: ends the commands it left running, so nothing runs
    /// twice, and lets the daemon run the verb itself.
    pub(super) fn bury(&self, handle: &Handle) {
        if let Some(host) = &self.host {
            host.end_runs_of(handle);
        }
        if let Some(activity) = self.steps.by_handle(handle) {
            activity.finish();
        }
        self.forget_process(handle);
    }
}

/// Watches `handle`'s verb process until its handle finishes; runs the verb again with `rerun`,
/// or ends it `incomplete` without one, if the process ends first.
fn watch(
    shared: Shared,
    handle: &Handle,
    mut child: Option<Box<dyn VerbChild>>,
    rerun: Option<Arc<dyn Verb>>,
) {
    let name = format!("ignite-watch-{handle}");
    let watched = handle.clone();
    let spawned = std::thread::Builder::new().name(name).spawn(move || {
        let handle = watched;
        loop {
            std::thread::sleep(shared.watch);
            if shared.server.upgrade().is_none() {
                return;
            }
            if shared.deliver_if_finished(&handle) {
                return;
            }
            let ended = match child.as_mut() {
                Some(child) => child.exited(),
                None => !crate::verbproc::alive(&shared.home, &handle),
            };
            if !ended {
                continue;
            }
            // The process writes its reply before it exits.
            if shared.deliver_if_finished(&handle) {
                return;
            }
            died(&shared, &handle, rerun);
            return;
        }
    });
    if let Err(error) = spawned {
        eprintln!("ignite: cannot watch the verb process of {handle}: {error}");
    }
}

/// The verb process of `handle` ended without writing its reply.
fn died(shared: &Shared, handle: &Handle, rerun: Option<Arc<dyn Verb>>) {
    let Some(db) = &shared.db else {
        return;
    };
    let found = crate::handles::running_request(&lock(db), handle);
    let Ok(Some((verb, request))) = found else {
        return;
    };
    eprintln!("ignite: the verb process of `ignite {verb}` ({handle}) ended without a reply");
    shared.bury(handle);
    let rerun = rerun.ok_or("it cannot be run again safely");
    super::resume::run_again(shared, handle, &verb, rerun.map(|verb| (verb, request)));
}

#[cfg(all(test, unix))]
mod reap_tests {
    use super::*;

    /// The process table's state letters for `pid`, empty once it is gone.
    fn state(pid: u32) -> String {
        let out = Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    #[test]
    fn a_dropped_verb_process_is_reaped() {
        let child = Command::new("sh").args(["-c", "exit 0"]).spawn().unwrap();
        let pid = child.id();
        reap_in_background(child).unwrap().join().unwrap();
        assert_eq!(state(pid), "", "the process is still in the process table");
    }

    #[test]
    fn an_unreaped_process_stays_a_zombie() {
        // Guards the test above: without a wait, the state is `Z`.
        let child = Command::new("sh").args(["-c", "exit 0"]).spawn().unwrap();
        let pid = child.id();
        while !state(pid).starts_with('Z') {
            std::thread::yield_now();
        }
        drop(child);
        assert!(state(pid).starts_with('Z'));
    }
}

#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;
