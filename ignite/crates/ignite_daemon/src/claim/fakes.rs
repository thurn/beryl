//! Fakes of the hub's hand-off routes and the build trees, and the context the
//! claim tests share.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::Value;

use ignite_core::admission::Worked;
use ignite_core::config::Config;
use ignite_core::fakes::{FakeBuilds, FakeClock, FakeGit, FakeHub, FakeLauncher};
use ignite_core::ids::{Commit, Handle, MachineId, Seq, SessionId, TicketId, TreeId};
use ignite_core::protocol::client::Request;
use ignite_core::protocol::hub::{DependencyNote, TicketBrief};
use ignite_core::protocol::tickets::{Comment, History, Status, Ticket, TicketDetail, TicketType};
use ignite_core::time::Timestamp;
use ignite_core::traits::{Error, Result};

use super::{
    ClaimRequest, Claims, Lease, Leased, ReleaseRequest, Resumed, RunningCommands, Saved, Target,
    TicketDesk, TreeDesk, lock,
};
use crate::server::Context;

pub fn id(text: &str) -> TicketId {
    TicketId::new(text).unwrap()
}

pub fn session(text: &str) -> SessionId {
    SessionId::new(text).unwrap()
}

pub fn ticket(id_text: &str, title: &str, body: &str) -> Ticket {
    let id = id(id_text);
    Ticket {
        parent: id.parent(),
        id,
        next_child: 1,
        title: title.to_owned(),
        body: body.to_owned(),
        acceptance: String::new(),
        design: String::new(),
        notes: String::new(),
        ticket_type: TicketType::Task,
        status: Status::Open,
        priority: 2,
        assignee: None,
        last: 0,
        labels: Vec::new(),
        metadata: BTreeMap::new(),
        defer_until: None,
        close_reason: None,
        created_at: Timestamp::EPOCH,
        created_by: "test".to_owned(),
        updated_at: Timestamp::EPOCH,
        closed_at: None,
        version: Seq(0),
        workers: None,
        max_workers: None,
        estimate_minutes: None,
        unfiled_minutes: None,
        cores: None,
        machine: None,
        session: None,
        tree: None,
        claimed_at: None,
        landed_at: None,
        landed: None,
        saving: None,
        needs_os: None,
        fallback: false,
        every_hours: None,
    }
}

/// The hub's hand-off routes, in memory.
#[derive(Default)]
pub struct FakeDesk {
    pub details: Mutex<BTreeMap<TicketId, TicketDetail>>,
    pub needs: Mutex<BTreeMap<TicketId, Vec<DependencyNote>>>,
    /// Reservation codes by ticket.
    pub reservations: Mutex<BTreeMap<TicketId, String>>,
    pub holders: Mutex<BTreeMap<TicketId, SessionId>>,
    pub claims: Mutex<Vec<ClaimRequest>>,
    pub releases: Mutex<Vec<ReleaseRequest>>,
    pub closes: Mutex<Vec<(TicketId, String)>>,
    pub detail_reads: Mutex<u32>,
    clock: Mutex<i64>,
}

impl FakeDesk {
    pub fn add(&self, ticket: Ticket) {
        let detail = TicketDetail {
            ticket,
            edges: Vec::new(),
            children: Vec::new(),
            ready: true,
            blocked_by: Vec::new(),
            comments: Some(Vec::new()),
            history: Some(Vec::new()),
            aliases: Vec::new(),
        };
        lock(&self.details).insert(detail.ticket.id.clone(), detail);
    }

    fn tick(&self) -> Timestamp {
        let mut clock = lock(&self.clock);
        *clock += 1;
        #[allow(clippy::cast_precision_loss)]
        Timestamp::EPOCH.plus_seconds(*clock as f64)
    }

    pub fn comment(&self, ticket: &str, author: &str, text: &str) {
        let at = self.tick();
        let mut details = lock(&self.details);
        let detail = details.get_mut(&id(ticket)).unwrap();
        let comments = detail.comments.get_or_insert_with(Vec::new);
        let n = u32::try_from(comments.len()).unwrap() + 1;
        comments.push(Comment {
            ticket: id(ticket),
            n,
            author: author.to_owned(),
            at,
            text: text.to_owned(),
            legacy_id: None,
        });
    }

