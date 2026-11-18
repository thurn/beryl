//! Steps: what each running command and background loop is doing now, and how a running
//! command is asked to stop.
//!
//! `ignite wait` prints the step of the handle it waits on, `ignite land` prints each step as it
//! starts, `ignite status` lists every running command and loop with its step, and `ignite
//! doctor` names what a slow loop waits for. `ignite stop`, `ignite close` and `ignite release`
//! ask a command to stop through the same record.
//!
//! # The API
//!
//! An [`Activity`] is one piece of work: a verb the server runs, or one round of a background
//! loop. Whoever starts the work creates it with [`Steps::begin`] and enters it on the thread
//! that does the work with [`Activity::enter`], which returns a guard. Code anywhere below,
//! however deep, then calls the free functions of this module. They act on the activity entered
//! on the calling thread, and do nothing on a thread that entered none, so no parameter has to
//! be passed down for them:
//!
//! - [`step`]`("listing family rust")` says what the work does now. It replaces the previous
//!   step, and the step's age starts again.
//! - [`during`]`("building rust-tests in tree-04")` sets a step for as long as the guard it
//!   returns lives, and then puts the previous step back, with its age. Use it for a part of a
//!   larger step, such as a build inside a listing.
//! - [`waiting`]`("8 cores")` returns a guard. Until the guard is dropped, the work is shown as
//!   waiting for that thing. Waits nest, and the innermost one is shown.
//! - [`class`], [`ticket`] and [`session`] say whom the work is for. A build started anywhere
//!   below a landing runs in the `land` class and is charged to the landing's ticket.
//!   [`set_class`] and [`set_ticket`] record them for a verb that learns them after it starts.
//! - [`stop_requested`] turns true once someone has asked the work to stop. The launcher ends
//!   the commands of such an activity and refuses to start new ones; long waits give up.
//! - [`refuse_stop`]`(reason)` makes every later request to stop fail with `reason`, as for a
//!   landing whose push has begun. It returns false if a stop was already asked for, so the
//!   caller can give up instead of starting what may not be interrupted.
//! - [`allow_stop`] lifts that refusal once the part that may not be interrupted is over, as
//!   when the remote rejected a landing's push and the landing goes back to rebase.
//! - [`critical`]`()` returns a guard. Until it is dropped, the work is in a part that may not
//!   be cut short, such as a push, and a ready build waits to restart the daemon (see
//!   `main/selfupdate.rs`). Every other moment, the daemon may restart under the work, which a
//!   restarted daemon then runs again (see `server/resume.rs`).
//! - [`current`] returns the entered activity, so that a thread spawned for part of the work can
//!   enter it as well:
//!
//! ```ignore
//! let scope = steps::current();
//! thread::spawn(move || {
//!     let _entered = scope.as_ref().map(|activity| activity.enter());
//!     steps::step("listing family rust");
//! });
//! ```
//!
//! The server's verbs live in the daemon's registry, [`global`]. A test thread gets a registry
//! of its own from [`global`], so that tests running at once do not see each other's work.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};
use std::time::{Duration, Instant};

use serde::Serialize;

use ignite_core::ids::{Handle, SessionId, TicketId, TreeId};
use ignite_core::protocol::Class;
use ignite_core::time::Timestamp;
use ignite_core::traits::Clock;

use crate::loop_health::span;

/// What kind of work an activity is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A verb the server runs, such as `land`.
    Verb(String),
    /// A round of the background loop of this name, such as `ignite-idle`.
    Loop(String),
}

/// Whom an activity works for, as far as known when it begins.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Work {
    pub class: Option<Class>,
    pub ticket: Option<TicketId>,
    pub session: Option<SessionId>,
    pub tree: Option<TreeId>,
    /// The tree's directory. Two daemons in one process (as in tests) can each have a tree of
    /// the same name; the directory tells them apart.
    pub tree_path: Option<PathBuf>,
}

/// Whether an activity may still be stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Stop {
    Running,
    Asked,
    /// Stopping is refused from now on, for this reason.
    Refused(String),
}

