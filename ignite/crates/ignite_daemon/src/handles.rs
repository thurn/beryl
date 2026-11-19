//! Handles: an operation that would outlive the client's wait answers with a short name, such as
//! `L-5521`, and `ignite wait <handle>` blocks until it finishes.
//!
//! A verb that does work gets its handle before it starts (see [`crate::server`]), so that a
//! client whose connection drops, as when the daemon restarts into a new build, can wait on it
//! again.
//!
//! Each handle is kept in the local database as well as in memory, so a restarted daemon still
//! answers `wait` for a handle from before the restart. While its operation runs, the row's
//! `reply` column holds the request that started it, so that a restarted daemon can
//! run the verb again under the same handle (see [`crate::server::Server`]). An operation that
//! cannot be run again ends `incomplete`, with a reply that says why.
//!
//! A wait that ends before its operation does prints what the operation is doing now and for
//! how long (see [`crate::steps`]). An operation that was stopped (see [`crate::stop`]) is kept
//! as `stopped`.
//!
//! A client that asks for progress lines, as one whose connection dropped and now waits on the
//! handle again, gets each step of the operation as it starts while `ignite wait` waits, whether
//! the operation runs on a thread of the daemon or in a verb process ([`StepFeed`]).
//!
//! A session inside `ignite wait` can be interrupted: [`Handles::interrupt`] ends every wait of
//! that session at once with the reply it is given, such as a message from a person.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use ignite_core::ids::{Handle, SessionId, TreeId};
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::time::Timestamp;
use ignite_core::traits::{Clock, Error, Result};
use ignite_store::local::{handles as store, rows};

use crate::server::{Context, Verb, still_running_at};
use crate::steps::{Activity, Steps};

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// How a wait on a handle ended.
#[derive(Clone, Debug, PartialEq)]
pub enum Waited {
    /// The operation finished with this reply.
    Done(Reply),
    /// The wait ended first; the operation, of this verb, is still running.
    Running(String),
    /// [`Handles::interrupt`] ended the wait with this reply.
    Interrupted(Reply),
    /// No such handle exists.
    Unknown,
}

/// Seconds a finished operation's reply stays in memory. After a read it is dropped at once when
/// the local database keeps it; without a database it stays this long.
pub const KEEP_REPLY_S: f64 = 24.0 * 60.0 * 60.0;

/// What the `reply` column of a running handle's row holds: the request that started it.
#[derive(Serialize, Deserialize)]
struct Running {
    request: Request,
}

/// A handle that was still running when the daemon stopped, as a restarted daemon finds it.
#[derive(Clone, Debug, PartialEq)]
pub struct Orphan {
    pub handle: Handle,
    pub verb: String,
    /// The request that started it, if it was recorded (handles made by older daemons, or for
    /// work no request started, have none).
    pub request: Option<Request>,
}

/// Operations still running, and the replies of those that finished.
pub struct Handles {
    clock: Arc<dyn Clock>,
    db: Option<Arc<Mutex<Connection>>>,
    state: Mutex<State>,
    changed: Condvar,
}

#[derive(Default)]
struct State {
    /// The number the next handle tries first.
    next: u64,
    /// Every handle this process created or read: its verb, and its reply once finished.
    replies: HashMap<Handle, Entry>,
    /// Waits in progress, by a number of their own, with the interrupt each has been given.
    waiters: BTreeMap<u64, Waiter>,
    next_waiter: u64,
    /// Handles the database listed as running when it was opened, until the server takes them.
    orphans: Vec<Orphan>,
    /// Handles whose `stopped` reply is not handed to waiters, because the daemon stopped their
    /// verb process only to run the verb again under a newer build (see `server/outdated.rs`).
    held: HashSet<Handle>,
}

/// One handle in memory.
struct Entry {
    verb: String,
    /// The build tree the operation was asked from, if any.
    tree: Option<TreeId>,
    /// The session that asked for it, if any.
    session: Option<SessionId>,
    reply: Option<Reply>,
    finished_at: Option<Timestamp>,
    /// A wait has returned the reply.
    read: bool,
    /// Why the operation was started again under the same handle, such as `L-128 was restarted
    /// under ignite eda21bd`, which a wait that ends first prints.
    restarted: Option<String>,
}

