//! Handles: an operation that would outlive the client's wait answers with a short name, such as
//! `L-5521`, and `ignite wait <handle>` blocks until it finishes.
//!
//! Each handle is kept in the local database as well as in memory, so a restarted daemon still
//! answers `wait` for a handle from before the restart. An operation that was still running when
//! the daemon stopped cannot finish any more: its handle answers that it was lost.
//!
//! A session inside `ignite wait` can be interrupted: [`Handles::interrupt`] ends every wait of
//! that session at once with the reply it is given, such as a message from a person.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use rusqlite::Connection;

use ignite_core::ids::{Handle, SessionId, TreeId};
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::time::Timestamp;
use ignite_core::traits::{Clock, Error, Result};
use ignite_store::local::{handles as store, rows};

use crate::server::{Context, Verb, still_running};

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
}

/// One handle in memory.
struct Entry {
    verb: String,
    reply: Option<Reply>,
    finished_at: Option<Timestamp>,
    /// A wait has returned the reply.
    read: bool,
}

impl Entry {
    fn running(verb: &str) -> Entry {
        Entry {
            verb: verb.to_owned(),
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
    /// belongs to a daemon that stopped before it finished, so it is marked lost here.
    pub fn open(clock: Arc<dyn Clock>, db: Arc<Mutex<Connection>>) -> Result<Handles> {
        {
            let conn = lock(&db);
            let now = clock.now().to_string();
            for mut row in store::running(&conn).map_err(|e| store_error(&e))? {
                let reply = lost(&row.verb, &row.id);
                "lost".clone_into(&mut row.state);
                row.exit = Some(i64::from(reply.exit.code()));
                row.reply = Some(serde_json::to_string(&reply).map_err(|e| json_error(&e))?);
                row.finished_at = Some(now.clone());
                store::upsert(&conn, &row).map_err(|e| store_error(&e))?;
            }
        }
        let mut handles = Handles::new(clock);
        handles.db = Some(db);
        Ok(handles)
    }

    /// Names a running operation: the verb's first letter in capitals and a number, such as
    /// `L-12` for `land`.
    pub fn create(&self, verb: &str, session: Option<&SessionId>, tree: Option<&TreeId>) -> Handle {
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
        state.replies.insert(handle.clone(), Entry::running(verb));
        self.save(&rows::Handle {
            id: handle.to_string(),
            verb: verb.to_owned(),
            session: session.map(ToString::to_string),
            tree: tree.map(ToString::to_string),
            state: "running".to_owned(),
            exit: None,
            reply: None,
            started_at: self.clock.now().to_string(),
            finished_at: None,
        });
        handle
    }

    /// Records the reply of a finished operation and wakes everyone waiting on it.
    pub fn finish(&self, handle: &Handle, reply: Reply) {
        let mut state = lock(&self.state);
        if let Some(mut row) = self.stored(handle) {
            "done".clone_into(&mut row.state);
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
                let reply = row.reply.and_then(|text| serde_json::from_str(&text).ok());
                let finished_at = reply.as_ref().map(|_| self.clock.now());
                state.replies.insert(
                    handle.clone(),
                    Entry {
                        verb: row.verb,
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

fn json_error(error: &serde_json::Error) -> Error {
    Error::Io(error.to_string())
}

/// The reply of an operation the daemon stopped in the middle of.
fn lost(verb: &str, handle: &str) -> Reply {
    Reply::text(
        Exit::Conflict,
        format!("lost     the daemon restarted while `ignite {verb}` ({handle}) ran; run it again"),
    )
}

/// `ignite wait <handle>`: blocks until a handle's operation finishes, for at most the
/// request's `wait_s`, then answers with the operation's reply.
pub struct WaitVerb {
    handles: Arc<Handles>,
}

impl WaitVerb {
    pub fn new(handles: Arc<Handles>) -> WaitVerb {
        WaitVerb { handles }
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
            Waited::Running(verb) => still_running(&verb, &handle),
            Waited::Unknown => Reply::text(Exit::Usage, format!("unknown handle: {name}")),
        }
    }
}

#[cfg(test)]
#[path = "handles_tests.rs"]
mod tests;
