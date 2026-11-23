//! Landing by verdict: `[land] mode = "verdict"` (see "Landing by verdict" in `docs/ignite.md`).
//!
//! On 2026-11-19 the median landing through the gate took 7.8 minutes and the slowest 91
//! minutes over 7 passes. Each pass rebased the ticket tree onto the tip, gated the rebased
//! commit, and went back to the rebase when another landing had moved the tip meanwhile, which
//! rebuilt and ran every family whose inputs the newly landed commits touched. None of the 23
//! extra passes that day's lost races caused found a failure.
//!
//! A verdict (see [`crate::verdicts`]) records that the gate passed for one exact tree of files
//! on one base commit. A landing in this mode:
//!
//! 1. Checks the ticket tree as [`Landing::land_checked`] does, settles a record that an earlier
//!    landing left, and runs the formatters on the change.
//! 2. Takes the merge base of the tree's `HEAD` with the tip. A base committed longer than
//!    `max_base_age_hours` ago is replaced: the ticket tree is rebased onto the tip for real.
//! 3. Looks up the verdict for the tree, the base and the `[gate]` section. Without one, it
//!    runs the gate in the ticket tree as it is, comparing a failure with the base (see
//!    [`super::base`]), and stores the verdict when the gate passes. When every failure fails
//!    at a base older than the tip as well, the ticket tree is rebased onto the tip for real
//!    and the gate runs again there, where such a check is filed as a regression and left out.
//! 4. Rebases the change onto the tip in git's object database ([`Git::replay`]), takes the
//!    lease, and pushes the rebased commit. When the tip moves, or the remote rejects the push,
//!    it rebases again in the object database and runs nothing. A conflict exits with code 3.
//! 5. Tells the hub and releases the ticket tree parked at its own `HEAD`, which is what its
//!    builds match: the pushed commit exists only in the object database.
//!
//! A ticket that a tip round reverted (metadata `reverted` at the hub) skips steps 2 and 3: its
//! ticket tree is rebased onto the tip for real and the gate runs there, so the build checks,
//! which are in the always set, judge the rebased code.
//!
//! A change to Ignite's own source never lands here, in either mode: [`Landing::land_checked`]
//! compiles and tests it on the exact commit that lands and moves the release branch to that
//! commit, and a replayed commit that nothing compiled could move it to a release that no daemon
//! can build. Every change the express lane takes goes there too. A change that mixes Ignite's
//! source with files outside the `[express] paths` is refused, by `ignite land` and by
//! `ignite check` ([`refuse_mixed`]), so that every change to Ignite stays an express landing.

use std::path::Path;

use serde_json::json;

use ignite_core::config::LandMode;
use ignite_core::ids::Commit;
use ignite_core::protocol::Class;
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::protocol::hub::{Landed, VerdictKey, verdict_settings};
use ignite_core::protocol::tickets::LandedWork;
use ignite_core::traits::{Error, Push, Rebase};

use super::args::LandArgs;
use super::env::{GateTree, TicketTree};
use super::express::section_in;
use super::lease::HeldLease;
use super::records::{LandingRecord, RecordState};
use super::replies::{failed_gate, stopped};
use super::timeline::Timeline;
use super::{Landing, Rebased, Stop, line, short, stop};
use crate::checks::verbs::{built_lines, gate_lines, render};
use crate::checks::{Excused, GateRun, RunOptions};
use crate::server::{Context, error_reply};
use crate::steps;
use crate::update::scope::{Source, checks_dir, ignite_change};

/// The passes after which a landing by verdict gives up. A pass runs nothing, so it may lose
/// the race for the tip more often than a strict landing before giving up.
const MAX_PASSES: u32 = 20;

/// The ticket's metadata key that a tip round sets when it reverts the ticket's landing.
const REVERTED: &str = "reverted";

/// Where the change stands before the landing touches anything: the tip, the tree's `HEAD`,
/// their merge base and the files the change touches since it.
struct Start {
    tip: Commit,
    head: Commit,
    base: Commit,
    changed: Vec<String>,
}

/// How the gate was settled before the push, for the reply.
enum Judged {
    /// A stored verdict: how long ago it passed, in seconds.
    Reused { age_s: f64 },
    /// The gate ran in the ticket tree; the `verdict` line says whether it was stored.
    Ran { gated: Box<GateRun>, stored: String },
}

/// Whether any of `files` is Ignite's own source (see [`ignite_change`]).
fn touches_ignite(ctx: &Context, files: &[String]) -> bool {
    let checks = checks_dir(&ctx.config);
    files
        .iter()
        .any(|file| ignite_change(Source::ignite(&checks), file))
}

