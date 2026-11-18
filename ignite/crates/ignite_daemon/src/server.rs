//! Requests from clients.
//!
//! The client sends one JSON line over the machine's local socket and reads one JSON line back.
//! A [`Verb`] answers one verb; the [`Registry`] finds the verb a request names; the [`Server`]
//! builds each request's [`Context`], runs the verb, turns a verb that outlives the caller's wait
//! into a handle (see [`crate::handles`]), runs the [`Hook`]s for requests from a leased tree and
//! remembers the command each session ran last.
//!
//! Only the session that holds a build tree's lease may use the tree. A request whose working
//! directory is inside a tree that the local database has not leased to the asking session is
//! refused with exit code 3 before its verb runs, so a session that was replaced cannot go on
//! working in a tree that now belongs to someone else. A verb that does not act on the tree it
//! is asked from opts out with [`Verb::needs_the_tree_lease`].

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use ignite_core::config::Config;
use ignite_core::ids::{Handle, MachineId, SessionId, TreeId};
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::time::Timestamp;
use ignite_core::traits::{Builds, Clock, Error, Git, Hub, Launcher};
use ignite_os::ipc::{Ipc, Listener, Stream};
use ignite_store::local::{rows, trees};

use crate::handles::{Handles, WaitVerb, lock};
use crate::steps::{self, Activity, Kind, Steps, Work};
use crate::stop::StopVerb;

/// What every verb may use. Tests build one from the fakes in `ignite_core::fakes`.
#[derive(Clone)]
pub struct Context {
    pub config: Config,
    /// The repository's main checkout.
    pub repo: PathBuf,
    pub machine: MachineId,
    pub clock: Arc<dyn Clock>,
    pub hub: Arc<dyn Hub>,
    pub git: Arc<dyn Git>,
    pub launcher: Arc<dyn Launcher>,
    pub builds: Arc<dyn Builds>,
    /// The session asking, as the client read it from its environment. `None` outside a
    /// request, and for a client that no session started.
    pub session: Option<SessionId>,
    /// The build tree that holds the client's working directory, if any does.
    pub tree: Option<TreeId>,
}

impl Context {
    /// The build tree that holds `cwd`: the first directory under the configured trees folder.
    pub fn tree_of(&self, cwd: &Path) -> Option<TreeId> {
        let root = self.repo.join(&self.config.repository.trees);
        let canonical = root.canonicalize().ok();
        let relative = cwd
            .strip_prefix(&root)
            .ok()
            .or_else(|| cwd.strip_prefix(canonical.as_ref()?).ok())?;
        match relative.components().next()? {
            Component::Normal(name) => TreeId::new(name.to_str()?).ok(),
            _ => None,
        }
    }
}

/// One verb of the `ignite` command, run inside the daemon.
pub trait Verb: Send + Sync {
    /// The verb as the client sends it, such as `land`.
    fn name(&self) -> &str;
    fn run(&self, ctx: &Context, req: &Request) -> Reply;
    /// True for a verb that keeps to the request's `wait_s` by itself, such as `wait`. The server
    /// runs such a verb directly instead of handing it a handle when the wait ends.
    fn keeps_its_own_wait(&self) -> bool {
        false
    }
    /// False for a verb that any session may run from inside a tree it does not hold, because
    /// the verb does not act on that tree: `start` (it leases a tree of the daemon's choosing),
    /// `wait` (it reads a handle's reply), and `status` and `doctor` (they only read).
    fn needs_the_tree_lease(&self) -> bool {
        true
    }
    /// False for a verb that only waits for something or only reads, so that a daemon waiting
    /// to update does not wait for it: the dispatcher's `dispatch --wait` and a worker's `wait`
    /// are always running, and an update that waited for them would never start.
    fn blocks_update(&self) -> bool {
        true
    }
}

/// Tells whether the client of the request that the current verb answers has closed its
/// connection before the verb replied.
pub type CallerProbe = Arc<dyn Fn() -> bool + Send + Sync>;