impl Entry {
    fn running(verb: &str, tree: Option<TreeId>, session: Option<SessionId>) -> Entry {
        Entry {
            verb: verb.to_owned(),
            tree,
            session,
            reply: None,
            finished_at: None,
            read: false,
            restarted: None,
        }
    }
}

struct Waiter {
    session: Option<SessionId>,
    interrupt: Option<Reply>,
}

impl Handles {
    /// Handles kept in memory only, for tests and for a daemon without a database.
    pub fn new(clock: Arc<dyn Clock>) -> Handles {
        Handles {
            clock,
            db: None,
            state: Mutex::new(State {
                next: 1,
                ..State::default()
            }),
            changed: Condvar::new(),
        }
    }

    /// Handles kept in the daemon's local database. A handle the database lists as running
    /// belongs to a daemon that stopped before it finished: it stays running, and
    /// [`Handles::take_orphans`] hands it to the server, which runs its verb again or ends it
    /// `incomplete`.
    pub fn open(clock: Arc<dyn Clock>, db: Arc<Mutex<Connection>>) -> Result<Handles> {
        let mut orphans = Vec::new();
        let mut trees = Vec::new();
        for row in store::running(&lock(&db)).map_err(|e| store_error(&e))? {
            let Ok(handle) = Handle::new(row.id.as_str()) else {
                continue;
            };
            let request = row
                .reply
                .as_deref()
                .and_then(|text| serde_json::from_str::<Running>(text).ok())
                .map(|running| running.request);
            let tree = row.tree.as_deref().and_then(|t| TreeId::new(t).ok());
            let session = row.session.as_deref().and_then(|s| SessionId::new(s).ok());
            trees.push((tree, session));
            orphans.push(Orphan {
                handle,
                verb: row.verb,
                request,
            });
        }
        let highest = store::highest_number(&lock(&db)).map_err(|e| store_error(&e))?;
        let mut handles = Handles::new(clock);
        handles.db = Some(db);
        let state = handles
            .state
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner);
        // One counter numbers every verb's handles, so start above every stored number.
        state.next = highest + 1;
        for (orphan, (tree, session)) in orphans.iter().zip(trees) {
            state.replies.insert(
                orphan.handle.clone(),
                Entry::running(&orphan.verb, tree, session),
            );
        }
        state.orphans = orphans;
        Ok(handles)
    }

    /// The handles that were running when the daemon stopped; the second call returns none.
    pub fn take_orphans(&self) -> Vec<Orphan> {
        std::mem::take(&mut lock(&self.state).orphans)
    }

    /// Names a running operation: the verb's first letter in capitals and a number, such as
    /// `L-12` for `land`.
    pub fn create(&self, verb: &str, session: Option<&SessionId>, tree: Option<&TreeId>) -> Handle {
        self.create_for(verb, session, tree, None)
    }

    /// [`Handles::create`], recording `request` with the handle so that a restarted daemon can
    /// run it again.
    pub fn create_for(
        &self,
        verb: &str,
        session: Option<&SessionId>,
        tree: Option<&TreeId>,
        request: Option<&Request>,
    ) -> Handle {
        let prefix = verb
            .chars()
            .find(char::is_ascii_alphanumeric)
            .map_or('H', |c| c.to_ascii_uppercase());
        let mut state = lock(&self.state);
        let handle = loop {
            let id = format!("{prefix}-{}", state.next);
            state.next += 1;
            let handle = Handle::new(id).expect("a letter, a dash and a number make a handle");
            if !state.replies.contains_key(&handle) && self.stored(&handle).is_none() {
                break handle;
            }
        };
        self.prune(&mut state);
        state.replies.insert(
            handle.clone(),
            Entry::running(verb, tree.cloned(), session.cloned()),
        );
        self.save(&rows::Handle {
            id: handle.to_string(),
            verb: verb.to_owned(),
            session: session.map(ToString::to_string),
            tree: tree.map(ToString::to_string),
            state: "running".to_owned(),
            exit: None,
            reply: request.and_then(|request| {
                serde_json::to_string(&Running {
                    request: request.clone(),
                })
                .ok()
            }),
            started_at: self.clock.now().to_string(),
            finished_at: None,
            pid: None,
            executable: None,
            protocol: None,
        });
        handle
    }

    /// The operations still running that were asked from `tree`, with their verbs, by handle.
    /// A handle a restarted daemon found running counts until it is resumed and finishes, or
    /// is ended `incomplete`.
    pub fn running_in(&self, tree: &TreeId) -> Vec<(Handle, String)> {
        let state = lock(&self.state);
        let mut running: Vec<(Handle, String)> = state
            .replies
            .iter()
            .filter(|(_, entry)| entry.reply.is_none() && entry.tree.as_ref() == Some(tree))
            .map(|(handle, entry)| (handle.clone(), entry.verb.clone()))
            .collect();
        running.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
        running
    }

    /// Whether an operation that `session` asked for is still running, such as a `land` whose
    /// caller's wait ended and that now runs under its handle.
    pub fn running_for(&self, session: &SessionId) -> bool {
        lock(&self.state)
            .replies
            .values()
            .any(|entry| entry.reply.is_none() && entry.session.as_ref() == Some(session))
    }

    /// Records the reply of a finished operation and wakes everyone waiting on it.
    pub fn finish(&self, handle: &Handle, reply: Reply) {
        self.finish_as(handle, reply, "done");
    }

    /// [`Handles::finish`] for an operation that `ignite stop`, `ignite close` or `ignite
    /// release` stopped: the database keeps it as `stopped`.
    pub fn finish_stopped(&self, handle: &Handle, reply: Reply) {
        self.finish_as(handle, reply, "stopped");
    }

    /// [`Handles::finish`] for an operation the daemon stopped in the middle of and could not run
    /// again: the database keeps it as `incomplete`.
    pub fn finish_incomplete(&self, handle: &Handle, reply: Reply) {
        self.finish_as(handle, reply, "incomplete");
    }

    /// Records that the caller got the operation's reply straight from the verb, so memory need
    /// not keep it for a wait (the database still does).
    pub fn delivered(&self, handle: &Handle) {
        let mut state = lock(&self.state);
        if let Some(entry) = state.replies.get_mut(handle) {
            entry.read = true;
        }
        self.prune(&mut state);
    }

    /// Keeps `handle` running for its waiters when its verb process ends `stopped`: the daemon
    /// asked it to stop only to run the verb again (see `server/outdated.rs`). `note` is what a
    /// wait that ends first says happened. [`Handles::release_hold`] undoes it.
    pub fn hold_stopped(&self, handle: &Handle, note: &str) {
        let mut state = lock(&self.state);
        state.held.insert(handle.clone());
        if let Some(entry) = state.replies.get_mut(handle) {
            entry.restarted = Some(note.to_owned());
        }
    }

    /// Ends [`Handles::hold_stopped`]: a later `stopped` reply reaches the waiters again. With
    /// `keep_note` false the note is dropped too, as when the verb process refused to stop.
    pub fn release_hold(&self, handle: &Handle, keep_note: bool) {
        let mut state = lock(&self.state);
        state.held.remove(handle);
        if !keep_note && let Some(entry) = state.replies.get_mut(handle) {
            entry.restarted = None;
        }
    }

    /// Makes `handle`'s row `running` again with `request`, as before its first run started, for
    /// a run again under the same handle after its verb process ended `stopped` while held (see
    /// [`Handles::hold_stopped`]). The row no longer names a verb process.
    pub fn reopen(&self, handle: &Handle, request: &Request) {
        let state = lock(&self.state);
        if let Some(mut row) = self.stored(handle) {
            "running".clone_into(&mut row.state);
            row.exit = None;
            row.reply = serde_json::to_string(&Running {
                request: request.clone(),
            })
            .ok();
            row.finished_at = None;
            row.pid = None;
            row.executable = None;
            row.protocol = None;
            self.save(&row);
        }
        drop(state);
    }

    /// Why the running operation of `handle` was started again, if it was.
    pub fn restarted(&self, handle: &Handle) -> Option<String> {
        lock(&self.state)
            .replies
            .get(handle)
            .and_then(|entry| entry.restarted.clone())
    }

    fn finish_as(&self, handle: &Handle, reply: Reply, how: &str) {
        let mut state = lock(&self.state);
        if how == "stopped" && state.held.contains(handle) {
            return;
        }
        if let Some(mut row) = self.stored(handle) {
            how.clone_into(&mut row.state);
            row.exit = Some(i64::from(reply.exit.code()));
            row.reply = serde_json::to_string(&reply).ok();
            row.finished_at = Some(self.clock.now().to_string());
            self.save(&row);
        }
        if let Some(entry) = state.replies.get_mut(handle) {
            entry.reply = Some(reply);
            entry.finished_at = Some(self.clock.now());
        }
        self.prune(&mut state);
        drop(state);
        self.changed.notify_all();
    }

    /// Waits at most `wait_s` seconds for the operation to finish. `session` is the session
    /// waiting, which [`Handles::interrupt`] names.
    pub fn wait(&self, handle: &Handle, wait_s: u32, session: Option<&SessionId>) -> Waited {
        let mut state = lock(&self.state);
        let id = state.next_waiter;
        state.next_waiter += 1;
        state.waiters.insert(
            id,
            Waiter {
                session: session.cloned(),
                interrupt: None,
            },
        );
        let deadline = Instant::now() + Duration::from_secs(u64::from(wait_s));
        let waited = loop {
            if let Some(reply) = state.waiters.get_mut(&id).and_then(|w| w.interrupt.take()) {
                break Waited::Interrupted(reply);
            }
            // A handle pruned from memory while this wait slept is read again from the database.
            if !state.replies.contains_key(handle) {
                let Some(row) = self.stored(handle) else {
                    break Waited::Unknown;
                };
                // A running row's `reply` holds its request, not a reply.
                let reply = row
                    .reply
                    .filter(|_| row.state != "running")
                    .and_then(|text| serde_json::from_str(&text).ok());
                let finished_at = reply.as_ref().map(|_| self.clock.now());
                state.replies.insert(
                    handle.clone(),
                    Entry {
                        verb: row.verb,
                        tree: row.tree.as_deref().and_then(|t| TreeId::new(t).ok()),
                        session: row.session.as_deref().and_then(|s| SessionId::new(s).ok()),
                        reply,
                        finished_at,
                        read: false,
                        restarted: None,
                    },
                );
            }
            let entry = state
                .replies
                .get_mut(handle)
                .expect("the handle was just inserted");
            if let Some(reply) = entry.reply.clone() {
                entry.read = true;
                break Waited::Done(reply);
            }
            let verb = entry.verb.clone();
            let now = Instant::now();
            if now >= deadline {
                break Waited::Running(verb);
            }
            state = self
                .changed
                .wait_timeout(state, deadline - now)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        };
        state.waiters.remove(&id);
        waited
    }

    /// Drops finished replies from memory: those older than [`KEEP_REPLY_S`], and those a wait
    /// has read when the database still has them. A handle dropped here is read from the
    /// database by the next wait on it.
    fn prune(&self, state: &mut State) {
        let now = self.clock.now();
        let stored = self.db.is_some();
        state.replies.retain(|_, entry| {
            let Some(finished) = entry.finished_at else {
                return true;
            };
            let old = now.seconds_since(finished) > KEEP_REPLY_S;
            !(old || (entry.read && stored))
        });
    }

    /// How many waits `session` is in now.
    pub fn waits_of(&self, session: &SessionId) -> usize {
        let state = lock(&self.state);
        state
            .waiters
            .values()
            .filter(|w| w.session.as_ref() == Some(session))
            .count()
    }

    /// Ends every wait `session` is in with `reply`. Returns how many waits it ended.
    pub fn interrupt(&self, session: &SessionId, reply: &Reply) -> usize {
        let mut state = lock(&self.state);
        let mut ended = 0;
        for waiter in state.waiters.values_mut() {
            if waiter.session.as_ref() == Some(session) {
                waiter.interrupt = Some(reply.clone());
                ended += 1;
            }
        }
        drop(state);
        self.changed.notify_all();
        ended
    }

    fn stored(&self, handle: &Handle) -> Option<rows::Handle> {
        let db = self.db.as_ref()?;
        store::get(&lock(db), handle.as_str()).ok()?
    }

    fn save(&self, row: &rows::Handle) {
        if let Some(db) = &self.db
            && let Err(error) = store::upsert(&lock(db), row)
        {
            // The handle still works from memory; only a restart would forget it.
            eprintln!("ignite: could not save handle {}: {error}", row.id);
        }
    }
}