struct State {
    work: Work,
    handle: Option<Handle>,
    step: Option<(String, Timestamp)>,
    /// What the work waits for, innermost last, each with its own number and start.
    waits: Vec<(u64, String, Timestamp)>,
    next_wait: u64,
    stop: Stop,
    /// How many [`critical`] guards are held.
    critical: u32,
    finished: bool,
    /// Receivers of each step and wait as it starts, such as the client of `ignite land`.
    observers: Vec<Sender<String>>,
}

/// One running piece of work. See the module documentation.
pub struct Activity {
    kind: Kind,
    clock: Arc<dyn Clock>,
    started: Timestamp,
    state: Mutex<State>,
    changed: Condvar,
}

/// What an activity is doing, for `status`, `wait` and `doctor`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct View {
    /// `verb` or `loop`.
    pub kind: &'static str,
    /// The verb or the loop's name.
    pub name: String,
    pub handle: Option<String>,
    pub ticket: Option<String>,
    pub session: Option<String>,
    pub tree: Option<String>,
    pub class: Option<&'static str>,
    pub age_s: f64,
    pub step: Option<String>,
    pub step_age_s: Option<f64>,
    pub waiting_for: Option<String>,
    pub waiting_s: Option<f64>,
    /// A stop has been asked for and the work has not ended yet.
    pub stopping: bool,
}

impl View {
    /// The step and what the work waits for, with how long each has lasted, such as
    /// `building rust-tests in landing-3 (4 min), waiting for 8 cores (3 min)`.
    pub fn doing(&self) -> String {
        let mut text = match (&self.step, self.step_age_s) {
            (Some(step), Some(age)) => format!("{step} ({})", span(age)),
            _ => "no step recorded".to_owned(),
        };
        if let (Some(what), Some(age)) = (&self.waiting_for, self.waiting_s) {
            let _ = write!(text, ", waiting for {what} ({})", span(age));
        }
        if self.stopping {
            text.push_str(", stopping");
        }
        text
    }

