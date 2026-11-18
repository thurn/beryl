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
//! A session inside `ignite wait` can be interrupted: [`Handles::interrupt`] ends every wait of
//! that session at once with the reply it is given, such as a message from a person.

use std::collections::{BTreeMap, HashMap};
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
use crate::steps::Steps;

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
}

/// One handle in memory.
struct Entry {
    verb: String,
    /// The build tree the operation was asked from, if any.
    tree: Option<TreeId>,
    reply: Option<Reply>,
    finished_at: Option<Timestamp>,
    /// A wait has returned the reply.
    read: bool,
}

impl Entry {
    fn running(verb: &str, tree: Option<TreeId>) -> Entry {
        Entry {
            verb: verb.to_owned(),
            tree,
            reply: None,
            finished_at: None,
            read: false,
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
            trees.push(tree);
            orphans.push(Orphan {
                handle,
                verb: row.verb,
                request,
            });
        }
        let mut handles = Handles::new(clock);
        handles.db = Some(db);
        let state = handles
            .state
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner);
        for (orphan, tree) in orphans.iter().zip(trees) {
            state
                .replies
                .insert(orphan.handle.clone(), Entry::running(&orphan.verb, tree));
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
        state
            .replies
            .insert(handle.clone(), Entry::running(verb, tree.cloned()));
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

    fn finish_as(&self, handle: &Handle, reply: Reply, how: &str) {
        let mut state = lock(&self.state);
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
                        reply,
                        finished_at,
                        read: false,
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
/// request's `wait_s`, then answers with the operation's reply.
pub struct WaitVerb {
    handles: Arc<Handles>,
    steps: Arc<Steps>,
}

impl WaitVerb {
    /// `steps` holds what each running operation is doing, which a wait that ends first prints.
    pub fn new(handles: Arc<Handles>, steps: Arc<Steps>) -> WaitVerb {
        WaitVerb { handles, steps }
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
        let [name] = req.args.as_slice() else {
            return Reply::text(Exit::Usage, "usage    ignite wait <handle>");
        };
        let Ok(handle) = Handle::new(name.as_str()) else {
            return Reply::text(Exit::Usage, format!("unknown handle: {name}"));
        };
        match self.handles.wait(&handle, req.wait_s, ctx.session.as_ref()) {
            Waited::Done(reply) | Waited::Interrupted(reply) => reply,
            Waited::Running(verb) => {
                let doing = self.steps.by_handle(&handle).map(|a| a.view());
                still_running_at(&verb, &handle, doing.as_ref())
            }
            Waited::Unknown => Reply::text(Exit::Usage, format!("unknown handle: {name}")),
        }
    }
}

#[cfg(test)]
#[path = "handles_tests.rs"]
mod tests;