fn store_error(error: &ignite_store::StoreError) -> Error {
    Error::Io(error.to_string())
}

/// The verb and the request of a handle that is still running, as a verb process reads them
/// from the handle's row when it starts (see `crate::verbproc`). `None` when the handle is
/// unknown, has finished, or has no request recorded.
pub fn running_request(db: &Connection, handle: &Handle) -> Result<Option<(String, Request)>> {
    let Some(row) = store::get(db, handle.as_str()).map_err(|e| store_error(&e))? else {
        return Ok(None);
    };
    if row.state != "running" {
        return Ok(None);
    }
    Ok(row
        .reply
        .as_deref()
        .and_then(|text| serde_json::from_str::<Running>(text).ok())
        .map(|running| (row.verb, running.request)))
}

/// Writes the reply of a verb process's verb into its running handle's row, in one transaction:
/// `stopped` when it was stopped, otherwise `done`. The daemon hands it to whoever waits once
/// the process says it has finished. False when the row was no longer running.
pub fn finish_in_row(
    db: &Connection,
    handle: &Handle,
    reply: &Reply,
    stopped: bool,
    now: Timestamp,
) -> Result<bool> {
    let text = serde_json::to_string(reply).map_err(|e| Error::Invalid(e.to_string()))?;
    let ending = store::Ending {
        state: if stopped { "stopped" } else { "done" },
        exit: i64::from(reply.exit.code()),
        reply: &text,
        finished_at: &now.to_string(),
    };
    store::finish_running(db, handle.as_str(), &ending).map_err(|e| store_error(&e))
}