thread_local! {
    /// The probe of the request the verb on this thread is answering, if it can be made.
    static CALLER: RefCell<Option<CallerProbe>> = const { RefCell::new(None) };
}

/// The probe for the request that the calling verb is answering, or `None` where the
/// connection cannot be probed. It reports true only until the request is handed off to a
/// handle, because a client that leaves after its wait ended has done nothing wrong.
pub fn caller_probe() -> Option<CallerProbe> {
    CALLER.with(|caller| caller.borrow().clone())
}

/// The verbs the daemon answers, by name.
#[derive(Default)]
pub struct Registry {
    verbs: BTreeMap<String, Arc<dyn Verb>>,
}

impl Registry {
    /// Adds a verb. Registering two verbs under one name is a programming error.
    pub fn register(&mut self, verb: Box<dyn Verb>) {
        let name = verb.name().to_owned();
        let previous = self.verbs.insert(name.clone(), Arc::from(verb));
        assert!(previous.is_none(), "the verb {name:?} is registered twice");
    }

    /// The registered verbs' names, sorted.
    pub fn names(&self) -> Vec<&str> {
        self.verbs.keys().map(String::as_str).collect()
    }

    /// The verb registered under `name`.
    pub fn get(&self, name: &str) -> Option<Arc<dyn Verb>> {
        self.verbs.get(name).cloned()
    }

    /// Runs the verb the request names in this thread, or answers with a usage error if none
    /// is registered.
    pub fn handle(&self, ctx: &Context, req: &Request) -> Reply {
        match self.verbs.get(&req.verb) {
            Some(verb) => verb.run(ctx, req),
            None => unknown_verb(&req.verb),
        }
    }
}

fn unknown_verb(verb: &str) -> Reply {
    Reply::text(Exit::Usage, format!("unknown verb: {verb}"))
}

/// Code that runs on every request from a leased tree, after the verb and before the reply is
/// sent. Lease renewal and the `changed` lines about edits to the tree's ticket are hooks.
pub trait Hook: Send + Sync {
    /// Lines to print before the verb's own output; usually none.
    fn run(&self, ctx: &Context, req: &Request, tree: &rows::Tree) -> Vec<String>;
}

/// The `ignite` command a session is running, or ran last.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CurrentCommand {
    pub verb: String,
    /// The first argument after the verb, such as the handle of `ignite wait L-5521`.
    pub arg: Option<String>,
    pub started_at: Timestamp,
    /// `None` while the command is still running.
    pub ended_at: Option<Timestamp>,
}

/// The daemon's request handling, shared by every connection's thread.
pub struct Server {
    ctx: Context,
    registry: Registry,
    handles: Arc<Handles>,
    db: Option<Arc<Mutex<Connection>>>,
    hooks: Vec<Box<dyn Hook>>,
    commands: Mutex<BTreeMap<SessionId, CurrentCommand>>,
    /// What each running verb is doing (see [`crate::steps`]).
    steps: Arc<Steps>,
    /// Verbs running on threads of their own, including those handed off to a handle, except
    /// those that do not [block an update](Verb::blocks_update).
    running: Arc<AtomicUsize>,
}

impl Server {
    /// A server for `registry`'s verbs, with `wait` added. `db` is the daemon's local database:
    /// handles outlive a restart there, and the server reads which trees are leased from it.
    /// Without it, handles live in memory and no tree counts as leased.
    pub fn new(
        ctx: Context,
        registry: Registry,
        db: Option<Arc<Mutex<Connection>>>,
    ) -> ignite_core::traits::Result<Server> {
        Self::with_steps(ctx, registry, db, steps::global())
    }

