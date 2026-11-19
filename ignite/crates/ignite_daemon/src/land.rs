//! Landing: `ignite land` and `ignite sync` (see "Landing" in `docs/ignite.md`).
//!
//! `ignite land` is the only way commits reach the integration branch. In order:
//!
//! 1. The session must hold the tree's ticket, the tree must be on the ticket's branch, and no
//!    file may be neither committed nor ignored.
//! 2. The branch is rebased onto the remote tip. A conflict exits with code 3 and the files.
//!    On the first pass, the formatters of `[format]` run on the changed files in the ticket
//!    tree, and what they change is committed on the branch (see [`format`]).
//! 3. The gate runs in a landing tree checked out at the rebased commit, or in the ticket tree
//!    when the branch was already at the tip. A landing that changes only Ignite runs the
//!    express commands in the express tree instead (see [`express`]). Every later pass gates in
//!    the tree the first pass chose: a landing tree stays leased to the landing until it ends
//!    and is checked out at each new rebased commit, so it builds only what changed (see
//!    [`passes`]).
//!    When the gate's only faults are checks that fail at the tip too, it runs again with them
//!    known to fail, after the hub has filed their regressions (see [`base`]).
//! 4. The always set and the gate run there, each check from the result cache where it can be.
//!    A pass after one whose gate passed judges again only the families whose inputs the
//!    commits landed since touch, and pushes without running anything when they touch none.
//!    An express pass after one whose commands passed runs them again only when the commits
//!    landed since touch the change's own files or the workspace's build inputs.
//! 5. The hub's lease on the integration branch is taken (180 seconds, renewed while held), only
//!    after the gate has passed. If the tip moved since step 2, the lease is let go and the
//!    landing goes back to step 2. The lease is never held through a gate: a landing that lost
//!    three races used to take it before its next pass and hold it through the gate, and then
//!    every other landing waited for that gate, even those whose own gates had passed and only
//!    needed to push, so landings ran one at a time for the length of a gate each (15 minutes
//!    and more under load). The lease is renewed once more just before the push; if the hub
//!    says another landing holds it now, the landing starts over.
//! 6. A record of the landing is written to disk, and the rebased commit is pushed from the
//!    gate's tree. A push the remote refuses because the tip moved deletes the record and goes
//!    back to step 2. A remote that cannot be reached exits with code 4 and keeps the record,
//!    because the push may have landed before the connection failed; the next landing tells
//!    which from whether the record's commit is in the tip's history. A refusal that the tip
//!    moving does not explain (a hook, a protected branch) fails the landing at once, deletes
//!    its record and releases the lease, and a landing gives up after [`MAX_PASSES`] passes.
//!    Records that a landing left `pushing` are settled by the daemon's sender (see
//!    [`resolve`]).
//! 7. The hub is told what landed, which closes the ticket. If it cannot be reached, the record
//!    stays on disk and the ticket closes when the hub returns (see [`records`]). The ticket
//!    tree is released and the branch its work was saved to at a release is deleted.
//! 8. If the release branch is configured and the landed commits changed Ignite's own source,
//!    the commit also moves the release branch, if the release family's checks passed. That
//!    was decided after step 4 and before the lease, running the family there if the gate did
//!    not run all of it, so only a push is left here (see [`release`]). This never fails the
//!    landing.
//!
//! `ignite sync` does step 1's checks and step 2 alone.
//!
//! Each step is recorded as it starts (see [`crate::steps`]), so `ignite wait`, `ignite status`
//! and the client of `ignite land` can say what the landing is doing. `ignite stop` ends a
//! landing up to the moment its push begins, and is refused while the push runs and after it
//! succeeds. A push the remote rejects lets the landing be stopped again.

mod base;
pub mod env;
pub mod express;
pub mod format;
mod lease;
mod passes;
pub mod records;
mod release;
mod replies;
mod resolve;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use serde_json::json;

use ignite_core::ids::{Commit, TicketId};
use ignite_core::protocol::Class;
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::protocol::hub::Landed;
use ignite_core::protocol::tickets::{LandedWork, LeaseOp, LeaseReply};
use ignite_core::traits::{Error, Push, Rebase};