/// The reply of an operation the daemon stopped in the middle of and could not run again, for
/// `reason`.
pub fn incomplete(verb: &str, handle: &Handle, reason: &str) -> Reply {
    Reply {
        exit: Exit::Conflict,
        text: format!(
            "incomplete the daemon restarted while `ignite {verb}` ({handle}) ran, and {reason}\n\
             next     ignite status shows what it left; run it again if it is still needed"
        ),
        json: Some(serde_json::json!({
            "verb": verb,
            "handle": handle,
            "incomplete": true,
            "reason": reason,
        })),
        handle: None,
    }
}

/// `ignite wait <handle>`: blocks until a handle's operation finishes, for at most the
/// request's `wait_s`, then answers with the operation's reply. A `deploy-<commit>` handle,
/// which `ignite land` prints when it moves the release branch, is answered by the daemon's
/// deploy waiter instead (see [`crate::deploy`]).
pub struct WaitVerb {
    handles: Arc<Handles>,
    steps: Arc<Steps>,
    deploys: crate::deploy::Slot,
}

impl WaitVerb {
    /// `steps` holds what each running operation is doing, which a wait that ends first prints.
    pub fn new(handles: Arc<Handles>, steps: Arc<Steps>) -> WaitVerb {
        WaitVerb {
            handles,
            steps,
            deploys: crate::deploy::slot(),
        }
    }