    /// [`Server::new`], with the running verbs listed in `steps` instead of the daemon's
    /// registry. `wait` and `stop` are added.
    pub fn with_steps(
        ctx: Context,
        mut registry: Registry,
        db: Option<Arc<Mutex<Connection>>>,
        steps: Arc<Steps>,
    ) -> ignite_core::traits::Result<Server> {
        let handles = Arc::new(match &db {
            Some(db) => Handles::open(ctx.clock.clone(), db.clone())?,
            None => Handles::new(ctx.clock.clone()),
        });
        registry.register(Box::new(WaitVerb::new(handles.clone(), steps.clone())));
        registry.register(Box::new(StopVerb::new(handles.clone(), steps.clone())));
        Ok(Server {
            ctx,
            registry,
            handles,
            db,
            hooks: Vec::new(),
            commands: Mutex::new(BTreeMap::new()),
            steps,
            running: Arc::new(AtomicUsize::new(0)),
        })
    }

    /// What each running verb is doing.
    pub fn steps(&self) -> &Arc<Steps> {
        &self.steps
    }

    /// The step of the verb `session` is running, or of the handle its `ignite wait` waits on,
    /// such as `building rust-tests in landing-3 (4 min)`, for the dashboard's "current command".
    pub fn current_step(&self, session: &SessionId) -> Option<String> {
        let command = self.current_command(session)?;
        if command.ended_at.is_some() {
            return None;
        }
        let live = self.steps.live();
        let waited = command
            .arg
            .as_deref()
            .filter(|_| command.verb == "wait")
            .and_then(|arg| Handle::new(arg).ok());
        let activity = match waited {
            Some(handle) => live
                .into_iter()
                .find(|a| a.handle() == Some(handle.clone())),
            None => live
                .into_iter()
                .rev()
                .find(|a| a.work().session.as_ref() == Some(session)),
        }?;
        Some(activity.view().doing())
    }

    /// Adds a hook. Hooks run in the order they were added.
    pub fn add_hook(&mut self, hook: Box<dyn Hook>) {
        self.hooks.push(hook);
    }

    /// The operations that outlived their callers' waits.
    pub fn handles(&self) -> &Arc<Handles> {
        &self.handles
    }

    /// The command `session` is running, or the one it ran last.
    pub fn current_command(&self, session: &SessionId) -> Option<CurrentCommand> {
        lock(&self.commands).get(session).cloned()
    }

    /// The number of verbs still running that an update has to wait for, whether or not their
    /// callers still wait. `wait` itself is not counted: it only waits on another verb, and
    /// neither is any verb that [does not block an update](Verb::blocks_update).
    pub fn running_verbs(&self) -> usize {
        self.running.load(Ordering::SeqCst)
    }

    /// Answers one request.
    pub fn handle(&self, req: &Request) -> Reply {
        self.handle_from(req, None)
    }

    /// Answers one request whose client's connection `caller` probes, if it can.
    pub fn handle_from(&self, req: &Request, caller: Option<CallerProbe>) -> Reply {
        self.handle_streaming(req, caller, None)
    }

    /// [`Server::handle_from`], also passing each step of the verb to `progress` as it starts,
    /// until the reply or the handle. `progress` returns false once its client has gone; no
    /// step is passed after that.
    pub fn handle_streaming(
        &self,
        req: &Request,
        caller: Option<CallerProbe>,
        progress: Option<&mut dyn FnMut(&str) -> bool>,
    ) -> Reply {
        let mut ctx = self.ctx.clone();
        ctx.session.clone_from(&req.session);
        ctx.tree = ctx.tree_of(&req.cwd);
        let verb = self.registry.get(&req.verb);
        if verb.as_ref().is_none_or(|verb| verb.needs_the_tree_lease())
            && let Some(refusal) = self.refuse_without_lease(&ctx)
        {
            return refusal;
        }
        if let Some(session) = &req.session {
            lock(&self.commands).insert(
                session.clone(),
                CurrentCommand {
                    verb: req.verb.clone(),
                    arg: req.args.first().cloned(),
                    started_at: ctx.clock.now(),
                    ended_at: None,
                },
            );
        }
        let mut reply = match verb {
            None => unknown_verb(&req.verb),
            Some(verb) if verb.keeps_its_own_wait() => verb.run(&ctx, req),
            Some(verb) => self.run_or_hand_off(verb, &ctx, req, caller, progress),
        };
        self.run_hooks(&ctx, req, &mut reply);
        if let Some(session) = &req.session
            && let Some(command) = lock(&self.commands).get_mut(session)
        {
            command.ended_at = Some(ctx.clock.now());
        }
        reply
    }