    pub fn edit(&self, ticket: &str, actor: &str, change: Value) {
        let at = self.tick();
        let mut details = lock(&self.details);
        let detail = details.get_mut(&id(ticket)).unwrap();
        detail.ticket.version = Seq(detail.ticket.version.0 + 1);
        let seq = detail.ticket.version;
        detail.history.get_or_insert_with(Vec::new).push(History {
            ticket: id(ticket),
            seq,
            actor: actor.to_owned(),
            at,
            change,
        });
    }

    /// Closes a ticket as `ignite ticket close` does: at the hub only, which drops the claim.
    pub fn close_by_hand(&self, ticket: &str) {
        lock(&self.holders).remove(&id(ticket));
        lock(&self.details)
            .get_mut(&id(ticket))
            .unwrap()
            .ticket
            .status = Status::Closed;
    }

    /// Reopens a ticket as upkeep reopens a regression whose fix did not work: open, with no
    /// assignee, while the daemon still holds its tree.
    pub fn reopen(&self, ticket: &str) {
        lock(&self.holders).remove(&id(ticket));
        let mut details = lock(&self.details);
        let held = &mut details.get_mut(&id(ticket)).unwrap().ticket;
        held.status = Status::Open;
        held.assignee = None;
    }

    /// Records the ticket's holder in its detail, as the hub's claim and release do.
    fn assign(&self, ticket: &TicketId, holder: Option<&SessionId>) {
        let mut holders = lock(&self.holders);
        match holder {
            Some(session) => holders.insert(ticket.clone(), session.clone()),
            None => holders.remove(ticket),
        };
        if let Some(detail) = lock(&self.details).get_mut(ticket) {
            detail.ticket.assignee = holder.map(ToString::to_string);
            detail.ticket.status = if holder.is_some() {
                Status::InProgress
            } else {
                Status::Open
            };
        }
    }

    /// The hub's refusal of a release or close from a session that no longer holds the ticket:
    /// it was closed, reopened or claimed by another session.
    fn refuse_not_held(&self, ticket: &TicketId, session: &SessionId) -> Result<()> {
        let closed = lock(&self.details)
            .get(ticket)
            .is_some_and(|d| d.ticket.status == Status::Closed);
        if closed || lock(&self.holders).get(ticket) != Some(session) {
            return Err(Error::Conflict(format!(
                "{ticket} is not held by {session}"
            )));
        }
        Ok(())
    }

    fn brief_of(&self, ticket: &TicketId) -> Result<TicketBrief> {
        let details = lock(&self.details);
        let detail = details
            .get(ticket)
            .ok_or_else(|| Error::NotFound(ticket.to_string()))?;
        Ok(TicketBrief {
            ticket: detail.ticket.clone(),
            needs: lock(&self.needs).get(ticket).cloned().unwrap_or_default(),
            saved_branch: None,
            attachments: Vec::new(),
        })
    }
}

impl TicketDesk for FakeDesk {
    fn detail(&self, ticket: &TicketId) -> Result<TicketDetail> {
        *lock(&self.detail_reads) += 1;
        lock(&self.details)
            .get(ticket)
            .cloned()
            .ok_or_else(|| Error::NotFound(ticket.to_string()))
    }

    fn history(&self, _machine: &MachineId) -> Result<Vec<Worked>> {
        Ok(Vec::new())
    }

    fn claim(&self, claim: &ClaimRequest) -> Result<TicketBrief> {
        lock(&self.claims).push(claim.clone());
        let ticket = match &claim.target {
            Target::Adhoc(summary) => {
                let n = lock(&self.details).len() + 1;
                let mut new = ticket(&format!("Tadhoc.{n}"), summary, "");
                new.labels = vec!["adhoc".to_owned()];
                let id = new.id.clone();
                self.add(new);
                id
            }
            Target::Ticket { id, code } => {
                if !lock(&self.details).contains_key(id) {
                    return Err(Error::NotFound(id.to_string()));
                }
                let reserved = lock(&self.reservations).get(id).cloned();
                if reserved.as_ref() != code.as_ref() && (code.is_some() || reserved.is_some()) {
                    return Err(Error::Conflict(format!(
                        "{id}: no live reservation with that code"
                    )));
                }
                if let Some(holder) = lock(&self.holders).get(id)
                    && holder != &claim.session
                {
                    return Err(Error::Conflict(format!("{id} is held by another session")));
                }
                lock(&self.reservations).remove(id);
                id.clone()
            }
        };
        self.assign(&ticket, Some(&claim.session));
        self.brief_of(&ticket)
    }

