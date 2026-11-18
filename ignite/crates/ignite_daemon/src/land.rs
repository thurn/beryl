//! Landing: `ignite land` and `ignite sync` (see "Landing" in `docs/ignite.md`).
//!
//! `ignite land -m "<note>"` is the only way commits reach the integration branch. In order:
//!
//! 1. The session must hold the tree's ticket, the tree must be on the ticket's branch, and no
//!    file may be neither committed nor ignored.
//! 2. The branch is rebased onto the remote tip. A conflict exits with code 3 and the files.
//! 3. The gate runs in a landing tree checked out at the rebased commit, or in the ticket tree
//!    when the branch was already at the tip.
//! 4. The always set and the gate run there, each check from the result cache where it can be.
//!    On a rebase, only a check that reads both the ticket's change and what landed meanwhile
//!    has a new input hash, so a second pass is mostly cache hits.
//! 5. The hub's lease on the integration branch is taken (180 seconds, renewed while held). If
//!    the tip moved since step 2, the lease is let go and the landing goes back to step 2. A
//!    landing that loses this race three times takes the lease before its next pass and holds
//!    it through the gate, so it cannot lose again.
//! 6. A record of the landing is written to disk, and the rebased commit is pushed from the
//!    gate's tree. A push the remote refuses because the tip moved deletes the record and goes
//!    back to step 2. A remote that cannot be reached exits with code 4 and keeps the record,
//!    because the push may have landed before the connection failed; the next landing tells
//!    which from whether the rebased branch is empty.
//! 7. The hub is told what landed, which closes the ticket. If it cannot be reached, the record
//!    stays on disk and the ticket closes when the hub returns (see [`records`]). The ticket
//!    tree is released and the branch its work was saved to at a release is deleted.
//!
//! `ignite sync` does step 1's checks and step 2 alone.

pub mod env;
pub mod records;

#[cfg(test)]
mod tests;

use std::path::Path;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::json;

use ignite_core::ids::{Commit, TicketId};
use ignite_core::protocol::Class;
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::protocol::hub::Landed;
use ignite_core::protocol::tickets::{LandedWork, LeaseOp, LeaseReply};
use ignite_core::traits::{Error, Hub, Push, Rebase};

use crate::checks::verbs::{built_lines, gate_lines, render};
use crate::checks::{GateRun, RunOptions};
use crate::server::{Context, Verb, error_reply};
use env::{GateTree, LandGate, LandTrees, TicketTree};
use records::{LandingRecord, RecordState, Records};

/// How long the hub keeps the integration branch's lease without a renewal.
const LEASE_TTL_S: u64 = 180;
/// How often a held lease is renewed.
const RENEW_EVERY: Duration = Duration::from_secs(60);
/// The longest wait between two tries for a busy lease.
const BUSY_WAIT_S: f64 = 10.0;
/// Races lost before a landing holds the lease through its gate.
const LOSSES_BEFORE_HOLDING: u32 = 3;

/// The landing machinery that `ignite land` and `ignite sync` share.
pub struct Landing {
    pub trees: Arc<dyn LandTrees>,
    pub gate: Arc<dyn LandGate>,
    pub records: Records,
    /// Waits this many seconds before trying a busy lease again: a sleep in the daemon, a move
    /// of the fake clock in tests.
    pub pause: Arc<dyn Fn(f64) + Send + Sync>,
}

/// `ignite land -m "<note>" [--fresh]`: rebases, runs the gate and lands the commits.
pub struct LandVerb {
    pub landing: Arc<Landing>,
}

impl Verb for LandVerb {
    fn name(&self) -> &'static str {
        "land"
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        let Some(args) = LandArgs::parse(&req.args) else {
            return Reply::text(Exit::Usage, "usage: ignite land -m \"<note>\" [--fresh]");
        };
        self.landing.land(ctx, req, &args)
    }
}

/// `ignite sync`: rebases the tree's branch onto the tip.
pub struct SyncVerb {
    pub landing: Arc<Landing>,
}

impl Verb for SyncVerb {
    fn name(&self) -> &'static str {
        "sync"
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        if !req.args.is_empty() {
            return Reply::text(Exit::Usage, "usage: ignite sync");
        }
        self.landing.sync(ctx)
    }
}

/// The arguments of `ignite land`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct LandArgs {
    note: String,
    fresh: bool,
}

impl LandArgs {
    fn parse(args: &[String]) -> Option<LandArgs> {
        let mut note = None;
        let mut fresh = false;
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-m" | "--message" => note = Some(args.next()?.trim().to_owned()),
                "--fresh" => fresh = true,
                _ => return None,
            }
        }
        let note = note.filter(|n| !n.is_empty())?;
        Some(LandArgs { note, fresh })
    }
}