use crate::checks::verbs::render;
use crate::checks::{GateRun, RunOptions};
use crate::server::{Context, Verb, error_reply};
use crate::steps;
use crate::update::prebuild;
use env::{GateTree, LandGate, LandTrees, TicketTree};
use express::{ExpressLane, ExpressRun, Expressed};
use format::{Formatted, Formatter};
use lease::{HeldLease, LEASE_TTL_S};
use passes::{GateHome, Passed};
use records::{LandingRecord, RecordState, Records};
use replies::{failed_gate, stopped};

/// The longest wait between two tries for a busy lease.
const BUSY_WAIT_S: f64 = 10.0;

/// The passes after which a landing gives up, so a branch that never gets a push through
/// (the tip keeps moving, or the remote keeps answering that it moved) ends with a message.
const MAX_PASSES: u32 = 10;

/// The landing machinery that `ignite land` and `ignite sync` share.
pub struct Landing {
    pub trees: Arc<dyn LandTrees>,
    pub gate: Arc<dyn LandGate>,
    pub records: Records,
    /// The machine's express lane; `None` sends every landing through the gate.
    pub express: Option<Arc<dyn ExpressLane>>,
    /// The repository's formatters; `None` formats nothing.
    pub format: Option<Arc<dyn Formatter>>,
    /// Waits this many seconds before trying a busy lease again: a sleep in the daemon, a move
    /// of the fake clock in tests.
    pub pause: Arc<dyn Fn(f64) + Send + Sync>,
}

/// `ignite land [-m "<note>"] [--fresh]`: rebases, runs the gate and lands the commits. Without
/// a note, the landed commits' subjects, one per line, are the note.
pub struct LandVerb {
    pub landing: Arc<Landing>,
}

