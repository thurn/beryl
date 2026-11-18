//! Starting and releasing tickets.
//!
//! - `ignite start <ticket> [--reservation <code>]` leases a build tree at the tip, claims the
//!   ticket at the hub for the asking session and that tree, and prints the brief: the ticket,
//!   its tree, the title to set, the files it names and what each dependency left behind.
//!   `ignite start "<summary>"` files an `adhoc` ticket and claims it in the same request.
//! - `ignite brief` prints the brief again.
//! - `ignite release -m "<note>" [--needs <what>]` saves what the tree holds as the branch
//!   `ignite/<ticket>`, gives the ticket back with the note and frees the tree. If the save's push
//!   fails, the release fails and the session keeps the ticket.
//! - `ignite close -m "<reason>"` closes a ticket that needs no landing and frees the tree.
//!
//! [`ClaimHook`], a hook the server runs after every request from a leased tree, renews the
//! ticket's lease and prints the comments and edits that others made to the ticket since the
//! session's previous command (see `watch`). [`Claims::tick`] releases the tickets of sessions
//! that have stopped, but only on the dispatcher's word that the session is idle or gone, never
//! on silence alone, and never while the server says a command of the session is running.
//!
//! The hub's hand-off routes, the build trees and the dispatcher are reached through
//! [`TicketDesk`], [`TreeDesk`] and [`Dispatcher`], so this module is tested against fakes of
//! them.

mod brief;
mod watch;

#[cfg(test)]
mod fakes;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod watch_tests;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use rusqlite::Connection;

use ignite_core::admission::{self, Worked};
use ignite_core::ids::{Commit, Handle, MachineId, Seq, SessionId, TicketId, TreeId};
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::protocol::hub::TicketBrief;
use ignite_core::protocol::tickets::TicketDetail;
use ignite_core::time::Timestamp;
use ignite_core::traits::{Clock, Error, Result};
use ignite_store::local::{rows, shown, trees as tree_rows};

use crate::server::{Context, Verb};

pub use watch::ClaimHook;

/// A ticket expected to use fewer cores than this is not expected to compile Rust, so its tree
/// is not prebuilt and it takes the free tree with the oldest build.
pub const COMPILES_FROM_CORES: f64 = 0.5;

/// The `--needs` value that sets a ticket aside for a person, which requires a note.
pub const NEEDS_PERSON: &str = "person";

/// What `ignite start` asks the hub to claim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A ticket by ID, with the code of the reservation made for this session, if any.
    Ticket { id: TicketId, code: Option<String> },
    /// A summary from which the hub files an `adhoc` ticket.
    Adhoc(String),
}

/// `POST /claims`: claim a ticket for one session and tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaimRequest {
    pub target: Target,
    pub machine: MachineId,
    pub session: SessionId,
    pub tree: TreeId,
}

/// What a release saved: the branch on the remote, the commits it holds beyond the tip the tree
/// was leased at (the saved commit included), and the uncommitted paths that commit took in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved {
    pub branch: String,
    pub commits: u32,
    pub uncommitted: Vec<String>,
}

/// `POST /release`: the ticket goes back to the queue with a note, the saved branch and, with
/// `needs`, the label `needs:<needs>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseRequest {
    pub ticket: TicketId,
    pub machine: MachineId,
    pub session: SessionId,
    pub note: String,
    pub needs: Option<String>,
    pub saved: Saved,
}

/// The hub's hand-off routes as this module needs them.
pub trait TicketDesk: Send + Sync {
    /// `GET /tickets/<id>?comments=1&history=1`.
    fn detail(&self, ticket: &TicketId) -> Result<TicketDetail>;
    /// The machine's landed tickets, the history behind expected cores.
    fn history(&self, machine: &MachineId) -> Result<Vec<Worked>>;
    /// Claims the target. A reserved ticket needs its reservation's code; a lapsed or wrong
    /// code, or a ticket that another session holds, is [`Error::Conflict`].
    fn claim(&self, claim: &ClaimRequest) -> Result<TicketBrief>;
    /// The brief of a ticket the session already holds.
    fn brief(&self, ticket: &TicketId, session: &SessionId) -> Result<TicketBrief>;
    fn release(&self, release: &ReleaseRequest) -> Result<()>;
    /// Closes the ticket without a landing and frees its worker.
    fn close(
        &self,
        ticket: &TicketId,
        machine: &MachineId,
        session: &SessionId,
        reason: &str,
    ) -> Result<()>;
}

