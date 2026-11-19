//! The adapters that join modules written against each other's traits: the hub's hand-off
//! routes as [`TicketDesk`], the machine's [`Trees`] as [`TreeDesk`], and the rest of the daemon
//! as the dispatcher's [`Host`].

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, Weak};
use std::time::{Duration, Instant};

use ignite_core::admission::Worked;
use ignite_core::ids::{Commit, MachineId, SessionId, TicketId, TreeId};
use ignite_core::protocol::client::{Exit, Reply};
use ignite_core::protocol::hub::{AdhocClaim, Claim, Close, Release, TicketBrief};
use ignite_core::protocol::tickets::TicketDetail;
use ignite_core::traits::{Error, Hub, Result};
use ignite_git::{push, refs, save};

use crate::claim::{
    self, ClaimRequest, Claims, Leased, Liveness, ReleaseRequest, Resumed, Saved, Target,
    TicketDesk, TreeDesk,
};
use crate::dispatchd::{Host, SessionCommand};
use crate::handles::Handles;
use crate::hubclient::HubClient;
use crate::sched::Scheduler;
use crate::server::{Context, Server, error_reply};
use crate::trees::{self, Kind, LeaseRequest, TreeLease, Trees};

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// How long a waiting `ignite start` may go without any client waiting on its handle before the
/// daemon stops waiting for a tree on its behalf.
const WAITER_ABANDONED_AFTER: Duration = Duration::from_mins(10);
/// How often a waiting lease checks for a served tree and for a client that went away.
const WAITER_POLL: Duration = Duration::from_secs(30);
/// How long a tree held for a waiting `ignite start` waits for the client's next try before it
/// goes back to the free trees.
const SERVED_HELD_FOR: Duration = Duration::from_mins(5);

/// Whether a waiting lease's client has been away for too long to come back: nobody has waited
/// on its handle for `away`.
fn abandoned(away: Duration) -> bool {
    away >= WAITER_ABANDONED_AFTER
}

/// Trees held for a waiting `ignite start`'s next try, by the ticket and session that asked.
type Served = Arc<Mutex<BTreeMap<(TicketId, SessionId), TreeLease>>>;

/// [`TicketDesk`] over the hub's ticket and hand-off routes.
pub struct HubDesk {
    pub hub: Arc<HubClient>,
    pub machine: MachineId,
}

impl TicketDesk for HubDesk {
    fn detail(&self, ticket: &TicketId) -> Result<TicketDetail> {
        self.hub.ticket(ticket)
    }

    /// The hub has no route for a machine's landed tickets yet, so expected cores fall back to
    /// the planner's figure and the defaults in `admission`.
    fn history(&self, _machine: &MachineId) -> Result<Vec<Worked>> {
        Ok(Vec::new())
    }

    fn claim(&self, claim: &ClaimRequest) -> Result<TicketBrief> {
        match &claim.target {
            Target::Ticket { id, code } => self.hub.claim(&Claim {
                ticket: id.clone(),
                machine: claim.machine.clone(),
                session: claim.session.clone(),
                tree: claim.tree.clone(),
                code: code.clone(),
            }),
            Target::Adhoc(summary) => self.hub.claim_adhoc(&AdhocClaim {
                summary: summary.clone(),
                machine: claim.machine.clone(),
                session: claim.session.clone(),
                tree: claim.tree.clone(),
            }),
        }
    }

    /// Claiming a ticket again from the session that holds it changes nothing at the hub and
    /// answers with its brief.
    fn brief(&self, ticket: &TicketId, session: &SessionId) -> Result<TicketBrief> {
        let detail = self.hub.ticket(ticket)?;
        let tree = detail
            .ticket
            .tree
            .ok_or_else(|| Error::NotFound(format!("{ticket} is held in no tree")))?;
        self.hub.claim(&Claim {
            ticket: ticket.clone(),
            machine: self.machine.clone(),
            session: session.clone(),
            tree,
            code: None,
        })
    }