/// The refusal of a change that mixes Ignite's own source with files outside the `[express]
/// paths` of the `ignite.toml` in `tree`, in verdict mode; `None` for any other change, and for
/// every change in strict mode or with no `[express]` section. `changed` reads the change's
/// files, and is called only in verdict mode.
pub(crate) fn refuse_mixed(
    ctx: &Context,
    tree: &Path,
    changed: impl FnOnce() -> ignite_core::traits::Result<Vec<String>>,
) -> Option<Reply> {
    if ctx.config.land.mode != LandMode::Verdict {
        return None;
    }
    let express = section_in(tree, ctx.config.express.as_ref())?;
    let changed = changed().ok()?;
    if !touches_ignite(ctx, &changed) {
        return None;
    }
    let outside: Vec<&String> = changed
        .iter()
        .filter(|path| !express.covers(std::slice::from_ref(*path)))
        .collect();
    if outside.is_empty() {
        return None;
    }
    let mut lines = vec![line(
        "refused",
        "this change mixes Ignite's own source with other files",
    )];
    lines.extend(
        outside
            .iter()
            .take(10)
            .map(|path| line("", (*path).clone())),
    );
    if outside.len() > 10 {
        lines.push(line("", format!("and {} more", outside.len() - 10)));
    }
    lines.push(line(
        "next",
        "land the files under ignite/ and the other files as two tickets",
    ));
    Some(Reply::text(Exit::Usage, render(&lines)))
}

impl Landing {
    /// The whole landing in the configured mode: [`Landing::land_checked`] in strict mode, and
    /// in verdict mode for a change to Ignite's own source or one the express lane takes;
    /// [`Landing::land_by_verdict`] for any other change in verdict mode.
    pub(super) fn land_in_mode(
        &self,
        ctx: &Context,
        req: &Request,
        args: &LandArgs,
    ) -> Result<Reply, Stop> {
        if ctx.config.land.mode == LandMode::Strict {
            return self.land_checked(ctx, req, args);
        }
        let held = self.held_tree(ctx, true)?;
        let io = |e: Error| Box::new(error_reply(&e));
        let tip = ctx.git.tip(&ctx.config.repository.staging).map_err(io)?;
        let branch_head = self.trees.head(&held.path).map_err(io)?;
        let base = ctx
            .git
            .merge_base(&held.path, &branch_head, &tip)
            .map_err(io)?;
        let changed = ctx.git.changed(&base, &branch_head).map_err(io)?;
        if let Some(refused) = refuse_mixed(ctx, &held.path, || Ok(changed.clone())) {
            return Err(Box::new(refused));
        }
        if touches_ignite(ctx, &changed) || self.express_for(ctx, &held.path, &changed).is_some() {
            return self.land_checked(ctx, req, args);
        }
        let start = Start {
            tip,
            head: branch_head,
            base,
            changed,
        };
        self.land_by_verdict(ctx, req, args, &held, start)
    }