/// A branch of saved work that a lease checked out: how many commits it holds beyond the tip,
/// who made them, and whether the last is unfinished work saved at release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resumed {
    pub commits: u32,
    pub authors: Vec<String>,
    pub last_saved: bool,
}

/// A ticket tree leased to one ticket, on `branch`, based at the tip `at`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Leased {
    pub tree: TreeId,
    pub path: PathBuf,
    pub branch: String,
    pub at: Commit,
    /// The ticket's saved branch, when an earlier release left one and the lease checked it out.
    pub resumed: Option<Resumed>,
}

/// The answer to a lease.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lease {
    Leased(Leased),
    /// Every ticket tree is leased; the handle finishes when one is free.
    Waiting(Handle),
}

/// The build trees as this module needs them.
pub trait TreeDesk: Send + Sync {
    /// Leases a free ticket tree to the ticket and session at `tip`, checking out the ticket's
    /// saved branch if the remote has one. With `compiles`, the tree built nearest the tip is
    /// chosen and its usual builds start at once.
    fn lease(
        &self,
        ticket: &TicketId,
        session: &SessionId,
        tip: &Commit,
        compiles: bool,
    ) -> Result<Lease>;
    /// Commits whatever the tree holds as one commit marked as saved at release and pushes the
    /// branch as `ignite/<ticket>`. A failed push is an error and leaves the tree as it was.
    fn save(&self, tree: &TreeId, ticket: &TicketId) -> Result<Saved>;
    /// Ends the tree's processes, resets it to a clean checkout and frees it.
    fn reset(&self, tree: &TreeId) -> Result<()>;
    /// Deletes the ticket's saved branch, once the ticket is closed.
    fn drop_saved(&self, ticket: &TicketId) -> Result<()>;
}

/// What the desktop app says of a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Liveness {
    /// In the middle of a turn.
    Running,
    /// Waiting for a prompt.
    Idle,
    /// The app confirms that the session no longer exists.
    Gone,
    /// The lookup failed. This is not an answer.
    Unknown,
}

/// The dispatcher session, through the daemon's queue of instructions.
pub trait Dispatcher: Send + Sync {
    /// Queues a `check` of the session; returns the instruction's ID.
    fn check(&self, session: &SessionId) -> u64;
    /// The answer to a `check`, once the dispatcher has acknowledged it.
    fn answer(&self, check: u64) -> Option<Liveness>;
    /// Queues a `message` to the session.
    fn message(&self, session: &SessionId, text: &str);
}

/// Whether a session has an `ignite` command running, `ignite wait` included.
pub trait RunningCommands: Send + Sync {
    fn running(&self, session: &SessionId) -> bool;
}

impl RunningCommands for crate::server::Server {
    fn running(&self, session: &SessionId) -> bool {
        self.current_command(session)
            .is_some_and(|command| command.ended_at.is_none())
    }
}

/// Where a quiet ticket is in the stall procedure (see `watch`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Stall {
    /// Commands are recent, or one is running.
    Active,
    /// A `check` is queued. `second` is set for the check after the message.
    Asked { check: u64, second: bool },
    /// The session was idle and has been sent one message.
    Messaged { at: Timestamp },
    /// A `check`'s lookup failed at `at`; it is asked again later.
    Retry { at: Timestamp, second: bool },
}

/// One ticket held on this machine.
#[derive(Clone, Debug)]
struct Held {
    ticket: TicketId,
    session: SessionId,
    tree: TreeId,
    path: PathBuf,
    branch: String,
    /// The tip the tree was leased at; unknown after a restart.
    at: Option<Commit>,
    /// The brief as last fetched; `None` after a restart or an edit to the ticket.
    brief: Option<TicketBrief>,
    /// When the last command ended, or when a running one was last seen.
    active_at: Timestamp,
    stall: Stall,
    /// Whether this daemon has read the ticket's comments and history since it started.
    synced: bool,
}

#[derive(Default)]
struct State {
    held: BTreeMap<TreeId, Held>,
    /// The change feed's position, once known.
    cursor: Option<Seq>,
    /// Held tickets the feed says changed since their session's last command.
    dirty: BTreeSet<TicketId>,
}