    /// Runs the verb on a thread of its own. If it has not finished when the caller's wait
    /// ends, the reply names a handle, and the verb's reply goes to the handle when it finishes.
    fn run_or_hand_off(
        &self,
        verb: Arc<dyn Verb>,
        ctx: &Context,
        req: &Request,
        caller: Option<CallerProbe>,
        mut progress: Option<&mut dyn FnMut(&str) -> bool>,
    ) -> Reply {
        let activity = self.steps.begin(
            Kind::Verb(req.verb.clone()),
            Work {
                class: None,
                ticket: self
                    .leased_tree(ctx)
                    .and_then(|tree| tree.ticket)
                    .and_then(|ticket| ignite_core::ids::TicketId::new(ticket).ok()),
                session: ctx.session.clone(),
                tree: ctx.tree.clone(),
                tree_path: ctx.tree.as_ref().map(|tree| {
                    ctx.repo
                        .join(&ctx.config.repository.trees)
                        .join(tree.as_str())
                }),
            },
        );
        let steps_heard = progress.is_some().then(|| activity.subscribe());
        let slot = Arc::new((Mutex::new(Slot::Pending), Condvar::new()));
        let counted = verb.blocks_update();
        if counted {
            self.running.fetch_add(1, Ordering::SeqCst);
        }
        // Set once the caller has been given a handle and may leave.
        let handed_off = Arc::new(AtomicBool::new(false));
        let probe: Option<CallerProbe> = caller.map(|gone| {
            let handed_off = handed_off.clone();
            Arc::new(move || !handed_off.load(Ordering::SeqCst) && gone()) as CallerProbe
        });
        let worker = {
            let (slot, handles, ctx, req, running, activity) = (
                slot.clone(),
                self.handles.clone(),
                ctx.clone(),
                req.clone(),
                self.running.clone(),
                activity.clone(),
            );
            move || {
                CALLER.with(|caller| *caller.borrow_mut() = probe);
                let entered = activity.enter();
                let mut reply =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| verb.run(&ctx, &req)))
                        .unwrap_or_else(|_| {
                            Reply::text(
                                Exit::Usage,
                                format!("failed   `{}` panicked in the daemon", req.verb),
                            )
                        });
                drop(entered);
                let stopped = activity.stop_requested();
                if stopped {
                    reply = stopped_reply(&req.verb, activity.handle().as_ref());
                }
                let mut state = lock(&slot.0);
                match &*state {
                    Slot::HandedOff(handle) if stopped => handles.finish_stopped(handle, reply),
                    Slot::HandedOff(handle) => handles.finish(handle, reply),
                    _ => *state = Slot::Done(reply),
                }
                if counted {
                    running.fetch_sub(1, Ordering::SeqCst);
                }
                slot.1.notify_all();
                drop(state);
                // After the reply is kept, so that whoever waits for the work to end finds it.
                activity.finish();
            }
        };
        let name = format!("ignite-verb-{}", req.verb);
        if let Err(error) = std::thread::Builder::new().name(name).spawn(worker) {
            if counted {
                self.running.fetch_sub(1, Ordering::SeqCst);
            }
            return Reply::text(
                Exit::Usage,
                format!("failed   cannot start a thread: {error}"),
            );
        }

        let wait = Duration::from_secs(u64::from(req.wait_s));
        if let (Some(heard), Some(progress)) = (steps_heard, progress.as_mut()) {
            pass_steps(&slot, &activity, &heard, wait, &mut **progress);
        }
        let (state, finished) = &*slot;
        let guard = lock(state);
        let (mut guard, _) = finished
            .wait_timeout_while(guard, wait, |s| matches!(s, Slot::Pending))
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Slot::Done(reply) = std::mem::replace(&mut *guard, Slot::Pending) {
            return reply;
        }
        let handle = self
            .handles
            .create(&req.verb, ctx.session.as_ref(), ctx.tree.as_ref());
        activity.attach_handle(&handle);
        activity.unsubscribe_all();
        *guard = Slot::HandedOff(handle.clone());
        handed_off.store(true, Ordering::SeqCst);
        still_running_at(&req.verb, &handle, Some(&activity.view()))
    }

    fn run_hooks(&self, ctx: &Context, req: &Request, reply: &mut Reply) {
        if self.hooks.is_empty() {
            return;
        }
        let Some(tree) = self.leased_tree(ctx) else {
            return;
        };
        let lines: Vec<String> = self
            .hooks
            .iter()
            .flat_map(|hook| hook.run(ctx, req, &tree))
            .collect();
        if !lines.is_empty() {
            let mut text = lines.join("\n");
            if !reply.text.is_empty() {
                text.push('\n');
                text.push_str(&reply.text);
            }
            reply.text = text;
        }
    }

    /// The refusal for a request from inside a tree that the local database knows and has not
    /// leased to the asking session: free, being reset, landing, or held by another session.
    /// A tree the database has no row for, or a server without a database, refuses nothing.
    fn refuse_without_lease(&self, ctx: &Context) -> Option<Reply> {
        let id = ctx.tree.as_ref()?;
        let db = self.db.as_ref()?;
        let tree = match trees::get(&lock(db), id.as_str()) {
            Ok(tree) => tree?,
            Err(error) => {
                let text = format!("failed   cannot read the lease of {id}: {error}");
                return Some(Reply::text(Exit::Usage, text));
            }
        };
        let asking = ctx.session.as_ref().map(SessionId::as_str);
        if tree.state == trees::State::Leased.as_str()
            && asking.is_some()
            && tree.session.as_deref() == asking
        {
            return None;
        }
        let now = match (tree.state.as_str(), tree.session.as_deref()) {
            ("leased", Some(holder)) => format!("leased to {holder}"),
            (state, _) => state.to_owned(),
        };
        let asker = asking.unwrap_or("a client no session started");
        Some(Reply::text(
            Exit::Conflict,
            format!(
                "conflict {id} is not leased to {asker} (it is {now}); only the session that \
                 holds a tree may use it"
            ),
        ))
    }

    /// The request's tree, when the local database has it leased to someone.
    fn leased_tree(&self, ctx: &Context) -> Option<rows::Tree> {
        let id = ctx.tree.as_ref()?;
        let db = self.db.as_ref()?;
        let tree = trees::get(&lock(db), id.as_str()).ok()??;
        (tree.state != trees::State::Free.as_str()).then_some(tree)
    }
}