    fn release(&self, release: &ReleaseRequest) -> Result<()> {
        self.hub.release(&Release {
            ticket: release.ticket.clone(),
            machine: release.machine.clone(),
            session: release.session.clone(),
            reason: release.note.clone(),
            needs: release.needs.clone(),
            branch: Some(release.saved.branch.clone()),
        })
    }

    fn close(
        &self,
        ticket: &TicketId,
        machine: &MachineId,
        session: &SessionId,
        reason: &str,
    ) -> Result<()> {
        self.hub.close(&Close {
            ticket: ticket.clone(),
            machine: machine.clone(),
            session: session.clone(),
            reason: reason.to_owned(),
        })
    }
}

/// [`TreeDesk`] over the machine's [`Trees`] and the remote.
pub struct TreeDeskOver {
    pub trees: Trees,
    pub staging: String,
    /// Set once the server exists: a lease that waits for a tree finishes through a handle.
    pub handles: OnceLock<Arc<Handles>>,
    /// Trees that came free for a waiting `ignite start`, kept for its next try.
    pub served: Served,
}

impl TreeDeskOver {
    pub fn new(trees: Trees, staging: &str) -> TreeDeskOver {
        TreeDeskOver {
            trees,
            staging: staging.to_owned(),
            handles: OnceLock::new(),
            served: Arc::default(),
        }
    }

    fn repo(&self) -> &Path {
        &self.trees.config().repo
    }

    fn remote(&self) -> &str {
        &self.trees.config().remote
    }

    fn path(&self, tree: &ignite_core::ids::TreeId) -> PathBuf {
        self.trees.config().dir.join(tree.as_str())
    }