/// A reply that ends the verb early.
type Stop = Box<Reply>;

#[allow(clippy::unnecessary_box_returns)]
fn stop(exit: Exit, lines: &[(String, String)]) -> Stop {
    Box::new(Reply::text(exit, render(lines)))
}

fn line(label: &str, value: impl Into<String>) -> (String, String) {
    (label.to_owned(), value.into())
}

fn short(commit: &Commit) -> &str {
    let text = commit.as_str();
    &text[..text.len().min(7)]
}

impl Landing {
    /// Tells the hub about landings it has not heard of. The daemon's sender calls this each
    /// round; every `ignite land` calls it first.
    pub fn deliver_pending(&self, hub: &dyn Hub) {
        if let Err(error) = self.records.deliver_all(hub) {
            eprintln!("ignite: landings not recorded at the hub: {error}");
        }
    }

    fn land(&self, ctx: &Context, req: &Request, args: &LandArgs) -> Reply {
        self.deliver_pending(ctx.hub.as_ref());
        self.land_checked(ctx, req, args)
            .unwrap_or_else(|stop| *stop)
    }

    fn sync(&self, ctx: &Context) -> Reply {
        let run = || -> Result<Reply, Stop> {
            let held = self.held_tree(ctx, false)?;
            let staging = &ctx.config.repository.staging;
            let tip = ctx
                .git
                .tip(staging)
                .map_err(|e| Box::new(error_reply(&e)))?;
            let lines = match Self::rebase(ctx, &held, &tip)? {
                Rebased {
                    landed_since: 0, ..
                } => {
                    vec![line(
                        "current",
                        format!("already at {staging} {}", short(&tip)),
                    )]
                }
                Rebased { landed_since, .. } => vec![line(
                    "rebased",
                    format!(
                        "onto {staging} {} ({landed_since} commits landed since)",
                        short(&tip)
                    ),
                )],
            };
            Ok(Reply::text(Exit::Success, render(&lines)))
        };
        run().unwrap_or_else(|stop| *stop)
    }

    /// Step 1: the ticket tree the request came from, held by the asking session and on its
    /// ticket's branch. With `whole`, no file in it may be neither committed nor ignored either;
    /// `ignite sync` leaves that to the rebase, which refuses only edits to tracked files.
    fn held_tree(&self, ctx: &Context, whole: bool) -> Result<TicketTree, Stop> {
        let Some(tree) = &ctx.tree else {
            return Err(stop(Exit::Usage, &[line("failed", "not in a build tree")]));
        };
        let held = self
            .trees
            .ticket_tree(tree)
            .map_err(|e| Box::new(error_reply(&e)))?
            .ok_or_else(|| {
                stop(
                    Exit::Conflict,
                    &[line(
                        "conflict",
                        format!("{tree} is not leased to a ticket"),
                    )],
                )
            })?;
        if held.session.is_none() || held.session != ctx.session {
            let holder = held
                .session
                .as_ref()
                .map_or("no session".to_owned(), |s| format!("session {s}"));
            return Err(stop(
                Exit::Conflict,
                &[line(
                    "conflict",
                    format!("{} is held by {holder}, not this one", held.ticket),
                )],
            ));
        }
        let branch = self
            .trees
            .branch(&held.path)
            .map_err(|e| Box::new(error_reply(&e)))?;
        if let Some(wanted) = &held.branch
            && branch.as_ref() != Some(wanted)
        {
            let on = branch.map_or("a detached HEAD".to_owned(), |b| format!("branch {b}"));
            return Err(stop(
                Exit::Usage,
                &[
                    line("refused", format!("{} is on {on}", held.tree)),
                    line("next", format!("git switch {wanted}, then try again")),
                ],
            ));
        }
        if !whole {
            return Ok(held);
        }
        let stray = self
            .trees
            .uncommitted(&held.path)
            .map_err(|e| Box::new(error_reply(&e)))?;
        if !stray.is_empty() {
            let mut lines = vec![line(
                "refused",
                format!("{} files are neither committed nor ignored", stray.len()),
            )];
            lines.extend(stray.iter().take(10).map(|path| line("", path.clone())));
            if stray.len() > 10 {
                lines.push(line("", format!("and {} more", stray.len() - 10)));
            }
            lines.push(line(
                "next",
                "commit them, or move scratch output to your session's scratch folder",
            ));
            return Err(stop(Exit::Usage, &lines));
        }
        Ok(held)
    }