/// Passes each step `heard` from the verb's thread to `progress` until the verb has replied or
/// `wait` has passed. The wait for the reply itself follows.
fn pass_steps(
    slot: &(Mutex<Slot>, Condvar),
    activity: &Activity,
    heard: &std::sync::mpsc::Receiver<String>,
    wait: Duration,
    progress: &mut dyn FnMut(&str) -> bool,
) {
    let deadline = Instant::now() + wait;
    loop {
        if !matches!(*lock(&slot.0), Slot::Pending) {
            // The verb has replied; pass on the steps it recorded before that.
            for line in heard.try_iter() {
                if !progress(&line) {
                    break;
                }
            }
            return;
        }
        let Some(left) = deadline.checked_duration_since(Instant::now()) else {
            return;
        };
        match heard.recv_timeout(left) {
            Ok(line) => {
                if !progress(&line) {
                    activity.unsubscribe_all();
                    return;
                }
            }
            // The verb has finished (or the deadline came); the reply is in the slot.
            Err(_) => return,
        }
    }
}

/// The reply of a verb that was stopped (see [`crate::stop`]).
pub fn stopped_reply(verb: &str, handle: Option<&Handle>) -> Reply {
    let name = handle.map_or_else(String::new, |h| format!(" ({h})"));
    Reply {
        exit: Exit::Conflict,
        text: format!(
            "stopped  `ignite {verb}`{name} was stopped before it finished; its processes were \
             ended and its cores, trees and lease given back\nnext     run it again when you \
             want it"
        ),
        json: Some(serde_json::json!({ "verb": verb, "handle": handle, "stopped": true })),
        handle: None,
    }
}