/// The tickets held on this machine, shared by the verbs, the request hooks and the stall check.
pub struct Claims {
    clock: Arc<dyn Clock>,
    desk: Arc<dyn TicketDesk>,
    trees: Arc<dyn TreeDesk>,
    dispatcher: Arc<dyn Dispatcher>,
    /// The server, once it exists: it is built with verbs that hold these claims.
    commands: OnceLock<Arc<dyn RunningCommands>>,
    db: Mutex<Connection>,
    state: Mutex<State>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Claims {
    /// Reads the leased ticket trees from the local database, so a restarted daemon still knows
    /// which tickets it holds.
    pub fn new(
        clock: Arc<dyn Clock>,
        desk: Arc<dyn TicketDesk>,
        trees: Arc<dyn TreeDesk>,
        dispatcher: Arc<dyn Dispatcher>,
        db: Connection,
    ) -> Result<Arc<Claims>> {
        let now = clock.now();
        let mut state = State::default();
        let rows = tree_rows::all(&db).map_err(|e| Error::Io(e.to_string()))?;
        for row in rows {
            if let Some(held) = held_from_row(&row, now) {
                state.held.insert(held.tree.clone(), held);
            }
        }
        Ok(Arc::new(Claims {
            clock,
            desk,
            trees,
            dispatcher,
            commands: OnceLock::new(),
            db: Mutex::new(db),
            state: Mutex::new(state),
        }))
    }

    /// Tells the stall check which sessions have a command running. Until this is called, none
    /// counts as running. A second call changes nothing.
    pub fn watch_commands(&self, commands: Arc<dyn RunningCommands>) {
        let _ = self.commands.set(commands);
    }

    /// The tree a working directory is inside, with the ticket and session holding it.
    pub fn holder(&self, cwd: &Path) -> Option<(TreeId, TicketId, SessionId)> {
        let state = lock(&self.state);
        held_at(&state, cwd).map(|h| (h.tree.clone(), h.ticket.clone(), h.session.clone()))
    }

    fn start(&self, ctx: &Context, req: &Request) -> Reply {
        let (target, session) = match parse_start(req) {
            Ok(parsed) => parsed,
            Err(reply) => return reply,
        };
        if let Target::Ticket { id, .. } = &target
            && let Some(reply) = self.already_held(id, &session, &ctx.config.repository.staging)
        {
            return reply;
        }
        let now = ctx.clock.now();
        let detail = match &target {
            Target::Ticket { id, .. } => match self.desk.detail(id) {
                Ok(detail) => Some(detail),
                Err(error) => return failure(&error, &format!("cannot read {id}")),
            },
            Target::Adhoc(_) => None,
        };
        let tip = match ctx.git.tip(&ctx.config.repository.staging) {
            Ok(tip) => tip,
            Err(error) => return failure(&error, "cannot read the tip"),
        };
        let compiles = self.expected_cores(ctx, detail.as_ref(), now) >= COMPILES_FROM_CORES;
        let lease_id = match &target {
            Target::Ticket { id, .. } => id.clone(),
            // The hub numbers an ad hoc ticket only when it files it, and the tree's branch needs
            // a name before that.
            Target::Adhoc(_) => adhoc_branch_name(now),
        };
        let leased = match self.trees.lease(&lease_id, &session, &tip, compiles) {
            Ok(Lease::Leased(leased)) => leased,
            Ok(Lease::Waiting(handle)) => return brief::waiting_for_tree(handle),
            Err(error) => return failure(&error, "cannot lease a tree"),
        };
        let claim = ClaimRequest {
            target,
            machine: ctx.machine.clone(),
            session: session.clone(),
            tree: leased.tree.clone(),
        };
        let brief = match self.desk.claim(&claim) {
            Ok(brief) => brief,
            Err(error) => {
                let reset = self.trees.reset(&leased.tree);
                let mut reply = failure(&error, "claim refused");
                if let Err(reset) = reset {
                    let _ = write!(reply.text, "\nreset    {} failed: {reset}", leased.tree);
                }
                return reply;
            }
        };
        self.remember(&brief, &leased, &session, detail.as_ref(), now);
        let tree = brief::TreeView {
            tree: &leased.tree,
            path: &leased.path,
            branch: &leased.branch,
            at: Some(&leased.at),
            resumed: leased.resumed.as_ref(),
            staging: &ctx.config.repository.staging,
        };
        Reply {
            exit: Exit::Success,
            text: brief::render(&brief, &tree),
            json: Some(brief::json(&brief, &tree)),
            handle: None,
        }
    }

    /// The reply for a ticket this machine already holds: the brief again for its own session,
    /// a lost claim for any other.
    fn already_held(&self, id: &TicketId, session: &SessionId, staging: &str) -> Option<Reply> {
        let held = {
            let state = lock(&self.state);
            state.held.values().find(|h| &h.ticket == id).cloned()?
        };
        if &held.session != session {
            return Some(Reply::text(
                Exit::Conflict,
                format!("held     {id} is held by another session in {}", held.tree),
            ));
        }
        Some(self.brief_reply(&held, staging))
    }

    fn expected_cores(&self, ctx: &Context, detail: Option<&TicketDetail>, now: Timestamp) -> f64 {
        let history = self.desk.history(&ctx.machine).unwrap_or_default();
        let (planner, files) = match detail {
            Some(detail) => (detail.ticket.cores, brief::named_files(&detail.ticket)),
            None => (None, Vec::new()),
        };
        let mut areas: Vec<String> = files.iter().map(|f| admission::area_of(f, &[])).collect();
        areas.sort();
        areas.dedup();
        admission::expected_cores(planner, &areas, &history, now)
    }

    /// Records the new claim, and marks every comment and history entry the ticket has as
    /// shown, because the brief carries the ticket as it is.
    fn remember(
        &self,
        brief: &TicketBrief,
        leased: &Leased,
        session: &SessionId,
        detail: Option<&TicketDetail>,
        now: Timestamp,
    ) {
        let ticket = brief.ticket.id.clone();
        let marks = rows::Shown {
            tree: leased.tree.to_string(),
            ticket: ticket.to_string(),
            comment: detail.map_or(0, watch::newest_comment),
            history: detail.map_or(0, watch::newest_history),
        };
        // A failed write only means a later command may show an old change again.
        let _ = shown::set(&lock(&self.db), &marks);
        let mut state = lock(&self.state);
        if state.cursor.is_none() {
            state.cursor = Some(brief.ticket.version);
        }
        state.dirty.remove(&ticket);
        state.held.insert(
            leased.tree.clone(),
            Held {
                ticket,
                session: session.clone(),
                tree: leased.tree.clone(),
                path: leased.path.clone(),
                branch: leased.branch.clone(),
                at: Some(leased.at.clone()),
                brief: Some(brief.clone()),
                active_at: now,
                stall: Stall::Active,
                synced: true,
            },
        );
    }

    /// The held ticket of the request's tree, or the reply that refuses the request.
    fn held_for(&self, req: &Request) -> std::result::Result<Held, Reply> {
        let state = lock(&self.state);
        let Some(held) = held_at(&state, &req.cwd) else {
            return Err(Reply::text(
                Exit::Usage,
                format!(
                    "no ticket: {} is not inside a leased tree; run this from the tree that \
                     `ignite start` printed",
                    req.cwd.display()
                ),
            ));
        };
        if req.session.as_ref() != Some(&held.session) {
            return Err(Reply::text(
                Exit::Conflict,
                format!(
                    "lost     {} belongs to another session; do not use this path again",
                    held.tree
                ),
            ));
        }
        Ok(held.clone())
    }

    fn brief_reply(&self, held: &Held, staging: &str) -> Reply {
        let brief = match &held.brief {
            Some(brief) => brief.clone(),
            None => match self.desk.brief(&held.ticket, &held.session) {
                Ok(brief) => {
                    let mut state = lock(&self.state);
                    if let Some(h) = state.held.get_mut(&held.tree) {
                        h.brief = Some(brief.clone());
                    }
                    brief
                }
                Err(error) => return failure(&error, &format!("cannot read {}", held.ticket)),
            },
        };
        let tree = brief::TreeView {
            tree: &held.tree,
            path: &held.path,
            branch: &held.branch,
            at: held.at.as_ref(),
            resumed: None,
            staging,
        };
        Reply {
            exit: Exit::Success,
            text: brief::render(&brief, &tree),
            json: Some(brief::json(&brief, &tree)),
            handle: None,
        }
    }

    fn release(&self, ctx: &Context, req: &Request) -> Reply {
        let (note, needs) = match parse_release(req) {
            Ok(parsed) => parsed,
            Err(reply) => return reply,
        };
        let held = match self.held_for(req) {
            Ok(held) => held,
            Err(reply) => return reply,
        };
        let title = brief::title_of(held.brief.as_ref(), &held.ticket);
        match self.give_back(ctx, &held, |_| note, needs) {
            Ok((saved, reset)) => brief::released(&held.ticket, &held.tree, &saved, &title, reset),
            Err(error) => failure(&error, &format!("{} is still yours", held.ticket)),
        }
    }

    /// Saves the tree's work, releases the ticket at the hub with the note made from what was saved,
    /// and frees the tree. If the save or
    /// the release fails, the session keeps the ticket and the tree. Returns what was saved and
    /// the reset's failure, if any.
    fn give_back(
        &self,
        ctx: &Context,
        held: &Held,
        note: impl FnOnce(&Saved) -> String,
        needs: Option<String>,
    ) -> Result<(Saved, Option<Error>)> {
        let saved = self.trees.save(&held.tree, &held.ticket)?;
        self.desk.release(&ReleaseRequest {
            ticket: held.ticket.clone(),
            machine: ctx.machine.clone(),
            session: held.session.clone(),
            note: note(&saved),
            needs,
            saved: saved.clone(),
        })?;
        Ok((saved, self.free(held)))
    }

    fn close(&self, ctx: &Context, req: &Request) -> Reply {
        let reason = match parse_close(req) {
            Ok(reason) => reason,
            Err(reply) => return reply,
        };
        let held = match self.held_for(req) {
            Ok(held) => held,
            Err(reply) => return reply,
        };
        let title = brief::title_of(held.brief.as_ref(), &held.ticket);
        if let Err(error) = self
            .desk
            .close(&held.ticket, &ctx.machine, &held.session, &reason)
        {
            return failure(&error, &format!("{} is still open", held.ticket));
        }
        let dropped = self.trees.drop_saved(&held.ticket).err();
        let reset = self.free(&held);
        brief::closed(&held.ticket, &held.tree, &reason, &title, dropped, reset)
    }

    /// Forgets a held ticket and resets its tree; returns the reset's failure, if any.
    fn free(&self, held: &Held) -> Option<Error> {
        {
            let mut state = lock(&self.state);
            state.held.remove(&held.tree);
            state.dirty.remove(&held.ticket);
        }
        let _ = shown::clear(&lock(&self.db), held.tree.as_str());
        self.trees.reset(&held.tree).err()
    }
}

/// A held ticket rebuilt from a leased tree's row.
fn held_from_row(row: &rows::Tree, now: Timestamp) -> Option<Held> {
    if row.kind != "ticket" || row.state != tree_rows::State::Leased.as_str() {
        return None;
    }
    Some(Held {
        ticket: TicketId::new(row.ticket.clone()?).ok()?,
        session: SessionId::new(row.session.clone()?).ok()?,
        tree: TreeId::new(row.id.clone()).ok()?,
        path: PathBuf::from(&row.path),
        branch: row.branch.clone().unwrap_or_default(),
        at: None,
        brief: None,
        active_at: now,
        stall: Stall::Active,
        synced: false,
    })
}

fn held_at<'a>(state: &'a State, cwd: &Path) -> Option<&'a Held> {
    state.held.values().find(|h| cwd.starts_with(&h.path))
}