    /// Step 2: rebases the ticket's branch onto `tip`.
    fn rebase(ctx: &Context, held: &TicketTree, tip: &Commit) -> Result<Rebased, Stop> {
        match ctx.git.rebase_onto(&held.path, tip) {
            Ok(Rebase::Clean {
                onto,
                head,
                landed_since,
            }) => Ok(Rebased {
                onto,
                head,
                landed_since,
            }),
            Ok(Rebase::Conflict { files }) => {
                let repository = &ctx.config.repository;
                let mut lines = vec![line(
                    "conflict",
                    format!(
                        "rebasing onto {} {} conflicts in {} files",
                        repository.staging,
                        short(tip),
                        files.len()
                    ),
                )];
                lines.extend(files.iter().take(10).map(|f| line("", f.clone())));
                lines.push(line(
                    "next",
                    format!(
                        "git rebase {}/{}, resolve them, git rebase --continue, then try again",
                        repository.remote, repository.staging
                    ),
                ));
                Err(stop(Exit::Conflict, &lines))
            }
            Err(error) => Err(Box::new(error_reply(&error))),
        }
    }

    /// The whole landing. `Err` is any reply but a landing.
    #[allow(clippy::too_many_lines)]
    fn land_checked(&self, ctx: &Context, req: &Request, args: &LandArgs) -> Result<Reply, Stop> {
        let held = self.held_tree(ctx, true)?;
        let ticket = held.ticket.clone();
        let session = held.session.clone().ok_or_else(|| {
            stop(
                Exit::Conflict,
                &[line("conflict", "no session holds the ticket")],
            )
        })?;
        let staging = ctx.config.repository.staging.clone();
        let holder = format!("{}:{ticket}", ctx.machine);
        let io = |e: Error| Box::new(error_reply(&e));

        // A landing already pushed but not yet told to the hub is never pushed again.
        let mut interrupted = None;
        match self.records.get(&ticket).map_err(io)? {
            Some(record) if record.state == RecordState::Pushed => {
                return Ok(self.finish(ctx, req, &held, &record, None, None));
            }
            Some(record) => interrupted = Some(record),
            None => {}
        }

        let mut lease: Option<HeldLease> = None;
        let mut losses = 0;
        let mut passes = 0;
        let mut first_landed_since = None;
        loop {
            let tip = ctx.git.tip(&staging).map_err(io)?;
            let rebased = Self::rebase(ctx, &held, &tip)?;
            first_landed_since.get_or_insert(rebased.landed_since);
            if rebased.head == rebased.onto {
                // Nothing is left to land. A push that the daemon began before it stopped may
                // have reached the remote, which the empty branch shows.
                if let Some(mut record) = interrupted.take() {
                    record.state = RecordState::Pushed;
                    self.records.write(&record).map_err(io)?;
                    return Ok(self.finish(ctx, req, &held, &record, None, None));
                }
                return Err(stop(
                    Exit::Usage,
                    &[
                        line(
                            "failed",
                            format!("nothing to land: {staging} holds every commit"),
                        ),
                        line("next", "ignite close -m \"<reason>\" closes the ticket"),
                    ],
                ));
            }
            if let Some(record) = interrupted.take() {
                // The interrupted push never landed: start again from nothing.
                self.records.remove(&record.landed.ticket).map_err(io)?;
            }
            passes += 1;
            let landing_tree = if rebased.landed_since == 0 {
                None
            } else {
                self.trees
                    .lease_landing(&holder, &rebased.head)
                    .map_err(io)?
            };
            let gate_tree = landing_tree.clone().unwrap_or_else(|| GateTree {
                tree: held.tree.clone(),
                path: held.path.clone(),
            });
            let options = RunOptions {
                verb: "land".to_owned(),
                class: Class::Land,
                rerun: false,
                ticket: Some(ticket.clone()),
                session: Some(session.clone()),
            };
            let gated = self.gate.run(ctx, &gate_tree, &options);
            let free_landing = |built: Option<&Commit>| {
                if let Some(tree) = &landing_tree
                    && let Err(error) = self.trees.release_landing(&tree.tree, built)
                {
                    eprintln!("ignite: landing tree {} not freed: {error}", tree.tree);
                }
            };
            let fail = |error: Error| {
                free_landing(None);
                io(error)
            };
            if !gated.run.passed() {
                free_landing(None);
                return Err(Box::new(failed_gate(
                    &staging,
                    &rebased,
                    &gated,
                    &gate_tree.path,
                )));
            }

            if lease.is_none() {
                lease = Some(self.acquire(ctx, &holder, &ticket).map_err(fail)?);
            }
            let now = ctx.git.tip(&staging).map_err(fail)?;
            if now != tip {
                free_landing(None);
                losses += 1;
                if losses < LOSSES_BEFORE_HOLDING {
                    lease = None;
                }
                continue;
            }

            let work = LandedWork {
                commits: self
                    .trees
                    .commits(&held.path, &rebased.onto, &rebased.head)
                    .map_err(fail)?,
                files: ctx
                    .git
                    .changed(&rebased.onto, &rebased.head)
                    .map_err(fail)?,
                note: args.note.clone(),
            };
            let mut record = LandingRecord {
                landed: Landed {
                    ticket: ticket.clone(),
                    machine: ctx.machine.clone(),
                    session: session.clone(),
                    work,
                    fresh: args.fresh,
                },
                commit: rebased.head.clone(),
                state: RecordState::Pushing,
            };
            self.records.write(&record).map_err(fail)?;
            match ctx.git.push(&gate_tree.path, &rebased.head, &staging) {
                Ok(Push::Pushed { .. }) => {
                    record.state = RecordState::Pushed;
                    self.records.write(&record).map_err(fail)?;
                }
                Ok(Push::Rejected { .. }) => {
                    self.records.remove(&ticket).map_err(fail)?;
                    free_landing(None);
                    losses += 1;
                    if losses < LOSSES_BEFORE_HOLDING {
                        lease = None;
                    }
                    continue;
                }
                // The push may have reached the remote before the error, so the record stays
                // `pushing`: the next landing finds the branch empty if it did, and closes the
                // ticket without pushing again, or drops the record and starts over if not.
                Err(error) => return Err(fail(error)),
            }
            drop(lease);
            free_landing(Some(&rebased.head));
            let summary = Summary {
                tip,
                landed_since: first_landed_since.unwrap_or(0),
                passes,
                gated,
            };
            return Ok(self.finish(ctx, req, &held, &record, Some(&summary), Some(&args.note)));
        }
    }