/// Where a verb running on its own thread leaves its reply.
enum Slot {
    Pending,
    Done(Reply),
    HandedOff(Handle),
}

/// The reply to a request whose verb is still running when the caller's wait ends.
pub fn still_running(verb: &str, handle: &Handle) -> Reply {
    still_running_at(verb, handle, None)
}

/// [`still_running`], with what the verb is doing now, when known.
pub fn still_running_at(verb: &str, handle: &Handle, doing: Option<&steps::View>) -> Reply {
    let doing = doing.filter(|view| view.step.is_some() || view.waiting_for.is_some());
    let step = doing.map_or_else(String::new, |view| {
        format!(
            "\nstep     {} (started {} ago)",
            view.doing(),
            crate::loop_health::span(view.age_s)
        )
    });
    Reply {
        exit: Exit::Running,
        text: format!(
            "waiting  `ignite {verb}` is still running{step}\nnext     ignite wait {handle}"
        ),
        json: Some(serde_json::json!({
            "verb": verb,
            "handle": handle,
            "step": doing.and_then(|view| view.step.clone()),
            "step_age_s": doing.and_then(|view| view.step_age_s),
            "waiting_for": doing.and_then(|view| view.waiting_for.clone()),
        })),
        handle: Some(handle.clone()),
    }
}

/// The reply for an error from the hub, git or the local store, with the exit code every verb
/// shares: 4 when the hub or remote cannot be reached, 3 for a conflict, otherwise 1.
pub fn error_reply(error: &Error) -> Reply {
    let (exit, label) = match error {
        Error::Unreachable(_) => (Exit::Unreachable, "offline"),
        Error::Conflict(_) => (Exit::Conflict, "conflict"),
        Error::NotFound(_) | Error::Invalid(_) | Error::Io(_) => (Exit::Usage, "failed"),
    };
    Reply::text(exit, format!("{label:<8} {error}"))
}

/// The daemon's socket: `daemon.sock` in the machine's Ignite home (`$IGNITE_HOME`, or
/// `~/.config/ignite`).
pub fn default_socket() -> PathBuf {
    crate::update::Home::from_env().root.join("daemon.sock")
}

/// Listens on `socket` and answers every client from `server`, one thread per connection,
/// until the process ends.
pub fn serve(server: &Arc<Server>, socket: &str) -> std::io::Result<()> {
    let listener = ignite_os::Native.listen(socket)?;
    serve_listener(server, listener.as_ref())
}

/// How long a client that has connected has to send its request. A client that sends nothing
/// would otherwise hold its thread for as long as it stays connected.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// The longest pause after failed accepts in a row.
const ACCEPT_PAUSE_MAX: Duration = Duration::from_secs(2);

/// [`serve`] on a listener that is already open. A failed accept (as when the process has run
/// out of file descriptors for a moment) is reported and tried again after a pause that grows
/// while they continue; it never ends the daemon.
pub fn serve_listener(server: &Arc<Server>, listener: &dyn Listener) -> std::io::Result<()> {
    let mut pause = Duration::from_millis(50);
    loop {
        let stream = match listener.accept() {
            Ok(stream) => stream,
            Err(error) => {
                eprintln!("ignite: cannot accept a connection: {error}");
                std::thread::sleep(pause);
                pause = (pause * 2).min(ACCEPT_PAUSE_MAX);
                continue;
            }
        };
        pause = Duration::from_millis(50);
        let server = server.clone();
        let spawned = std::thread::Builder::new()
            .name("ignite-connection".to_owned())
            .spawn(move || answer(&server, stream));
        if let Err(error) = spawned {
            eprintln!("ignite: cannot start a thread for a connection: {error}");
        }
    }
}