/// The branch name for an ad hoc ticket's tree before the hub has numbered the ticket.
fn adhoc_branch_name(now: Timestamp) -> TicketId {
    let digits: String = now
        .to_string()
        .chars()
        .filter(char::is_ascii_digit)
        .collect();
    TicketId::new(format!("adhoc{digits}")).unwrap_or_else(|_| unreachable!("letters and digits"))
}

/// The reply for a failure behind one of the traits, with the exit code its kind calls for.
fn failure(error: &Error, what: &str) -> Reply {
    let exit = match error {
        Error::Unreachable(_) => Exit::Unreachable,
        Error::Conflict(_) => Exit::Conflict,
        Error::NotFound(_) | Error::Invalid(_) | Error::Io(_) => Exit::Usage,
    };
    Reply::text(exit, format!("failed   {what}: {error}"))
}

fn usage(text: &str) -> Reply {
    Reply::text(Exit::Usage, format!("usage    {text}"))
}

fn session_of(req: &Request) -> std::result::Result<SessionId, Reply> {
    req.session.clone().ok_or_else(|| {
        usage("no session: run this from a desktop-app session, or set IGNITE_SESSION")
    })
}

/// Reads `--name value` and `--name=value` options (and `-m value` for `--message`) and the
/// positional words, rejecting any other option.
fn options(
    args: &[String],
    known: &[(&str, Option<&str>)],
) -> std::result::Result<(BTreeMap<String, String>, Vec<String>), Reply> {
    let mut values = BTreeMap::new();
    let mut words = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if !arg.starts_with('-') || arg == "-" {
            words.push(arg.clone());
            continue;
        }
        let (flag, inline) = match arg.split_once('=') {
            Some((flag, value)) => (flag, Some(value.to_owned())),
            None => (arg.as_str(), None),
        };
        let Some((name, _)) = known
            .iter()
            .find(|(long, short)| flag == format!("--{long}") || Some(flag) == *short)
        else {
            return Err(usage(&format!("unknown option {flag}")));
        };
        let Some(value) = inline.or_else(|| iter.next().cloned()) else {
            return Err(usage(&format!("{flag} needs a value")));
        };
        values.insert((*name).to_owned(), value);
    }
    Ok((values, words))
}