    /// Step 7: tells the hub, releases the ticket tree and deletes the saved branch.
    fn finish(
        &self,
        ctx: &Context,
        req: &Request,
        held: &TicketTree,
        record: &LandingRecord,
        summary: Option<&Summary>,
        note: Option<&str>,
    ) -> Reply {
        let ticket = &record.landed.ticket;
        let closed = match self.records.deliver(ctx.hub.as_ref(), record) {
            Ok(()) => "ticket closed".to_owned(),
            Err(Error::Unreachable(_)) => "the ticket closes when the hub returns".to_owned(),
            Err(error) => format!("the hub refused the record ({error}); it is kept"),
        };
        let mut problems = Vec::new();
        let released = match self
            .trees
            .release_ticket(&held.tree, &record.commit, req.pid)
        {
            Ok(()) => format!("{} released", held.tree),
            Err(error) => {
                problems.push(line(
                    "warning",
                    format!("{} not released: {error}", held.tree),
                ));
                format!("{} still leased", held.tree)
            }
        };
        if let Err(error) = self.trees.delete_saved(ticket) {
            problems.push(line(
                "warning",
                format!("the saved branch of {ticket} was not deleted: {error}"),
            ));
        }
        let staging = &ctx.config.repository.staging;
        let mut lines = Vec::new();
        if let Some(summary) = summary {
            lines.push(line(
                "rebased",
                format!(
                    "onto {staging} {} ({} commits landed since start)",
                    short(&summary.tip),
                    summary.landed_since
                ),
            ));
            if summary.passes > 1 {
                lines.push(line(
                    "raced",
                    format!(
                        "the tip moved during {} passes; landed on pass {}",
                        summary.passes - 1,
                        summary.passes
                    ),
                ));
            }
            lines.extend(built_lines(&summary.gated.run));
            lines.extend(gate_lines(&summary.gated));
        }
        lines.push(line(
            "landed",
            format!(
                "{} on {staging}; {closed}; {released}",
                short(&record.commit)
            ),
        ));
        lines.extend(problems);
        lines.push(line(
            "title",
            format!("✅ [{ticket}] and the summary that ignite start gave"),
        ));
        lines.push(line("next", "nothing; wait for your next ticket"));
        Reply {
            exit: Exit::Success,
            text: render(&lines),
            json: Some(json!({
                "ticket": ticket,
                "commit": record.commit,
                "note": note,
                "passes": summary.map(|s| s.passes),
                "from_cache": summary.map(|s| s.gated.run.from_cache),
                "ran": summary.map(|s| s.gated.run.ran),
                "left_to_tip": summary.map(|s| s.gated.left),
                "closed": closed == "ticket closed",
            })),
            handle: None,
        }
    }