    /// The verb with deploy handles answered by the waiter in `deploys` instead of the daemon's.
    #[must_use]
    pub fn with_deploys(self, deploys: crate::deploy::Slot) -> WaitVerb {
        WaitVerb { deploys, ..self }
    }
}

impl Verb for WaitVerb {
    fn name(&self) -> &'static str {
        "wait"
    }

    fn keeps_its_own_wait(&self) -> bool {
        true
    }

    fn needs_the_tree_lease(&self) -> bool {
        false
    }

    fn blocks_update(&self) -> bool {
        false
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        self.run_streaming(ctx, req, None)
    }

    /// Waits on another thread, so that an interrupt reaches the wait at once, and passes the
    /// operation's steps to `progress` meanwhile.
    fn run_streaming(
        &self,
        ctx: &Context,
        req: &Request,
        progress: Option<&mut dyn FnMut(&str) -> bool>,
    ) -> Reply {
        let [name] = req.args.as_slice() else {
            return Reply::text(Exit::Usage, "usage    ignite wait <handle>");
        };
        if let Some(target) = crate::deploy::parse_handle(name) {
            let Some(waiter) = self.deploys.get() else {
                return Reply::text(
                    Exit::Usage,
                    format!("failed   this daemon does not follow releases, so {name} is unknown"),
                );
            };
            let probe = crate::server::caller_probe();
            let left = move || probe.as_ref().is_some_and(|gone| gone());
            return waiter.wait(&target, req.wait_s, progress, &left);
        }
        let Ok(handle) = Handle::new(name.as_str()) else {
            return Reply::text(Exit::Usage, format!("unknown handle: {name}"));
        };
        let Some(progress) = progress else {
            let waited = self.handles.wait(&handle, req.wait_s, ctx.session.as_ref());
            return self.answer(name, &handle, waited);
        };
        let (sender, receiver) = std::sync::mpsc::channel();
        {
            let (handles, waited, session) =
                (self.handles.clone(), handle.clone(), ctx.session.clone());
            let wait_s = req.wait_s;
            let spawned = std::thread::Builder::new()
                .name(format!("ignite-wait-{handle}"))
                .spawn(move || {
                    let _ = sender.send(handles.wait(&waited, wait_s, session.as_ref()));
                });
            if let Err(error) = spawned {
                return Reply::text(
                    Exit::Usage,
                    format!("failed   cannot start a thread: {error}"),
                );
            }
        }
        let mut feed = StepFeed::new(self.steps.clone(), handle.clone());
        let mut open = true;
        loop {
            let lines = feed.next(FEED_POLL);
            for line in lines {
                open = open && progress(&line);
            }
            match receiver.try_recv() {
                Ok(waited) => return self.answer(name, &handle, waited),
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return Reply::text(Exit::Usage, format!("failed   the wait on {name} ended"));
                }
            }
        }
    }
}

