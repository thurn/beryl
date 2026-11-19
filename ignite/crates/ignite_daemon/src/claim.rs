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
//! session's previous command (see `watch`). Each held ticket keeps a silence clock, when its
//! session last ran a command, saved across restarts (see `activity`). The dispatcher's stall
//! procedure (see `dispatchd_stall`) reads it through [`Claims::quiet_since`] when the server has
//! no record of the session's last command, and releases a stopped session's ticket through
//! [`Claims::release_session`].
//!
//! The hub's hand-off routes and the build trees are reached through [`TicketDesk`] and
//! [`TreeDesk`], so this module is tested against fakes of them.

mod activity;
mod brief;
mod finish;
mod parse;
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

use parse::{failure, parse_start};

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
    /// saved branch if the remote has one. With `compiles`, the tree that holds the build
    /// directories beside `files` (the files the ticket names) and is built nearest the tip is
    /// chosen and its usual builds start at once. A lease that waits records `start`, the
    /// request of the `ignite start` that asked, with its handle, so that a daemon that restarts
    /// meanwhile runs that `start` again under the handle instead of ending it.
    fn lease(
        &self,
        ticket: &TicketId,
        session: &SessionId,
        tip: &Commit,
        compiles: bool,
        files: &[String],
        start: &Request,
    ) -> Result<Lease>;
    /// Commits whatever the tree holds as one commit marked as saved at release and pushes the
    /// branch as `ignite/<ticket>`. A failed push is an error and leaves the tree as it was.
    fn save(&self, tree: &TreeId, ticket: &TicketId) -> Result<Saved>;
    /// Ends the tree's processes, resets it to a clean checkout and frees it. `caller` is the
    /// `ignite` client asking, whose process is spared so that it can print the reply.
    fn reset(&self, tree: &TreeId, caller: Option<u32>) -> Result<()>;
    /// Deletes the ticket's saved branch, once the ticket is closed.
    fn drop_saved(&self, ticket: &TicketId) -> Result<()>;
    /// Names the ticket a leased tree holds, keeping its branch.
    fn set_ticket(&self, tree: &TreeId, ticket: &TicketId) -> Result<()>;
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
    /// Trees whose reset failed after their ticket was given up. Nobody holds them, so
    /// [`Claims::tick`] keeps retrying the reset until it works.
    reset_failed: BTreeSet<TreeId>,
    /// The silence clock rows last written, so that only a change is written again.
    saved: BTreeMap<TreeId, rows::ClaimActivity>,
}

