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

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

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
    /// Verbs running on threads of their own, including those handed off to a handle.
    running: Arc<AtomicUsize>,
}

impl Server {
    /// A server for `registry`'s verbs, with `wait` added. `db` is the daemon's local database:
    /// handles outlive a restart there, and the server reads which trees are leased from it.
    /// Without it, handles live in memory and no tree counts as leased.
    pub fn new(
        ctx: Context,
        mut registry: Registry,
        db: Option<Arc<Mutex<Connection>>>,
    ) -> ignite_core::traits::Result<Server> {
        let handles = Arc::new(match &db {
            Some(db) => Handles::open(ctx.clock.clone(), db.clone())?,
            None => Handles::new(ctx.clock.clone()),
        });
        registry.register(Box::new(WaitVerb::new(handles.clone())));
        Ok(Server {
            ctx,
            registry,
            handles,
            db,
            hooks: Vec::new(),
            commands: Mutex::new(BTreeMap::new()),
            running: Arc::new(AtomicUsize::new(0)),
        })
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

    /// The number of verbs still running, whether or not their callers still wait. `wait`
    /// itself is not counted: it only waits on another verb.
    pub fn running_verbs(&self) -> usize {
        self.running.load(Ordering::SeqCst)
    }

    /// Answers one request.
    pub fn handle(&self, req: &Request) -> Reply {
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
            Some(verb) => self.run_or_hand_off(verb, &ctx, req),
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
    fn run_or_hand_off(&self, verb: Arc<dyn Verb>, ctx: &Context, req: &Request) -> Reply {
        let slot = Arc::new((Mutex::new(Slot::Pending), Condvar::new()));
        self.running.fetch_add(1, Ordering::SeqCst);
        let worker = {
            let (slot, handles, ctx, req, running) = (
                slot.clone(),
                self.handles.clone(),
                ctx.clone(),
                req.clone(),
                self.running.clone(),
            );
            move || {
                let reply =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| verb.run(&ctx, &req)))
                        .unwrap_or_else(|_| {
                            Reply::text(
                                Exit::Usage,
                                format!("failed   `{}` panicked in the daemon", req.verb),
                            )
                        });
                let mut state = lock(&slot.0);
                match &*state {
                    Slot::HandedOff(handle) => handles.finish(handle, reply),
                    _ => *state = Slot::Done(reply),
                }
                running.fetch_sub(1, Ordering::SeqCst);
                slot.1.notify_all();
            }
        };
        std::thread::spawn(worker);

        let (state, finished) = &*slot;
        let guard = lock(state);
        let wait = Duration::from_secs(u64::from(req.wait_s));
        let (mut guard, _) = finished
            .wait_timeout_while(guard, wait, |s| matches!(s, Slot::Pending))
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Slot::Done(reply) = std::mem::replace(&mut *guard, Slot::Pending) {
            return reply;
        }
        let handle = self
            .handles
            .create(&req.verb, ctx.session.as_ref(), ctx.tree.as_ref());
        *guard = Slot::HandedOff(handle.clone());
        still_running(&req.verb, &handle)
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

/// Where a verb running on its own thread leaves its reply.
enum Slot {
    Pending,
    Done(Reply),
    HandedOff(Handle),
}

/// The reply to a request whose verb is still running when the caller's wait ends.
pub fn still_running(verb: &str, handle: &Handle) -> Reply {
    Reply {
        exit: Exit::Running,
        text: format!("waiting  `ignite {verb}` is still running\nnext     ignite wait {handle}"),
        json: Some(serde_json::json!({ "verb": verb, "handle": handle })),
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

/// [`serve`] on a listener that is already open.
pub fn serve_listener(server: &Arc<Server>, listener: &dyn Listener) -> std::io::Result<()> {
    loop {
        let stream = listener.accept()?;
        let server = server.clone();
        std::thread::spawn(move || answer(&server, stream));
    }
}

/// Reads one request line from `stream` and writes the reply line.
fn answer(server: &Server, mut stream: Box<dyn Stream>) {
    let mut line = String::new();
    if BufReader::new(&mut stream).read_line(&mut line).is_err() {
        return;
    }
    let reply = match serde_json::from_str::<Request>(&line) {
        Ok(req) => server.handle(&req),
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

/// The client's side: sends `req` to the daemon listening on `socket` and reads its reply.
pub fn send(socket: &str, req: &Request) -> std::io::Result<Reply> {
    let mut stream = ignite_os::Native.connect(socket)?;
    write_line(&mut stream, req)?;
    let mut line = String::new();
    BufReader::new(&mut stream).read_line(&mut line)?;
    serde_json::from_str(&line).map_err(std::io::Error::other)
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