/// How often a wait that passes on steps looks whether the operation has finished.
const FEED_POLL: Duration = Duration::from_millis(100);

impl WaitVerb {
    fn answer(&self, name: &str, handle: &Handle, waited: Waited) -> Reply {
        match waited {
            Waited::Done(reply) | Waited::Interrupted(reply) => reply,
            Waited::Running(verb) => {
                let doing = self.steps.by_handle(handle).map(|a| a.view());
                let mut reply = still_running_at(&verb, handle, doing.as_ref());
                if let Some(note) = self.handles.restarted(handle) {
                    reply.text = format!("restart  {note}\n{}", reply.text);
                    if let Some(json) = reply.json.as_mut().and_then(|j| j.as_object_mut()) {
                        json.insert("restarted".to_owned(), serde_json::json!(note));
                    }
                }
                reply
            }
            Waited::Unknown => Reply::text(Exit::Usage, format!("unknown handle: {name}")),
        }
    }
}

/// Each step of the operation running under a handle, as it starts, read from whichever activity
/// the steps registry lists for the handle: the one of a verb on a thread of the daemon, or the
/// one the daemon keeps for a verb process (see [`crate::host`]). The activity is looked up
/// again once it ends, as when a verb whose process died is run again by the daemon.
pub(crate) struct StepFeed {
    steps: Arc<Steps>,
    handle: Handle,
    current: Option<(Arc<Activity>, Receiver<String>)>,
}

impl StepFeed {
    pub(crate) fn new(steps: Arc<Steps>, handle: Handle) -> StepFeed {
        StepFeed {
            steps,
            handle,
            current: None,
        }
    }

    /// The steps heard within `timeout`: it returns at the first one, with any that came with it.
    /// The first call that finds the activity starts with its step now, if it has one, so a
    /// client that comes in the middle, as after a restart, sees where the work is at once.
    pub(crate) fn next(&mut self, timeout: Duration) -> Vec<String> {
        if self
            .current
            .as_ref()
            .is_none_or(|(activity, _)| activity.is_finished())
        {
            self.current = self.steps.by_handle(&self.handle).map(|activity| {
                let heard = activity.subscribe();
                (activity, heard)
            });
            if let Some(step) = self.current.as_ref().and_then(|(a, _)| a.view().step) {
                return vec![step];
            }
        }
        let Some((_, heard)) = &self.current else {
            std::thread::sleep(timeout);
            return Vec::new();
        };
        match heard.recv_timeout(timeout) {
            Ok(line) => {
                let mut lines = vec![line];
                lines.extend(heard.try_iter());
                lines
            }
            Err(RecvTimeoutError::Timeout) => Vec::new(),
            Err(RecvTimeoutError::Disconnected) => {
                self.current = None;
                Vec::new()
            }
        }
    }
}

#[cfg(test)]
#[path = "handles_tests.rs"]
mod tests;
