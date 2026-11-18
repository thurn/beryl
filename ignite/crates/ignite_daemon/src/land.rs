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
//!    it through the gate, so it cannot lose again. The lease is renewed once more just before
//!    the push; if the hub says another landing holds it now, the landing starts over.
//! 6. A record of the landing is written to disk, and the rebased commit is pushed from the
//!    gate's tree. A push the remote refuses because the tip moved deletes the record and goes
//!    back to step 2. A remote that cannot be reached exits with code 4 and keeps the record,
//!    because the push may have landed before the connection failed; the next landing tells
//!    which from whether the record's commit is in the tip's history. A refusal that the tip
//!    moving does not explain (a hook, a protected branch) fails the landing at once, deletes
//!    its record and releases the lease, and a landing gives up after [`MAX_PASSES`] passes.
//!    Records that a landing left `pushing` are settled by the daemon's sender (see
//!    [`resolve`]).
//! 7. If the release branch is configured and the landed commits changed Ignite's own source,
//!    the commit also moves the release branch once its family's checks have passed (see
//!    [`release`]). This never fails the landing.
//! 8. The hub is told what landed, which closes the ticket. If it cannot be reached, the record
//!    stays on disk and the ticket closes when the hub returns (see [`records`]). The ticket
//!    tree is released and the branch its work was saved to at a release is deleted.
//!
//! `ignite sync` does step 1's checks and step 2 alone.
//!
//! Each step is recorded as it starts (see [`crate::steps`]), so `ignite wait`, `ignite status`
//! and the client of `ignite land` can say what the landing is doing. `ignite stop` ends a
//! landing up to the moment its push begins, and is refused while the push runs and after it
//! succeeds. A push the remote rejects lets the landing be stopped again.

pub mod env;
mod lease;
pub mod records;
mod release;
mod resolve;

#[cfg(test)]
mod tests;

use std::path::Path;
use std::sync::Arc;

use serde_json::json;

use ignite_core::ids::{Commit, TicketId};
use ignite_core::protocol::Class;
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::protocol::hub::Landed;
use ignite_core::protocol::tickets::{LandedWork, LeaseOp, LeaseReply};
use ignite_core::traits::{Error, Push, Rebase};

use crate::checks::verbs::{built_lines, gate_lines, render};
use crate::checks::{GateRun, RunOptions};
use crate::server::{Context, Verb, error_reply};
use crate::steps;
use env::{GateTree, LandGate, LandTrees, TicketTree};
use lease::{HeldLease, LEASE_TTL_S};
use records::{LandingRecord, RecordState, Records};

/// The longest wait between two tries for a busy lease.
const BUSY_WAIT_S: f64 = 10.0;
/// Races lost before a landing holds the lease through its gate.
const LOSSES_BEFORE_HOLDING: u32 = 3;