    fn brief(&self, ticket: &TicketId, _session: &SessionId) -> Result<TicketBrief> {
        self.brief_of(ticket)
    }

    fn release(&self, release: &ReleaseRequest) -> Result<()> {
        // The hub answers a release of a ticket it never filed with "Unknown ticket".
        if !lock(&self.details).contains_key(&release.ticket) {
            return Err(Error::NotFound(format!(
                "{}: Unknown ticket",
                release.ticket
            )));
        }
        self.refuse_not_held(&release.ticket, &release.session)?;
        self.assign(&release.ticket, None);
        lock(&self.releases).push(release.clone());
        Ok(())
    }

    fn close(
        &self,
        ticket: &TicketId,
        _machine: &MachineId,
        session: &SessionId,
        reason: &str,
    ) -> Result<()> {
        self.refuse_not_held(ticket, session)?;
        lock(&self.holders).remove(ticket);
        if let Some(detail) = lock(&self.details).get_mut(ticket) {
            detail.ticket.status = Status::Closed;
        }
        lock(&self.closes).push((ticket.clone(), reason.to_owned()));
        Ok(())
    }
}

/// Build trees in memory: a free list, the uncommitted files of each tree, and the saved
/// branches on the remote.
#[derive(Default)]
pub struct FakeTrees {
    pub free: Mutex<Vec<TreeId>>,
    pub files: Mutex<BTreeMap<TreeId, Vec<String>>>,
    /// Saved branches by ticket: the uncommitted files they took in.
    pub saved: Mutex<BTreeMap<TicketId, Vec<String>>>,
    pub save_fails: Mutex<bool>,
    /// Makes every reset fail, as a tree that cannot be cleaned would.
    pub reset_fails: Mutex<bool>,
    pub leases: Mutex<Vec<(TicketId, bool)>>,
    pub resets: Mutex<Vec<TreeId>>,
    /// The ticket each tree was renamed to after its claim.
    pub renamed: Mutex<Vec<(TreeId, TicketId)>>,
}

pub fn tree_path(tree: &TreeId) -> PathBuf {
    PathBuf::from(format!("/trees/{tree}"))
}

impl FakeTrees {
    pub fn with_trees(count: u32) -> FakeTrees {
        let trees = FakeTrees::default();
        for n in (1..=count).rev() {
            lock(&trees.free).push(TreeId::new(format!("tree-{n:02}")).unwrap());
        }
        trees
    }

    pub fn touch(&self, tree: &str, file: &str) {
        lock(&self.files)
            .entry(TreeId::new(tree).unwrap())
            .or_default()
            .push(file.to_owned());
    }
}

impl TreeDesk for FakeTrees {
    fn set_ticket(&self, tree: &TreeId, ticket: &TicketId) -> Result<()> {
        lock(&self.renamed).push((tree.clone(), ticket.clone()));
        Ok(())
    }

    fn lease(
        &self,
        ticket: &TicketId,
        _session: &SessionId,
        tip: &Commit,
        compiles: bool,
        _files: &[String],
        _start: &ignite_core::protocol::client::Request,
    ) -> Result<Lease> {
        lock(&self.leases).push((ticket.clone(), compiles));
        let Some(tree) = lock(&self.free).pop() else {
            return Ok(Lease::Waiting(Handle::new("T-1").unwrap()));
        };
        let resumed = lock(&self.saved).get(ticket).cloned().map(|files| {
            lock(&self.files).insert(tree.clone(), files.clone());
            Resumed {
                commits: 1,
                authors: vec!["local_old".to_owned()],
                last_saved: !files.is_empty(),
            }
        });
        Ok(Lease::Leased(Leased {
            path: tree_path(&tree),
            tree,
            branch: format!("ignite/{ticket}"),
            at: tip.clone(),
            resumed,
        }))
    }