impl Verb for LandVerb {
    fn name(&self) -> &'static str {
        "land"
    }

    /// A landing rebases, builds and pushes its tree, so nothing else may change the tree
    /// while it runs, and a second landing of it is refused.
    fn holds_the_tree(&self) -> bool {
        true
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        let Some(args) = LandArgs::parse(&req.args) else {
            return Reply::text(Exit::Usage, "usage: ignite land [-m \"<note>\"] [--fresh]");
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
    /// The explicit note; `None` when the note is the landed commits' subjects.
    note: Option<String>,
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
        if note.as_ref().is_some_and(|n: &String| n.is_empty()) {
            return None;
        }
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
                return Ok(self.finish(ctx, req, &held, &record));
            }
            Some(record) => interrupted = Some(record),
            None => {}
        }

        let mut lease: Option<HeldLease> = None;
        let mut home = GateHome::new(self.trees.clone());
        let mut last_passed: Option<Passed> = None;
        let mut last_expressed: Option<Expressed> = None;
        let mut passes = 0;
        let mut last_cause = String::new();
        let mut first_landed_since = None;
        let mut formatted: Option<Formatted> = None;
        // Checks found failing at the tip as well, left out of every later gate (see `base`).
        let mut excused = crate::checks::Excused::new();
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
                    return Ok(self.finish(ctx, req, &held, &record));
                }
                // The interrupted push never landed: start again from nothing.
                self.records.remove(&record.landed.ticket).map_err(io)?;
            }
            steps::step(format!("rebasing onto {staging} {}", short(&tip)));
            let mut rebased = Self::rebase(ctx, &held, &tip)?;
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
            let mut changed = ctx.git.changed(&rebased.onto, &rebased.head).map_err(io)?;
            if formatted.is_none() {
                formatted = self.format_changes(ctx, &held, &mut rebased, &mut changed)?;
            }
            let format_onto = |reply: Reply| match &formatted {
                Some(done) => Box::new(done.onto(reply)),
                None => Box::new(reply),
            };
            let express = self.express_for(ctx, &held.path, &changed);
            prebuild::offer(ctx, &held.path, &rebased.head, express.is_some());
            // An express landing runs its commands in its own ticket tree, which the rebase has
            // put at the rebased commit, so express landings run side by side. They once all ran
            // in one shared tree under one lock, and on 2026-11-18 eight of them queued behind
            // each other for up to 32 minutes: each one that lost the race for the tip while it
            // waited ran its commands again.
            let gate_tree = match express {
                Some(_) => GateTree {
                    tree: held.tree.clone(),
                    path: held.path.clone(),
                },
                None => home
                    .for_pass(&holder, &rebased.head, rebased.landed_since, &held)
                    .map_err(io)?,
            };
            let options = RunOptions {
                verb: "land".to_owned(),
                class: Class::Land,
                rerun: false,
                ticket: Some(ticket.clone()),
                session: Some(session.clone()),
            };
            let (gated, expressed) = if let Some((lane, section)) = &express {
                // A pass after one whose commands passed, or after `ignite check` passed them in
                // this tree, runs them again only when the commits landed since touch what they
                // depend on (see `express::Expressed` and `express::reused`).
                let carried = match &last_expressed {
                    Some(earlier) => earlier.carried(ctx, &rebased.head, &changed),
                    None => express::reused(
                        *lane,
                        ctx.git.as_ref(),
                        section,
                        &held.path,
                        &rebased.head,
                        &changed,
                    ),
                };
                let run = carried
                    .map_err(io)?
                    .unwrap_or_else(|| lane.run_in(&held.path, section, &changed));
                (GateRun::default(), Some(run))
            } else {
                // A pass after one whose gate passed judges again only what the commits landed
                // since can change (see `passes`).
                let carried = match &last_passed {
                    Some(earlier) => earlier.carried(ctx, &rebased.head, &changed).map_err(io)?,
                    None => None,
                };
                steps::step(format!("running the gate in {}", gate_tree.tree));
                let pass = base::Pass {
                    holder: &holder,
                    tree: &gate_tree,
                    borrowable: gate_tree.tree != held.tree,
                    rebased: &rebased,
                    options: &options,
                    carried: carried.as_ref(),
                    earlier: last_passed.as_ref(),
                };
                (self.gate_pass(ctx, &pass, &mut excused), None)
            };
            // The landing tree, if the landing holds one, stays leased while the landing goes
            // back to rebase, and is freed when `home` is dropped as the landing ends.
            if steps::stop_requested() {
                return Err(stopped());
            }
            if let Some(run) = expressed.as_ref().filter(|run| !run.passed()) {
                return Err(format_onto(express::failed_express(
                    express
                        .as_ref()
                        .map_or("", |(_, section)| section.dir.as_str()),
                    run,
                )));
            }
            if !gated.run.passed() {
                return Err(format_onto(failed_gate(
                    &staging,
                    &rebased,
                    &gated,
                    &gate_tree.path,
                )));
            }
            // Whether the commit may move the release branch is settled now, running the
            // release family if the gate did not, so nothing but a push is left after the
            // landing's own push (see `release`).
            let mut gated = gated;
            let verdict = self.judge_release(
                ctx,
                &gate_tree,
                &changed,
                &mut gated,
                &options,
                expressed.is_some(),
            );
            if steps::stop_requested() {
                return Err(stopped());
            }
            match &expressed {
                Some(run) => {
                    last_expressed = Some(Expressed::new(
                        rebased.head.clone(),
                        changed.clone(),
                        run.clone(),
                    ));
                }
                None => {
                    last_passed = Some(Passed::new(
                        rebased.head.clone(),
                        changed.clone(),
                        gated.clone(),
                    ));
                }
            }

            if lease.is_none() {
                lease = Some(self.acquire(ctx, &holder, &ticket).map_err(io)?);
            }
            // A lease that expired while the gate ran, or that another landing took over, is not
            // this landing's any more: pushing now could race the new holder.
            if lease.as_ref().is_some_and(|held| !held.still_held()) {
                last_cause = format!("the {staging} lease was lost to another landing");
                lease = None;
                continue;
            }
            let now = ctx.git.tip(&staging).map_err(io)?;
            if now != tip {
                last_cause = format!("the tip moved from {} to {}", short(&tip), short(&now));
                lease = None;
                continue;
            }

            let note = match &args.note {
                Some(note) => note.clone(),
                None => self
                    .trees
                    .subjects(&held.path, &rebased.onto, &rebased.head)
                    .map_err(io)?
                    .join("\n"),
            };
            let work = LandedWork {
                commits: self
                    .trees
                    .commits(&held.path, &rebased.onto, &rebased.head)
                    .map_err(io)?,
                files: changed.clone(),
                note: note.clone(),
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
                return Err(stopped());
            }
            // The daemon does not restart into a new build from the record's write until the
            // push has been recorded either way.
            let critical = steps::critical();
            self.records.write(&record).map_err(io)?;
            steps::step(format!("pushing {} to {staging}", short(&rebased.head)));
            let pushed = ctx.git.push(&gate_tree.path, &rebased.head, &staging);
            match pushed {
                Ok(Push::Pushed { .. }) => {
                    record.state = RecordState::Pushed;
                    self.records.write(&record).map_err(io)?;
                    drop(critical);
                }
                // Nothing was pushed, so the landing may be stopped again while it goes back
                // to rebase.
                Ok(Push::Rejected { reason }) => {
                    steps::allow_stop();
                    last_cause = reason;
                    self.records
                        .remove_if(&ticket, &record.commit)
                        .map_err(io)?;
                    lease = None;
                    continue;
                }
                // The remote answered that it refuses the commit (a hook, a protected branch, a
                // file over its size limit), so nothing was pushed and the record is dropped.
                // The lease is released as the error returns.
                Err(error @ Error::Conflict(_)) => {
                    self.records
                        .remove_if(&ticket, &record.commit)
                        .map_err(io)?;
                    return Err(io(error));
                }
                // The push may have reached the remote before the error, so the record stays
                // `pushing`: the next landing, or the daemon's sender, finds the commit on the
                // branch if it did, and closes the ticket without pushing again, or drops the
                // record and starts over if not.
                Err(error) => return Err(io(error)),
            }
            drop(lease);
            // The ticket closes and its tree is freed before the release branch moves, which
            // needs neither.
            let settled = self.settle(ctx, req, &held, &record);
            let release = self.move_release(ctx, &gate_tree, &rebased.head, verdict);
            home.release(Some(&rebased.head));
            let summary = Summary {
                tip,
                landed_since: first_landed_since.unwrap_or(0),
                passes,
                gated,
                release,
                express: expressed,
                formatted,
            };
            return Ok(Self::reply(
                ctx,
                &record,
                settled,
                Some(&summary),
                Some(&note),
            ));
        }
    }

    /// Step 7 for a landing pushed earlier, which has nothing else to report.
    fn finish(
        &self,
        ctx: &Context,
        req: &Request,
        held: &TicketTree,
        record: &LandingRecord,
    ) -> Reply {
        let settled = self.settle(ctx, req, held, record);
        Self::reply(ctx, record, settled, None, None)
    }

    /// Step 7: tells the hub, releases the ticket tree and deletes the saved branch.
    fn settle(
        &self,
        ctx: &Context,
        req: &Request,
        held: &TicketTree,
        record: &LandingRecord,
    ) -> Settled {
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
        Settled {
            closed,
            released,
            problems,
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

/// A clean rebase.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Rebased {
    onto: Commit,
    head: Commit,
    landed_since: u32,
}

/// What step 7 did, for the landing's output.
struct Settled {
    /// What became of the ticket at the hub.
    closed: String,
    /// What became of the ticket tree.
    released: String,
    /// Warnings about what could not be done.
    problems: Vec<(String, String)>,
}

/// What a landing did besides step 7, for its output.
struct Summary {
    tip: Commit,
    landed_since: u32,
    passes: u32,
    gated: GateRun,
    /// The `release` line, when the landing called for a move of the release branch.
    release: Option<(String, String)>,
    /// The express run that took the gate's place, if the landing took the express lane.
    express: Option<ExpressRun>,
    /// What the formatters did, if the machine has any.
    formatted: Option<Formatted>,
}