    /// The ticket's saved branch on the remote, if it has one.
    fn saved_head(&self, ticket: &TicketId) -> Result<Option<Commit>> {
        match refs::remote_tip(self.repo(), self.remote(), &trees::save_branch(ticket)) {
            Ok(commit) => Ok(Some(commit)),
            Err(Error::NotFound(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn resumed(&self, tip: &Commit, saved: &Commit) -> Result<Resumed> {
        let range = format!("{tip}..{saved}");
        let authors = git(self.repo(), &["log", "--format=%an", &range])?;
        let mut authors: Vec<String> = authors.lines().map(str::to_owned).collect();
        let commits = authors.len() as u32;
        authors.sort();
        authors.dedup();
        let last = git(self.repo(), &["log", "-1", "--format=%B", saved.as_str()])?;
        Ok(Resumed {
            commits,
            authors,
            last_saved: last.contains(trees::SAVED_TRAILER),
        })
    }

    fn leased(&self, lease: TreeLease, tip: &Commit, saved: Option<&Commit>) -> Result<Leased> {
        let resumed = saved.map(|saved| self.resumed(tip, saved)).transpose()?;
        Ok(Leased {
            tree: lease.tree,
            path: lease.path,
            branch: lease.branch,
            at: tip.clone(),
            resumed,
        })
    }
}

impl TreeDesk for TreeDeskOver {
    fn set_ticket(&self, tree: &TreeId, ticket: &TicketId) -> Result<()> {
        self.trees.set_ticket(tree, ticket)
    }

    fn lease(
        &self,
        ticket: &TicketId,
        session: &SessionId,
        tip: &Commit,
        compiles: bool,
        files: &[String],
        start: &ignite_core::protocol::client::Request,
    ) -> Result<claim::Lease> {
        let saved = self.saved_head(ticket)?;
        let key = (ticket.clone(), session.clone());
        if let Some(lease) = lock(&self.served).remove(&key) {
            return Ok(claim::Lease::Leased(self.leased(
                lease,
                tip,
                saved.as_ref(),
            )?));
        }
        let request = LeaseRequest {
            ticket: ticket.clone(),
            session: Some(session.clone()),
            branch: ticket.to_string(),
            at: saved.clone().unwrap_or_else(|| tip.clone()),
            tip: tip.clone(),
            compiles,
            files: files.to_vec(),
        };
        let waiter = match self.trees.lease(request)? {
            trees::Lease::Ready(lease) => {
                return Ok(claim::Lease::Leased(self.leased(
                    lease,
                    tip,
                    saved.as_ref(),
                )?));
            }
            trees::Lease::Waiting(waiter) => waiter,
        };
        let handles = self
            .handles
            .get()
            .ok_or_else(|| Error::Invalid("the daemon is still starting".to_owned()))?
            .clone();
        // The handle records the request: a daemon that restarts while the lease waits runs the
        // `start` again under it (`start` is resumable), and that run finds the tree if one was
        // leased to the session meanwhile.
        let handle = handles.create_for("start", Some(session), None, Some(start));
        let (served, finished, trees) = (self.served.clone(), handle.clone(), self.trees.clone());
        let waiting_session = session.clone();
        std::thread::spawn(move || {
            let mut last_seen = Instant::now();
            let reply = loop {
                match waiter.wait(WAITER_POLL) {
                    None => {
                        if handles.waits_of(&waiting_session) > 0 {
                            last_seen = Instant::now();
                        } else if abandoned(last_seen.elapsed()) {
                            // Dropping the waiter takes it off the queue, so a later release
                            // is not spent on a client that has gone.
                            break Reply::text(
                                Exit::Conflict,
                                format!(
                                    "gave up  nobody waited for a tree for {}; run `ignite start \
                                     {}` again",
                                    key.0, key.0
                                ),
                            );
                        }
                    }
                    Some(Err(error)) => break error_reply(&error),
                    Some(Ok(lease)) => {
                        let text = format!(
                            "free     {} is held for {}; run `ignite start {}` again",
                            lease.tree, key.0, key.0
                        );
                        hold_for_next_try(&served, &trees, key, lease, SERVED_HELD_FOR);
                        break Reply::text(Exit::Success, text);
                    }
                }
            };
            handles.finish(&finished, reply);
        });
        Ok(claim::Lease::Waiting(handle))
    }

    fn save(&self, tree: &ignite_core::ids::TreeId, ticket: &TicketId) -> Result<Saved> {
        let path = self.path(tree);
        let message = format!("wip: {ticket} saved at release\n\n{}", trees::SAVED_TRAILER);
        let branch = trees::save_branch(ticket);
        let saved = save::save(&path, self.remote(), &branch, &message)?;
        let base = format!("{}/{}", self.remote(), self.staging);
        let range = format!("{base}..{}", saved.commit);
        let count = git(&path, &["rev-list", "--count", &range])?;
        Ok(Saved {
            branch,
            commits: count.trim().parse().unwrap_or(0),
            uncommitted: saved.uncommitted,
        })
    }

    /// Frees the tree without depending on the network: the tip comes from a fetch if one works,
    /// and otherwise from the last fetch's remote-tracking ref, so a network failure cannot
    /// leave the tree leased.
    fn reset(&self, tree: &ignite_core::ids::TreeId, caller: Option<u32>) -> Result<()> {
        let tip = refs::remote_tip(self.repo(), self.remote(), &self.staging)
            .or_else(|_| refs::cached_tip(self.repo(), self.remote(), &self.staging))?;
        self.trees.release_unsaved(tree, &tip, caller).map(|_| ())
    }

    fn drop_saved(&self, ticket: &TicketId) -> Result<()> {
        if self.saved_head(ticket)?.is_none() {
            return Ok(());
        }
        push::delete(self.repo(), self.remote(), &trees::save_branch(ticket))
    }
}

/// Keeps a tree that came free for a waiting `ignite start` until the client's next try, and
/// gives it back to the free trees if no try collects it within `hold`.
fn hold_for_next_try(
    served: &Served,
    trees: &Trees,
    key: (TicketId, SessionId),
    lease: TreeLease,
    hold: Duration,
) {
    let tree = lease.tree.clone();
    lock(served).insert(key.clone(), lease);
    let (served, trees) = (served.clone(), trees.clone());
    std::thread::spawn(move || {
        std::thread::sleep(hold);
        release_unclaimed(&served, &trees, &key, &tree);
    });
}

/// Gives the tree held for `key` back to the free trees, unless the client collected it (or a
/// later wait replaced it with another tree) in the meantime.
fn release_unclaimed(served: &Served, trees: &Trees, key: &(TicketId, SessionId), tree: &TreeId) {
    let mut held = lock(served);
    if held.get(key).is_some_and(|lease| lease.tree == *tree)
        && let Some(lease) = held.remove(key)
    {
        drop(held);
        trees.abandon(&lease);
    }
}

/// The dispatcher endpoint runs the stall procedure for working sessions itself (see
/// `dispatchd_stall`), so the copy in [`Claims::tick`] is given a dispatcher that never answers
/// its checks, and it never releases anything on its own.
#[derive(Default)]
pub struct QuietDispatcher {
    next: AtomicU64,
}

impl claim::Dispatcher for QuietDispatcher {
    fn check(&self, _session: &SessionId) -> u64 {
        self.next.fetch_add(1, Ordering::Relaxed)
    }

    fn answer(&self, _check: u64) -> Option<Liveness> {
        None
    }

    fn message(&self, _session: &SessionId, _text: &str) {}
}

/// The dispatcher's [`Host`]: the trees, the scheduler, the server and the claims.
pub struct DaemonHost {
    pub trees: Trees,
    pub scheduler: Arc<Scheduler>,
    pub claims: Arc<Claims>,
    /// The daemon's context outside any request, for releases the dispatcher asks for.
    pub ctx: Context,
    /// Set once the server exists.
    pub server: OnceLock<Weak<Server>>,
}

impl DaemonHost {
    fn server(&self) -> Option<Arc<Server>> {
        self.server.get().and_then(Weak::upgrade)
    }

    fn ticket_trees(&self) -> Vec<trees::TreeStatus> {
        self.trees
            .status()
            .map(|status| status.trees)
            .unwrap_or_default()
            .into_iter()
            .filter(|tree| tree.kind == Kind::Ticket)
            .collect()
    }
}

impl Host for DaemonHost {
    fn free_ticket_trees(&self) -> u32 {
        self.ticket_trees()
            .iter()
            .filter(|tree| tree.state == "free")
            .count() as u32
    }

    /// The cores neither Ignite's grants nor outside work are using. A grant that leaves cores
    /// idle lends them, so they count as free.
    fn free_cores(&self) -> f64 {
        let status = self.scheduler.status();
        f64::from(
            status
                .cores
                .saturating_sub(status.committed + status.outside),
        )
    }

    fn holders(&self) -> BTreeMap<SessionId, TicketId> {
        self.ticket_trees()
            .into_iter()
            .filter(|tree| tree.state == "leased")
            .filter_map(|tree| {
                let session = SessionId::new(tree.session?).ok()?;
                let ticket = TicketId::new(tree.ticket?).ok()?;
                Some((session, ticket))
            })
            .collect()
    }

    fn command(&self, session: &SessionId) -> Option<SessionCommand> {
        let command = self.server()?.current_command(session)?;
        let text = match &command.arg {
            Some(arg) => format!("{} {arg}", command.verb),
            None => command.verb.clone(),
        };
        Some(SessionCommand {
            command: text,
            since: command.ended_at.unwrap_or(command.started_at),
            running: command.ended_at.is_none(),
        })
    }

    fn interrupt(&self, session: &SessionId, reply: Reply) -> bool {
        self.server()
            .is_some_and(|server| server.handles().interrupt(session, &reply) > 0)
    }

    fn release(&self, session: &SessionId, ticket: &TicketId, reason: &str) -> Result<()> {
        self.claims
            .release_session(&self.ctx, session, ticket, reason)
    }
}

/// Runs `git` in `dir` and returns its standard output.
fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git").arg("-C").arg(dir).args(args).output()?;
    if !output.status.success() {
        return Err(Error::Io(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_waiting_start_is_given_up_only_after_its_client_has_been_away_a_while() {
        assert!(!abandoned(Duration::from_secs(60)));
        assert!(abandoned(WAITER_ABANDONED_AFTER));
    }
}