fn parse_start(req: &Request) -> std::result::Result<(Target, SessionId), Reply> {
    let (mut values, words) = options(&req.args, &[("reservation", None)])?;
    let session = session_of(req)?;
    let text = words.join(" ");
    let code = values.remove("reservation");
    if text.trim().is_empty() {
        return Err(usage(
            "ignite start <ticket> [--reservation <code>] | ignite start \"<summary>\"",
        ));
    }
    // A summary is several words; a ticket ID is one.
    if text.contains(char::is_whitespace) {
        if code.is_some() {
            return Err(usage("--reservation goes with a ticket ID, not a summary"));
        }
        return Ok((Target::Adhoc(text.trim().to_owned()), session));
    }
    match TicketId::new(text.clone()) {
        Ok(id) => Ok((Target::Ticket { id, code }, session)),
        Err(error) => Err(usage(&format!(
            "{error}; quote a summary of several words to file a ticket"
        ))),
    }
}

fn parse_release(req: &Request) -> std::result::Result<(String, Option<String>), Reply> {
    let (mut values, words) = options(&req.args, &[("message", Some("-m")), ("needs", None)])?;
    if !words.is_empty() {
        return Err(usage("ignite release -m \"<note>\" [--needs <what>]"));
    }
    let note = values.remove("message").unwrap_or_default();
    let needs = values.remove("needs");
    if needs.as_deref() == Some(NEEDS_PERSON) && note.trim().is_empty() {
        return Err(usage(
            "--needs person needs a note (-m) stating the exact need and what was tried",
        ));
    }
    Ok((note, needs))
}