/// Reads one request line from `stream` and writes the reply line.
fn answer(server: &Server, mut stream: Box<dyn Stream>) {
    let _ = stream.set_read_timeout(Some(REQUEST_TIMEOUT));
    let mut line = String::new();
    if BufReader::new(&mut stream).read_line(&mut line).is_err() {
        return;
    }
    let _ = stream.set_read_timeout(None);
    let caller: Option<CallerProbe> = stream.closed_probe().map(Arc::from);
    let wants = serde_json::from_str::<Wants>(&line).unwrap_or_default();
    let reply = match serde_json::from_str::<Request>(&line) {
        Ok(req) if wants.progress => {
            let mut open = true;
            let mut pass = |text: &str| {
                open = open && write_line(&mut stream, &Progress { progress: text }).is_ok();
                open
            };
            server.handle_streaming(&req, caller, Some(&mut pass))
        }
        Ok(req) => server.handle_from(&req, caller),
        Err(error) => Reply::text(Exit::Usage, format!("failed   unreadable request: {error}")),
    };
    // The client may have gone away, as when its tool call was interrupted; nothing is lost,
    // because a reply that outlives its caller is kept under a handle.
    let _ = write_line(&mut stream, &reply);
}

fn write_line(stream: &mut dyn Write, value: &impl Serialize) -> std::io::Result<()> {
    let mut bytes = serde_json::to_vec(value).map_err(std::io::Error::other)?;
    bytes.push(b'\n');
    stream.write_all(&bytes)?;
    stream.flush()
}

/// What a client asks for beside its [`Request`], read from the same JSON line. A daemon that
/// does not know a field ignores it, and a client that sends none gets one reply line.
#[derive(Default, Deserialize)]
struct Wants {
    /// Send each step of the verb as a [`Progress`] line before the reply.
    #[serde(default)]
    progress: bool,
}

/// A line sent before the reply to a client that asked for progress: a step of the verb.
#[derive(Serialize)]
struct Progress<'a> {
    progress: &'a str,
}

/// A [`Progress`] line as the client reads it.
#[derive(Deserialize)]
struct ProgressLine {
    progress: String,
}

/// The client's side: sends `req` to the daemon listening on `socket` and reads its reply.
pub fn send(socket: &str, req: &Request) -> std::io::Result<Reply> {
    send_reading(socket, req, None)
}

/// [`send`], asking the daemon for each step of the verb as it starts and passing it to
/// `progress` before the reply comes.
pub fn send_reading(
    socket: &str,
    req: &Request,
    progress: Option<&mut dyn FnMut(&str)>,
) -> std::io::Result<Reply> {
    let mut stream = ignite_os::Native.connect(socket)?;
    let mut value = serde_json::to_value(req).map_err(std::io::Error::other)?;
    if progress.is_some()
        && let Some(object) = value.as_object_mut()
    {
        object.insert("progress".to_owned(), serde_json::Value::Bool(true));
    }
    write_line(&mut stream, &value)?;
    read_reply(BufReader::new(&mut stream), progress)
}

/// Reads [`Progress`] lines, passing each to `progress`, until the reply line.
fn read_reply(
    mut reader: impl BufRead,
    mut progress: Option<&mut dyn FnMut(&str)>,
) -> std::io::Result<Reply> {
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Err(std::io::Error::other("the daemon closed the connection"));
        }
        if let Ok(step) = serde_json::from_str::<ProgressLine>(&line) {
            if let Some(progress) = progress.as_mut() {
                progress(&step.progress);
            }
            continue;
        }
        return serde_json::from_str(&line).map_err(std::io::Error::other);
    }
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