    /// The landing by verdict that the module's header describes. `Err` is any reply but a
    /// landing.
    #[allow(clippy::too_many_lines)]
    fn land_by_verdict(
        &self,
        ctx: &Context,
        req: &Request,
        args: &LandArgs,
        held: &TicketTree,
        start: Start,
    ) -> Result<Reply, Stop> {
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
        steps::set_class(Class::Land);
        steps::set_ticket(&ticket, Some(&session));
        let mut timeline = Timeline::start(ctx, self.gate.as_ref());

        // A landing already pushed but not yet told to the hub is never pushed again, and one
        // whose push was cut short landed only if its commit is in the tip's history.
        match self.records.get(&ticket).map_err(io)? {
            Some(record) if record.state == RecordState::Pushed => {
                return Ok(self.finish_parked(ctx, req, held, &record, &start.head));
            }
            Some(mut record) => {
                if self
                    .trees
                    .is_ancestor(&held.path, &record.commit, &start.tip)
                    .map_err(io)?
                {
                    record.state = RecordState::Pushed;
                    self.records.write(&record).map_err(io)?;
                    return Ok(self.finish_parked(ctx, req, held, &record, &start.head));
                }
                self.records.remove(&ticket).map_err(io)?;
            }
            None => {}
        }
        if start.changed.is_empty() {
            return Err(nothing_to_land(&staging));
        }

        let Start {
            tip,
            head,
            base,
            mut changed,
        } = start;
        let mut at = Rebased {
            onto: base,
            head,
            landed_since: 0,
        };
        let formatted = self.format_changes(ctx, held, &mut at, &mut changed)?;
        let reverted = ctx
            .hub
            .ticket(&ticket)
            .is_ok_and(|detail| detail.ticket.metadata.contains_key(REVERTED));
        let committed = ctx.git.committed_at(&held.path, &at.onto).map_err(io)?;
        let age_h = ctx.clock.now().seconds_since(committed) / 3600.0;
        if reverted || age_h > ctx.config.land.max_base_age_hours {
            steps::step(format!("rebasing onto {staging} {}", short(&tip)));
            at = Self::rebase(ctx, held, &tip)?;
            if at.head == at.onto {
                return Err(nothing_to_land(&staging));
            }
        }

        // Step 3: the verdict for the tree as it is now, or the gate run in it. When every
        // failure fails at a base older than the tip as well, the ticket tree is rebased onto
        // the tip for real and the gate runs there once more (see
        // `GateRun::fails_at_older_base`).
        let judged = loop {
            let key = VerdictKey {
                tree: ctx.git.tree_id(&held.path).map_err(io)?,
                base: at.onto.clone(),
                settings: verdict_settings(&ctx.config),
            };
            let found = if reverted {
                None
            } else {
                self.gate.verdict(ctx, &key)
            };
            if let Some(verdict) = found {
                break Judged::Reused {
                    age_s: ctx.clock.now().seconds_since(verdict.passed_at).max(0.0),
                };
            }
            let tree = GateTree {
                tree: held.tree.clone(),
                path: held.path.clone(),
            };
            let options = RunOptions {
                verb: "land".to_owned(),
                class: Class::Land,
                rerun: false,
                ticket: Some(ticket.clone()),
                session: Some(session.clone()),
            };
            steps::step(format!("running the gate in {}", held.tree));
            let pass = super::base::Pass {
                holder: &holder,
                tree: &tree,
                borrowable: false,
                rebased: &at,
                options: &options,
                carried: None,
                earlier: None,
                onto_tip: at.onto == tip,
            };
            let gated = self.gate_pass(ctx, &pass, &mut Excused::new());
            if steps::stop_requested() {
                return Err(stopped());
            }
            if !gated.run.passed() && gated.fails_at_older_base && at.onto != tip {
                steps::step(format!("rebasing onto {staging} {}", short(&tip)));
                at = Self::rebase(ctx, held, &tip)?;
                if at.head == at.onto {
                    return Err(nothing_to_land(&staging));
                }
                continue;
            }
            if !gated.run.passed() {
                let failed = failed_gate(&staging, &at, &gated, &held.path);
                let failed = timeline.onto(ctx.clock.now(), failed, gated.seconds);
                return Err(Box::new(match &formatted {
                    Some(done) => done.onto(failed),
                    None => failed,
                }));
            }
            let stored = match self.gate.remember(ctx, &tree, key, at.head.clone(), &gated) {
                Ok(()) => "stored for this tree".to_owned(),
                Err(error) => format!("not stored: {error}"),
            };
            break Judged::Ran {
                gated: Box::new(gated),
                stored,
            };
        };

        // Step 4: rebase in the object database, take the lease, push; again when the tip moves.
        let mut passes = 0;
        let mut last_cause = String::new();
        loop {
            timeline.next_pass(ctx, &last_cause);
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
            passes += 1;
            let tip = ctx.git.tip(&staging).map_err(io)?;
            steps::step(format!("rebasing onto {staging} {} in git", short(&tip)));
            let (onto, replayed, landed_since) =
                match ctx.git.replay(&held.path, &at.head, &tip).map_err(io)? {
                    Rebase::Clean {
                        onto,
                        head,
                        landed_since,
                    } => (onto, head, landed_since),
                    Rebase::Conflict { files } => return Err(conflict(ctx, &tip, &files)),
                };
            if replayed == onto {
                return Err(nothing_to_land(&staging));
            }
            if steps::stop_requested() {
                return Err(stopped());
            }
            let lease: HeldLease = self.acquire(ctx, &holder, &ticket).map_err(io)?;
            if !lease.still_held() {
                last_cause = format!("the {staging} lease was lost to another landing");
                continue;
            }
            let now = ctx.git.tip(&staging).map_err(io)?;
            if now != tip {
                last_cause = format!("the tip moved from {} to {}", short(&tip), short(&now));
                continue;
            }

            let note = match &args.note {
                Some(note) => note.clone(),
                None => self
                    .trees
                    .subjects(&held.path, &onto, &replayed)
                    .map_err(io)?
                    .join("\n"),
            };
            let work = LandedWork {
                commits: self
                    .trees
                    .commits(&held.path, &onto, &replayed)
                    .map_err(io)?,
                files: ctx.git.changed(&onto, &replayed).map_err(io)?,
                note: note.clone(),
            };
            let mut record = LandingRecord {
                landed: Landed {
                    ticket: ticket.clone(),
                    machine: ctx.machine.clone(),
                    session: session.clone(),
                    work,
                    fresh: args.fresh,
                    pressure: None,
                    defer_successor: false,
                },
                commit: replayed.clone(),
                state: RecordState::Pushing,
                written_at: Some(ctx.clock.now()),
                tree: Some(held.tree.clone()),
            };
            // From here the order is `land_checked`'s: refuse `ignite stop`, write the record,
            // push, and record the push either way before the daemon may restart.
            if !steps::refuse_stop(&format!("its push to {staging} has begun")) {
                return Err(stopped());
            }
            let critical = steps::critical();
            self.records.write(&record).map_err(io)?;
            steps::step(format!("pushing {} to {staging}", short(&replayed)));
            match ctx.git.push(&held.path, &replayed, &staging) {
                Ok(Push::Pushed { .. }) => {
                    record.state = RecordState::Pushed;
                    self.records.write(&record).map_err(io)?;
                    drop(critical);
                }
                Ok(Push::Rejected { reason }) => {
                    steps::allow_stop();
                    last_cause = reason;
                    self.records
                        .remove_if(&ticket, &record.commit)
                        .map_err(io)?;
                    continue;
                }
                Err(error @ Error::Conflict(_)) => {
                    self.records
                        .remove_if(&ticket, &record.commit)
                        .map_err(io)?;
                    return Err(io(error));
                }
                // The push may have reached the remote: the record stays `pushing` for the next
                // landing or the daemon's sender to settle.
                Err(error) => return Err(io(error)),
            }
            drop(lease);
            let settled = self.settle_parked(ctx, req, held, &record, &at.head);
            let mut lines = judged_lines(&judged, &at.onto);
            lines.push(line(
                "rebased",
                format!(
                    "onto {staging} {} without a checkout: {landed_since} commits landed since {}",
                    short(&onto),
                    short(&at.onto)
                ),
            ));
            if let Some(formatted) = &formatted {
                lines.extend(formatted.line());
                lines.extend(formatted.warnings());
            }
            let gate_s = match &judged {
                Judged::Ran { gated, .. } => gated.seconds,
                Judged::Reused { .. } => 0.0,
            };
            lines.extend(timeline.lines(ctx.clock.now(), gate_s));
            let mut reply = Self::reply(ctx, &record, settled, None, Some(&note));
            reply.text = format!("{}{}", render(&lines), reply.text);
            if let Some(object) = reply.json.as_mut().and_then(|j| j.as_object_mut()) {
                object.insert("passes".to_owned(), json!(passes));
                object.insert(
                    "verdict".to_owned(),
                    json!(matches!(judged, Judged::Reused { .. })),
                );
                object.insert("landing".to_owned(), timeline.json(ctx.clock.now()));
            }
            return Ok(reply);
        }
    }
}