    fn save(&self, tree: &TreeId, ticket: &TicketId) -> Result<Saved> {
        if *lock(&self.save_fails) {
            return Err(Error::Unreachable(
                "the remote rejected the push".to_owned(),
            ));
        }
        let files = lock(&self.files).get(tree).cloned().unwrap_or_default();
        lock(&self.saved).insert(ticket.clone(), files.clone());
        Ok(Saved {
            branch: format!("ignite/{ticket}"),
            commits: u32::from(!files.is_empty()),
            uncommitted: files,
        })
    }

    fn reset(&self, tree: &TreeId, _caller: Option<u32>) -> Result<()> {
        if *lock(&self.reset_fails) {
            return Err(Error::Io("the tree would not reset".to_owned()));
        }
        lock(&self.files).remove(tree);
        lock(&self.resets).push(tree.clone());
        lock(&self.free).push(tree.clone());
        Ok(())
    }

    fn drop_saved(&self, ticket: &TicketId) -> Result<()> {
        lock(&self.saved).remove(ticket);
        Ok(())
    }
}

/// The server's view of running commands: the sessions listed are in the middle of one.
#[derive(Default)]
pub struct FakeCommands {
    pub running: Mutex<Vec<SessionId>>,
}

impl RunningCommands for FakeCommands {
    fn running(&self, session: &SessionId) -> bool {
        lock(&self.running).contains(session)
    }
}

/// Everything a claim test needs.
pub struct World {
    pub clock: Arc<FakeClock>,
    pub hub: Arc<FakeHub>,
    pub desk: Arc<FakeDesk>,
    pub git: Arc<FakeGit>,
    pub trees: Arc<FakeTrees>,
    pub commands: Arc<FakeCommands>,
    pub ctx: Context,
    pub claims: Arc<Claims>,
}

pub fn config() -> Config {
    let text = r#"
        [repository]
        remote = "origin"
        staging = "staging"
        master = "master"
        trees = "trees"
        checks = "checks"
        [hub]
        url = "http://localhost:1"
        project = "test"
        [gate]
        threshold = 0.01
        cap = 10
        [machine.local]
        ticket_trees = 2
        landing_trees = 1
    "#;
    Config::parse("ignite.toml", text).unwrap()
}

impl World {
    pub fn new() -> World {
        World::with_db(ignite_store::local::open_in_memory().unwrap())
    }

    pub fn with_db(db: rusqlite::Connection) -> World {
        World::with_db_at(db, Timestamp::EPOCH)
    }

    /// A daemon started at `now` over an existing database, as after a restart.
    pub fn with_db_at(db: rusqlite::Connection, now: Timestamp) -> World {
        let clock = Arc::new(FakeClock::new(now));
        let hub = Arc::new(FakeHub::default());
        let git = Arc::new(FakeGit::default());
        git.set_tip("staging", Commit::new("8c1f2ab9").unwrap());
        let ctx = Context {
            config: config(),
            repo: PathBuf::from("/repo"),
            machine: MachineId::new("m-1").unwrap(),
            clock: clock.clone(),
            hub: hub.clone(),
            git: git.clone(),
            launcher: Arc::new(FakeLauncher::default()),
            builds: Arc::new(FakeBuilds::default()),
            session: None,
            tree: None,
        };
        let desk = Arc::new(FakeDesk::default());
        let trees = Arc::new(FakeTrees::with_trees(2));
        let claims = Claims::new(clock.clone(), desk.clone(), trees.clone(), db).unwrap();
        let commands = Arc::new(FakeCommands::default());
        claims.watch_commands(commands.clone());
        World {
            clock,
            hub,
            desk,
            git,
            trees,
            commands,
            ctx,
            claims,
        }
    }

    // A method, so that tests read `world.request(...)` beside the world they act on.
    #[allow(clippy::unused_self)]
    pub fn request(&self, verb: &str, args: &[&str], cwd: &str, who: &str) -> Request {
        Request {
            verb: verb.to_owned(),
            args: args.iter().map(|s| (*s).to_owned()).collect(),
            cwd: PathBuf::from(cwd),
            session: Some(session(who)),
            wait_s: 540,
            pid: None,
        }
    }
}