    /// Takes the integration branch's lease, waiting while another landing holds it. A holder
    /// that died is waited out: its lease expires within [`LEASE_TTL_S`] seconds.
    fn acquire(&self, ctx: &Context, holder: &str, ticket: &TicketId) -> Result<HeldLease, Error> {
        let name = ctx.config.repository.staging.clone();
        loop {
            let op = LeaseOp::Acquire {
                holder: holder.to_owned(),
                ttl_s: LEASE_TTL_S,
                payload: Some(json!({ "ticket": ticket, "machine": ctx.machine })),
                no_queue: true,
            };
            match ctx.hub.lease(&name, op)? {
                LeaseReply::Held(_) => {
                    return Ok(HeldLease::start(ctx.hub.clone(), name, holder.to_owned()));
                }
                LeaseReply::Busy(busy) => {
                    let wait = busy.expires_at.map_or(BUSY_WAIT_S, |at| {
                        at.seconds_since(ctx.clock.now()).clamp(1.0, BUSY_WAIT_S)
                    });
                    (self.pause)(wait);
                }
            }
        }
    }
}

/// A clean rebase.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Rebased {
    onto: Commit,
    head: Commit,
    landed_since: u32,
}

/// What a landing did before step 7, for its output.
struct Summary {
    tip: Commit,
    landed_since: u32,
    passes: u32,
    gated: GateRun,
}

/// The reply to a gate that failed: exit code 2, the failures with paths relative to the
/// repository, and a rerun command that works in the ticket tree.
fn failed_gate(staging: &str, rebased: &Rebased, gated: &GateRun, gate_path: &Path) -> Reply {
    let mut lines = vec![line(
        "rebased",
        format!("onto {staging} {}", short(&rebased.onto)),
    )];
    lines.extend(built_lines(&gated.run));
    lines.extend(gate_lines(gated));
    let rerun = gated.run.failures.first().map_or_else(
        || "ignite check".to_owned(),
        |failure| format!("ignite test {}", failure.check),
    );
    lines.push(line("rerun", rerun));
    lines.push(line(
        "next",
        "fix it in your tree; nothing landed and the tree stays rebased",
    ));
    let prefix = format!("{}/", gate_path.display());
    let text = render(&lines).replace(&prefix, "");
    Reply {
        exit: Exit::CheckFailed,
        text,
        json: Some(json!({
            "failures": gated.run.failures.iter().map(|f| json!({
                "check": f.check, "outcome": f.outcome, "at": f.at, "message": f.message,
            })).collect::<Vec<_>>(),
        })),
        handle: None,
    }
}

/// The integration branch's lease while this landing holds it. A thread renews it every
/// [`RENEW_EVERY`]; dropping the guard stops the thread and releases the lease.
struct HeldLease {
    hub: Arc<dyn Hub>,
    name: String,
    holder: String,
    stop: Arc<(Mutex<bool>, Condvar)>,
    renewer: Option<JoinHandle<()>>,
}

impl HeldLease {
    fn start(hub: Arc<dyn Hub>, name: String, holder: String) -> HeldLease {
        let stop = Arc::new((Mutex::new(false), Condvar::new()));
        let renewer = {
            let (hub, name, holder, stop) =
                (hub.clone(), name.clone(), holder.clone(), stop.clone());
            std::thread::spawn(move || {
                let (stopped, wake) = &*stop;
                let mut guard = stopped.lock().unwrap_or_else(PoisonError::into_inner);
                loop {
                    let (next, _) = wake
                        .wait_timeout_while(guard, RENEW_EVERY, |stopped| !*stopped)
                        .unwrap_or_else(PoisonError::into_inner);
                    guard = next;
                    if *guard {
                        return;
                    }
                    let op = LeaseOp::Renew {
                        holder: holder.clone(),
                        ttl_s: LEASE_TTL_S,
                        payload: None,
                        if_revision: None,
                    };
                    if let Err(error) = hub.lease(&name, op) {
                        eprintln!("ignite: the {name} lease was not renewed: {error}");
                    }
                }
            })
        };
        HeldLease {
            hub,
            name,
            holder,
            stop,
            renewer: Some(renewer),
        }
    }
}

impl Drop for HeldLease {
    fn drop(&mut self) {
        let (stopped, wake) = &*self.stop;
        *stopped.lock().unwrap_or_else(PoisonError::into_inner) = true;
        wake.notify_all();
        if let Some(renewer) = self.renewer.take() {
            let _ = renewer.join();
        }
        let op = LeaseOp::Release {
            holder: self.holder.clone(),
            if_revision: None,
        };
        if let Err(error) = self.hub.lease(&self.name, op) {
            eprintln!("ignite: the {} lease was not released: {error}", self.name);
        }
    }
}