/// The passes after which a landing gives up, so a branch that never gets a push through
/// (the tip keeps moving, or the remote keeps answering that it moved) ends with a message.
const MAX_PASSES: u32 = 10;

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
    pub fn deliver_pending(&self, ctx: &Context) {
        self.resolve_pushing(ctx);
        if let Err(error) = self.records.deliver_all(ctx.hub.as_ref()) {
            eprintln!("ignite: landings not recorded at the hub: {error}");
        }
    }

    fn land(&self, ctx: &Context, req: &Request, args: &LandArgs) -> Reply {
        self.deliver_pending(ctx);
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
        // What the landing builds and runs is `land` work for its ticket (see `crate::steps`).
        steps::set_class(Class::Land);
        steps::set_ticket(&ticket, Some(&session));

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
        let mut last_cause = String::new();
        let mut first_landed_since = None;
        loop {
            if passes >= MAX_PASSES {
                return Err(stop(
                    Exit::Conflict,
                    &[
                        line(
                            "failed",
                            format!(
                                "gave up after {MAX_PASSES} passes: {staging} moved or refused the \
                                 push every time (last: {last_cause})"
                            ),
                        ),
                        line("next", "wait a minute, then run ignite land again"),
                    ],
                ));
            }
            let tip = ctx.git.tip(&staging).map_err(io)?;
            if let Some(mut record) = interrupted.take() {
                // A push that the daemon began before it stopped landed only if its commit is
                // in the tip's history. An empty rebase does not show that: it is as empty when
                // the ticket's changes reached the branch some other way and the push never did.
                if self
                    .trees
                    .is_ancestor(&held.path, &record.commit, &tip)
                    .map_err(io)?
                {
                    record.state = RecordState::Pushed;
                    self.records.write(&record).map_err(io)?;
                    return Ok(self.finish(ctx, req, &held, &record, None, None));
                }
                // The interrupted push never landed: start again from nothing.
                self.records.remove(&record.landed.ticket).map_err(io)?;
            }
            steps::step(format!("rebasing onto {staging} {}", short(&tip)));
            let rebased = Self::rebase(ctx, &held, &tip)?;
            first_landed_since.get_or_insert(rebased.landed_since);
            if rebased.head == rebased.onto {
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
            passes += 1;
            let landing_tree = if rebased.landed_since == 0 {
                None
            } else {
                steps::step("taking a landing tree");
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
            steps::step(format!("running the gate in {}", gate_tree.tree));
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
            if steps::stop_requested() {
                free_landing(None);
                return Err(stopped());
            }
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
            // A lease that expired while the gate ran, or that another landing took over, is not
            // this landing's any more: pushing now could race the new holder.
            if lease.as_ref().is_some_and(|held| !held.still_held()) {
                free_landing(None);
                last_cause = format!("the {staging} lease was lost to another landing");
                lease = None;
                continue;
            }
            let now = ctx.git.tip(&staging).map_err(fail)?;
            if now != tip {
                free_landing(None);
                last_cause = format!("the tip moved from {} to {}", short(&tip), short(&now));
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
                written_at: Some(ctx.clock.now()),
                tree: Some(held.tree.clone()),
            };
            // A push cut short may or may not have reached the remote, so from here on the
            // landing refuses `ignite stop`, `ignite close` and `ignite release`.
            if !steps::refuse_stop(&format!("its push to {staging} has begun")) {
                free_landing(None);
                return Err(stopped());
            }
            // The daemon does not restart into a new build from the record's write until the
            // push has been recorded either way.
            let critical = steps::critical();
            self.records.write(&record).map_err(fail)?;
            steps::step(format!("pushing {} to {staging}", short(&rebased.head)));
            let pushed = ctx.git.push(&gate_tree.path, &rebased.head, &staging);
            match pushed {
                Ok(Push::Pushed { .. }) => {
                    record.state = RecordState::Pushed;
                    self.records.write(&record).map_err(fail)?;
                    drop(critical);
                }
                // Nothing was pushed, so the landing may be stopped again while it goes back
                // to rebase.
                Ok(Push::Rejected { reason }) => {
                    steps::allow_stop();
                    last_cause = reason;
                    self.records
                        .remove_if(&ticket, &record.commit)
                        .map_err(fail)?;
                    free_landing(None);
                    losses += 1;
                    if losses < LOSSES_BEFORE_HOLDING {
                        lease = None;
                    }
                    continue;
                }
                // The remote answered that it refuses the commit (a hook, a protected branch, a
                // file over its size limit), so nothing was pushed and the record is dropped.
                // The lease is released as the error returns.
                Err(error @ Error::Conflict(_)) => {
                    self.records
                        .remove_if(&ticket, &record.commit)
                        .map_err(fail)?;
                    return Err(fail(error));
                }
                // The push may have reached the remote before the error, so the record stays
                // `pushing`: the next landing, or the daemon's sender, finds the commit on the
                // branch if it did, and closes the ticket without pushing again, or drops the
                // record and starts over if not.
                Err(error) => return Err(fail(error)),
            }
            drop(lease);
            let release = self.move_release(
                ctx,
                &release::Landed {
                    tree: &gate_tree,
                    commit: &rebased.head,
                    files: &record.landed.work.files,
                    gated: &gated,
                    options: &options,
                },
            );
            free_landing(Some(&rebased.head));
            let summary = Summary {
                tip,
                landed_since: first_landed_since.unwrap_or(0),
                passes,
                gated,
                release,
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
        if let Some(release) = summary.and_then(|summary| summary.release.clone()) {
            lines.push(release);
        }
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
        steps::step(format!("taking the {name} lease"));
        loop {
            if steps::stop_requested() {
                return Err(Error::Conflict("the landing was stopped".to_owned()));
            }
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
                    let _waiting =
                        steps::waiting(format!("the {name} lease, which another landing holds"));
                    (self.pause)(wait);
                }
            }
        }
    }
}

/// The reply of a landing that stopped because someone asked it to (see `crate::stop`); the
/// server replaces it with the stopped reply.
#[allow(clippy::unnecessary_box_returns)]
fn stopped() -> Stop {
    stop(
        Exit::Conflict,
        &[line("stopped", "the landing was stopped")],
    )
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
    /// The `release` line, when the landing called for a move of the release branch.
    release: Option<(String, String)>,
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