impl Landing {
    /// Step 5 for a landing pushed earlier, whose ticket tree is released parked at its own
    /// `HEAD`: the pushed commit was rebased in the object database and never checked out.
    fn finish_parked(
        &self,
        ctx: &Context,
        req: &Request,
        held: &TicketTree,
        record: &LandingRecord,
        park_at: &Commit,
    ) -> Reply {
        let settled = self.settle_parked(ctx, req, held, record, park_at);
        Self::reply(ctx, record, settled, None, None)
    }
}

/// The reply lines that say how the gate was settled: the stored verdict's age, or the gate's
/// own lines and whether its verdict was stored.
fn judged_lines(judged: &Judged, base: &Commit) -> Vec<(String, String)> {
    match judged {
        Judged::Reused { age_s } => vec![line(
            "verdict",
            format!(
                "passed for this tree {:.0} min ago on {}",
                age_s / 60.0,
                short(base)
            ),
        )],
        Judged::Ran { gated, stored } => {
            let mut lines = built_lines(&gated.run);
            lines.extend(gate_lines(gated));
            lines.push(line("verdict", stored.clone()));
            lines
        }
    }
}

#[allow(clippy::unnecessary_box_returns)]
fn nothing_to_land(staging: &str) -> Stop {
    stop(
        Exit::Usage,
        &[
            line(
                "failed",
                format!("nothing to land: {staging} holds every commit"),
            ),
            line("next", "ignite close -m \"<reason>\" closes the ticket"),
        ],
    )
}

/// The reply to a rebase in the object database that conflicts in `files`: exit code 3, and
/// the `next` line of a conflicting rebase of the ticket tree.
#[allow(clippy::unnecessary_box_returns)]
fn conflict(ctx: &Context, tip: &Commit, files: &[String]) -> Stop {
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
            "git rebase {}, resolve them, git rebase --continue, then try again",
            ignite_core::refname::tracking_name(&repository.remote, &repository.staging)
        ),
    ));
    stop(Exit::Conflict, &lines)
}