    /// The line `ignite status` shows for a running command.
    pub fn line(&self) -> String {
        let mut words = vec![self.handle.clone().unwrap_or_else(|| "-".to_owned())];
        words.push(self.name.clone());
        words.extend(self.ticket.clone());
        words.extend(self.tree.clone());
        words.extend(self.class.map(str::to_owned));
        format!(
            "{} for {}: {}",
            words.join(" "),
            span(self.age_s),
            self.doing()
        )
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Activity {
    fn new(kind: Kind, work: Work, clock: Arc<dyn Clock>) -> Activity {
        Activity {
            kind,
            started: clock.now(),
            clock,
            state: Mutex::new(State {
                work,
                handle: None,
                step: None,
                waits: Vec::new(),
                next_wait: 0,
                stop: Stop::Running,
                critical: 0,
                finished: false,
                observers: Vec::new(),
            }),
            changed: Condvar::new(),
        }
    }

    pub fn kind(&self) -> &Kind {
        &self.kind
    }

    /// Makes this the activity of the calling thread until the guard is dropped.
    pub fn enter(self: &Arc<Self>) -> Entered {
        CURRENT.with(|current| current.borrow_mut().push(self.clone()));
        Entered {
            activity: self.clone(),
        }
    }

    /// Records the end of the work and wakes everyone waiting for it.
    pub fn finish(&self) {
        let mut state = lock(&self.state);
        state.finished = true;
        state.observers.clear();
        drop(state);
        self.changed.notify_all();
    }

    pub fn is_finished(&self) -> bool {
        lock(&self.state).finished
    }

    /// Blocks until the work has finished, for at most `timeout`. Returns whether it finished.
    pub fn wait_finished(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut state = lock(&self.state);
        loop {
            if state.finished {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            state = self
                .changed
                .wait_timeout(state, deadline - now)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Asks the work to stop. `Err` holds the reason it may not be stopped.
    pub fn request_stop(&self) -> Result<(), String> {
        let mut state = lock(&self.state);
        match &state.stop {
            Stop::Refused(reason) => Err(reason.clone()),
            Stop::Asked | Stop::Running => {
                state.stop = Stop::Asked;
                drop(state);
                self.changed.notify_all();
                Ok(())
            }
        }
    }

    /// Blocks until someone asks the work to stop, for at most `timeout`. Returns whether they
    /// did.
    pub fn wait_stop_requested(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut state = lock(&self.state);
        loop {
            if state.stop == Stop::Asked {
                return true;
            }
            let Some(left) = deadline.checked_duration_since(Instant::now()) else {
                return false;
            };
            state = self
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    pub fn stop_requested(&self) -> bool {
        lock(&self.state).stop == Stop::Asked
    }

    /// See [`refuse_stop`].
    pub fn refuse_stop(&self, reason: &str) -> bool {
        let mut state = lock(&self.state);
        if state.stop == Stop::Asked {
            return false;
        }
        state.stop = Stop::Refused(reason.to_owned());
        true
    }

    /// Whether the work is in a part that may not be cut short (see [`critical`]).
    pub fn in_critical(&self) -> bool {
        lock(&self.state).critical > 0
    }

    /// See [`allow_stop`].
    pub fn allow_stop(&self) {
        let mut state = lock(&self.state);
        if matches!(state.stop, Stop::Refused(_)) {
            state.stop = Stop::Running;
        }
    }

    /// Whether stopping is refused now (see [`refuse_stop`]): the work is in a part that may
    /// not be interrupted, such as a push.
    pub fn refusing_stop(&self) -> bool {
        matches!(lock(&self.state).stop, Stop::Refused(_))
    }

    /// Names the handle the work's reply goes to: given before a verb that does work starts,
    /// or once its caller stopped waiting.
    pub fn attach_handle(&self, handle: &Handle) {
        lock(&self.state).handle = Some(handle.clone());
    }

    pub fn handle(&self) -> Option<Handle> {
        lock(&self.state).handle.clone()
    }

    pub fn work(&self) -> Work {
        lock(&self.state).work.clone()
    }

    /// A receiver of each step and wait as it starts, until the work finishes.
    pub fn subscribe(&self) -> Receiver<String> {
        let (sender, receiver) = channel();
        let mut state = lock(&self.state);
        if !state.finished {
            state.observers.push(sender);
        }
        receiver
    }

    /// Ends every subscription, as when the client that read them has gone.
    pub fn unsubscribe_all(&self) {
        lock(&self.state).observers.clear();
    }

    pub fn set_step(&self, text: &str) {
        let mut state = lock(&self.state);
        state.step = Some((text.to_owned(), self.clock.now()));
        state.observers.retain(|o| o.send(text.to_owned()).is_ok());
    }

    fn swap_step(&self, step: Option<(String, Timestamp)>) -> Option<(String, Timestamp)> {
        let mut state = lock(&self.state);
        if let Some((text, _)) = &step {
            state.observers.retain(|o| o.send(text.clone()).is_ok());
        }
        std::mem::replace(&mut state.step, step)
    }

    fn push_wait(&self, what: &str) -> u64 {
        let mut state = lock(&self.state);
        let id = state.next_wait;
        state.next_wait += 1;
        state.waits.push((id, what.to_owned(), self.clock.now()));
        let line = format!("waiting for {what}");
        state.observers.retain(|o| o.send(line.clone()).is_ok());
        id
    }

    fn pop_wait(&self, id: u64) {
        lock(&self.state).waits.retain(|(n, _, _)| *n != id);
    }

    fn update(&self, change: impl FnOnce(&mut Work)) {
        change(&mut lock(&self.state).work);
    }

    /// What the work is doing now.
    pub fn view(&self) -> View {
        let now = self.clock.now();
        let state = lock(&self.state);
        let (kind, name) = match &self.kind {
            Kind::Verb(verb) => ("verb", verb.clone()),
            Kind::Loop(name) => ("loop", name.clone()),
        };
        let wait = state.waits.last();
        View {
            kind,
            name,
            handle: state.handle.as_ref().map(ToString::to_string),
            ticket: state.work.ticket.as_ref().map(ToString::to_string),
            session: state.work.session.as_ref().map(ToString::to_string),
            tree: state.work.tree.as_ref().map(ToString::to_string),
            class: state.work.class.map(Class::as_str),
            age_s: now.seconds_since(self.started),
            step: state.step.as_ref().map(|(text, _)| text.clone()),
            step_age_s: state.step.as_ref().map(|(_, at)| now.seconds_since(*at)),
            waiting_for: wait.map(|(_, what, _)| what.clone()),
            waiting_s: wait.map(|(_, _, at)| now.seconds_since(*at)),
            stopping: state.stop == Stop::Asked && !state.finished,
        }
    }
}

/// The guard [`Activity::enter`] returns.
pub struct Entered {
    activity: Arc<Activity>,
}

impl Drop for Entered {
    fn drop(&mut self) {
        CURRENT.with(|current| {
            let mut current = current.borrow_mut();
            if let Some(at) = current.iter().rposition(|a| Arc::ptr_eq(a, &self.activity)) {
                current.remove(at);
            }
        });
    }
}

/// The guard [`during`] returns.
pub struct During {
    held: Option<(Arc<Activity>, Option<StepAt>)>,
}

/// A step and when it started.
type StepAt = (String, Timestamp);

impl Drop for During {
    fn drop(&mut self) {
        if let Some((activity, previous)) = self.held.take() {
            let mut state = lock(&activity.state);
            state.step = previous;
        }
    }
}

/// The guard [`critical`] returns.
pub struct Critical {
    held: Option<Arc<Activity>>,
}

impl Drop for Critical {
    fn drop(&mut self) {
        if let Some(activity) = self.held.take() {
            let mut state = lock(&activity.state);
            state.critical = state.critical.saturating_sub(1);
        }
    }
}

/// The guard [`waiting`] returns.
pub struct Waiting {
    held: Option<(Arc<Activity>, u64)>,
}

impl Drop for Waiting {
    fn drop(&mut self) {
        if let Some((activity, id)) = self.held.take() {
            activity.pop_wait(id);
        }
    }
}

/// The activities of one daemon.
pub struct Steps {
    clock: Arc<dyn Clock>,
    live: Mutex<BTreeMap<u64, Weak<Activity>>>,
    next: AtomicU64,
}

impl Steps {
    pub fn new(clock: Arc<dyn Clock>) -> Steps {
        Steps {
            clock,
            live: Mutex::new(BTreeMap::new()),
            next: AtomicU64::new(1),
        }
    }

    /// A new activity, listed until it finishes or is dropped.
    pub fn begin(&self, kind: Kind, work: Work) -> Arc<Activity> {
        let activity = Arc::new(Activity::new(kind, work, self.clock.clone()));
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let mut live = lock(&self.live);
        live.retain(|_, weak| weak.upgrade().is_some_and(|a| !a.is_finished()));
        live.insert(id, Arc::downgrade(&activity));
        activity
    }

    /// The activities that have not finished, oldest first.
    pub fn live(&self) -> Vec<Arc<Activity>> {
        lock(&self.live)
            .values()
            .filter_map(Weak::upgrade)
            .filter(|activity| !activity.is_finished())
            .collect()
    }

    /// Whether any running work is in a part that may not be cut short (see [`critical`]).
    pub fn critical_now(&self) -> bool {
        self.live().iter().any(|activity| activity.in_critical())
    }

    /// The running activity whose reply goes to `handle`.
    pub fn by_handle(&self, handle: &Handle) -> Option<Arc<Activity>> {
        self.live()
            .into_iter()
            .find(|activity| activity.handle().as_ref() == Some(handle))
    }

    /// The running verbs asked from inside `tree`, whose directory is `path`.
    pub fn in_tree(&self, tree: &TreeId, path: &Path) -> Vec<Arc<Activity>> {
        self.live()
            .into_iter()
            .filter(|activity| matches!(activity.kind, Kind::Verb(_)))
            .filter(|activity| {
                let work = activity.work();
                work.tree.as_ref() == Some(tree)
                    && work.tree_path.as_deref().is_none_or(|p| p == path)
            })
            .collect()
    }
}

#[cfg(not(test))]
static GLOBAL: std::sync::OnceLock<Arc<Steps>> = std::sync::OnceLock::new();

/// The daemon's registry, which the server's verbs are listed in.
#[cfg(not(test))]
pub fn global() -> Arc<Steps> {
    GLOBAL
        .get_or_init(|| Arc::new(Steps::new(Arc::new(ignite_core::time::SystemClock))))
        .clone()
}

#[cfg(test)]
thread_local! {
    static GLOBAL: Arc<Steps> = Arc::new(Steps::new(Arc::new(ignite_core::time::SystemClock)));
}

/// In tests, a registry of the calling thread's own.
#[cfg(test)]
pub fn global() -> Arc<Steps> {
    GLOBAL.with(Arc::clone)
}

thread_local! {
    /// The activities entered on this thread, innermost last.
    static CURRENT: RefCell<Vec<Arc<Activity>>> = const { RefCell::new(Vec::new()) };
}

/// The activity entered on the calling thread, if any.
pub fn current() -> Option<Arc<Activity>> {
    CURRENT.with(|current| current.borrow().last().cloned())
}

/// Records what the calling thread's work does now. See the module documentation.
pub fn step(text: impl AsRef<str>) {
    if let Some(activity) = current() {
        activity.set_step(text.as_ref());
    }
}

/// Records `text` as the calling thread's step until the guard is dropped, then puts the
/// previous step back. See the module documentation.
#[must_use = "the step ends when the guard is dropped"]
pub fn during(text: impl AsRef<str>) -> During {
    During {
        held: current().map(|activity| {
            let now = activity.clock.now();
            let previous = activity.swap_step(Some((text.as_ref().to_owned(), now)));
            (activity, previous)
        }),
    }
}

/// Shows the calling thread's work as waiting for `what` until the guard is dropped.
#[must_use = "the wait ends when the guard is dropped"]
pub fn waiting(what: impl AsRef<str>) -> Waiting {
    Waiting {
        held: current().map(|activity| {
            let id = activity.push_wait(what.as_ref());
            (activity, id)
        }),
    }
}

/// Marks the calling thread's work as in a part that may not be cut short, such as a push, until
/// the guard is dropped: the daemon does not restart into a new build meanwhile.
#[must_use = "the part ends when the guard is dropped"]
pub fn critical() -> Critical {
    Critical {
        held: current().inspect(|activity| lock(&activity.state).critical += 1),
    }
}

/// The class of the calling thread's work, if it has one.
pub fn class() -> Option<Class> {
    current().and_then(|activity| activity.work().class)
}

/// The ticket the calling thread's work is for, if known.
pub fn ticket() -> Option<TicketId> {
    current().and_then(|activity| activity.work().ticket)
}

/// The session the calling thread's work is for, if known.
pub fn session() -> Option<SessionId> {
    current().and_then(|activity| activity.work().session)
}

/// Records the class of the calling thread's work.
pub fn set_class(class: Class) {
    if let Some(activity) = current() {
        activity.update(|work| work.class = Some(class));
    }
}

/// Records the ticket and session the calling thread's work is for.
pub fn set_ticket(ticket: &TicketId, session: Option<&SessionId>) {
    if let Some(activity) = current() {
        activity.update(|work| {
            work.ticket = Some(ticket.clone());
            if let Some(session) = session {
                work.session = Some(session.clone());
            }
        });
    }
}

/// Whether someone has asked the calling thread's work to stop.
pub fn stop_requested() -> bool {
    current().is_some_and(|activity| activity.stop_requested())
}

/// Refuses every later request to stop the calling thread's work, with `reason`. Returns false,
/// and refuses nothing, if a stop was already asked for. True on a thread with no activity.
pub fn refuse_stop(reason: &str) -> bool {
    current().is_none_or(|activity| activity.refuse_stop(reason))
}

/// Lets the calling thread's work be stopped again after [`refuse_stop`], once the part that
/// may not be interrupted is over. Does nothing on a thread with no activity, or to work that
/// refused nothing.
pub fn allow_stop() {
    if let Some(activity) = current() {
        activity.allow_stop();
    }
}

#[cfg(test)]
#[path = "steps_tests.rs"]
mod tests;