fn parse_close(req: &Request) -> std::result::Result<String, Reply> {
    let (mut values, words) = options(&req.args, &[("message", Some("-m"))])?;
    match values.remove("message") {
        Some(reason) if words.is_empty() && !reason.trim().is_empty() => Ok(reason),
        _ => Err(usage("ignite close -m \"<reason>\"")),
    }
}

/// `ignite start`: claims a ticket, leases a build tree at the tip and prints the ticket.
pub struct StartVerb(pub Arc<Claims>);

impl Verb for StartVerb {
    fn name(&self) -> &'static str {
        "start"
    }

    fn needs_the_tree_lease(&self) -> bool {
        false
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        self.0.start(ctx, req)
    }
}

/// `ignite brief`: prints the ticket's brief again.
pub struct BriefVerb(pub Arc<Claims>);

impl Verb for BriefVerb {
    fn name(&self) -> &'static str {
        "brief"
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        match self.0.held_for(req) {
            Ok(held) => self.0.brief_reply(&held, &ctx.config.repository.staging),
            Err(reply) => reply,
        }
    }
}

/// `ignite release`: saves the tree's work, gives the ticket back and frees its tree.
pub struct ReleaseVerb(pub Arc<Claims>);

impl Verb for ReleaseVerb {
    fn name(&self) -> &'static str {
        "release"
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        self.0.release(ctx, req)
    }
}

/// `ignite close`: closes a ticket that needs no landing and frees its tree.
pub struct CloseVerb(pub Arc<Claims>);

impl Verb for CloseVerb {
    fn name(&self) -> &'static str {
        "close"
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        self.0.close(ctx, req)
    }
}