/// The tickets held on this machine, shared by the verbs, the request hooks and the dispatcher.
pub struct Claims {
    clock: Arc<dyn Clock>,
    desk: Arc<dyn TicketDesk>,
    trees: Arc<dyn TreeDesk>,
    /// The server, once it exists: it is built with verbs that hold these claims.
    commands: OnceLock<Arc<dyn RunningCommands>>,
    db: Mutex<Connection>,
    state: Mutex<State>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Claims {
    /// Reads the leased ticket trees and their silence clocks from the local database, so a
    /// restarted daemon still knows which tickets it holds and how long each session has been
    /// quiet.
    pub fn new(
        clock: Arc<dyn Clock>,
        desk: Arc<dyn TicketDesk>,
        trees: Arc<dyn TreeDesk>,
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
        for row in activity::recorded(&db) {
            let held = TreeId::new(row.tree.clone())
                .ok()
                .and_then(|tree| state.held.get_mut(&tree))
                .filter(|held| held.ticket.as_str() == row.ticket);
            match held {
                Some(held) => {
                    activity::restore(held, &row);
                    state.saved.insert(held.tree.clone(), row);
                }
                // The tree was freed or leased to another ticket while no daemon ran.
                None => {
                    let _ = ignite_store::local::activity::clear(&db, &row.tree);
                }
            }
        }
        Ok(Arc::new(Claims {
            clock,
            desk,
            trees,
            commands: OnceLock::new(),
            db: Mutex::new(db),
            state: Mutex::new(state),
        }))
    }

    /// Tells the silence clock which sessions have a command running. Until this is called, none
    /// counts as running. A second call changes nothing.
    pub fn watch_commands(&self, commands: Arc<dyn RunningCommands>) {
        let _ = self.commands.set(commands);
    }

    /// When `session`, which holds a ticket tree, last ran a command or was last seen running
    /// one. The clock is saved, so after a restart it still counts from before the restart.
    pub fn quiet_since(&self, session: &SessionId) -> Option<Timestamp> {
        let state = lock(&self.state);
        state
            .held
            .values()
            .filter(|h| &h.session == session)
            .map(|h| h.active_at)
            .max()
    }

    /// The tree a working directory is inside, with the ticket and session holding it.
    pub fn holder(&self, cwd: &Path) -> Option<(TreeId, TicketId, SessionId)> {
        self.forget_released();
        let state = lock(&self.state);
        held_at(&state, cwd).map(|h| (h.tree.clone(), h.ticket.clone(), h.session.clone()))
    }

    /// Gives back `session`'s ticket for the dispatcher, keeping its work on the saved branch:
    /// the note is `reason` followed by what was saved.
    pub fn release_session(
        &self,
        ctx: &Context,
        session: &SessionId,
        ticket: &TicketId,
        reason: &str,
    ) -> Result<()> {
        let held = {
            let state = lock(&self.state);
            let found = state
                .held
                .values()
                .find(|h| &h.session == session && &h.ticket == ticket);
            found.cloned()
        }
        .ok_or_else(|| Error::NotFound(format!("{session} holds no tree for {ticket}")))?;
        let note = |saved: &Saved| format!("{reason} Its work is saved as {}.", saved.branch);
        match self.give_back(ctx, &held, note, None, None) {
            Ok(_) => Ok(()),
            // A tree that a waiting `ignite start "<summary>"` was given keeps the ad hoc
            // placeholder name until the session runs `start` again and claims it. When the
            // daemon restarts in between, nothing claims it: the hub has never heard of the
            // name and answers every release with "Unknown ticket", so the tree stayed leased
            // and the dispatcher reminded the session every 15 minutes, for hours. There is
            // nothing to release at the hub, and `give_back` saved the work before asking, so
            // the tree is freed here.
            Err(Error::NotFound(_)) if is_placeholder(ticket) => {
                if let Some(error) = self.free(&held, None) {
                    eprintln!("ignite: {} not reset: {error}", held.tree);
                }
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    fn start(&self, ctx: &Context, req: &Request) -> Reply {
        let (target, session) = match parse_start(req) {
            Ok(parsed) => parsed,
            Err(reply) => return reply,
        };
        if let Target::Ticket { id, .. } = &target {
            self.claim_restored(ctx, &target, id, &session);
            if let Some(reply) = self.already_held(id, &session, &ctx.config.repository.staging) {
                return reply;
            }
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
        let files = detail
            .as_ref()
            .map(|detail| brief::named_files(&detail.ticket))
            .unwrap_or_default();
        let leased = match self
            .trees
            .lease(&lease_id, &session, &tip, compiles, &files, req)
        {
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
                // The tree is freed without the hub or the network: only a local failure of the
                // reset leaves it leased, and then the tick retries it.
                let reset = self.reset_tree(&leased.tree, req.pid);
                let mut reply = failure(&error, "claim refused");
                if let Err(reset) = reset {
                    let _ = write!(reply.text, "\nreset    {} failed: {reset}", leased.tree);
                }
                return reply;
            }
        };
        // An ad hoc ticket's tree was leased under a placeholder; it takes the number the hub
        // gave the ticket, so that landing it records and closes that ticket.
        if brief.ticket.id != lease_id
            && let Err(error) = self.trees.set_ticket(&leased.tree, &brief.ticket.id)
        {
            eprintln!(
                "ignite: {} keeps the name {lease_id}, not {}: {error}",
                leased.tree, brief.ticket.id
            );
        }
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

    /// Claims at the hub a ticket whose tree a previous daemon leased to `session`, when this
    /// daemon has not seen its claim: a `start` that waited for a free tree and was run again
    /// after a restart finds the tree the waiting lease got meanwhile, which nobody has claimed
    /// at the hub yet. A claim of a ticket the session already holds is answered with its brief,
    /// so a ticket that was claimed is not harmed.
    fn claim_restored(&self, ctx: &Context, target: &Target, id: &TicketId, session: &SessionId) {
        self.forget_released();
        let restored = {
            let state = lock(&self.state);
            state
                .held
                .values()
                .find(|h| &h.ticket == id && &h.session == session && h.brief.is_none())
                .map(|h| h.tree.clone())
        };
        let Some(tree) = restored else {
            return;
        };
        let claim = ClaimRequest {
            target: target.clone(),
            machine: ctx.machine.clone(),
            session: session.clone(),
            tree: tree.clone(),
        };
        match self.desk.claim(&claim) {
            Ok(brief) => {
                if let Some(held) = lock(&self.state).held.get_mut(&tree) {
                    held.brief = Some(brief);
                }
            }
            Err(error) => eprintln!("ignite: cannot claim {id} for the tree {tree}: {error}"),
        }
    }

    /// The reply for a ticket this machine already holds: the brief again for its own session,
    /// a lost claim for any other.
    fn already_held(&self, id: &TicketId, session: &SessionId, staging: &str) -> Option<Reply> {
        self.forget_released();
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
                synced: true,
            },
        );
        drop(state);
        self.save_activity([&leased.tree]);
    }

    /// The held ticket of the request's tree, or the reply that refuses the request.
    fn held_for(&self, req: &Request) -> std::result::Result<Held, Reply> {
        self.forget_released();
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
        synced: false,
    })
}

fn held_at<'a>(state: &'a State, cwd: &Path) -> Option<&'a Held> {
    state.held.values().find(|h| cwd.starts_with(&h.path))
}

/// The prefix of [`adhoc_branch_name`].
const PLACEHOLDER_PREFIX: &str = "adhoc";

/// Whether `ticket` is an ad hoc placeholder from [`adhoc_branch_name`], not a ticket the hub
/// filed.
fn is_placeholder(ticket: &TicketId) -> bool {
    ticket
        .as_str()
        .strip_prefix(PLACEHOLDER_PREFIX)
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

/// The branch name for an ad hoc ticket's tree before the hub has numbered the ticket.
fn adhoc_branch_name(now: Timestamp) -> TicketId {
    let digits: String = now
        .to_string()
        .chars()
        .filter(char::is_ascii_digit)
        .collect();
    TicketId::new(format!("{PLACEHOLDER_PREFIX}{digits}"))
        .unwrap_or_else(|_| unreachable!("letters and digits"))
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
