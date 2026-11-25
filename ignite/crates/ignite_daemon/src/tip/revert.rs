//! Reverting the landing that broke a build or held back master (see "A broken build is
//! reverted" in `docs/ignite.md`).
//!
//! A landing no longer type-checks on the exact tip, so two landings that each compile alone can
//! leave a tip that does not compile. Until someone fixes it, `green` cannot move and tip rounds
//! cannot build anything, while every later landing piles onto code nobody can compile. A fix
//! written by an agent takes tens of minutes; removing the landing takes one round.
//!
//! With `[revert] broken_build` set, a failing check of the round's build group (the families
//! declared `builds = true`, see `green`) can trigger a revert. With `[revert] stall_minutes`
//! set, a required non-build check can also trigger one once the oldest unverified landing
//! has waited that long. The round reports a regression unless all of these hold:
//!
//! - the culprit search ended at one landing ([`CulpritSearch::OneCandidate`] or
//!   [`CulpritSearch::Bisected`]);
//! - the check fails again when this machine runs it at the tip once more, ignoring cached
//!   results, so that a fault of one run never reopens a ticket;
//! - the landing changed none of Ignite's own source. Ignite's releases are judged and moved by
//!   the landing itself, and a revert from a tip round would have to move the release branch on
//!   the judgement of code that may be the broken part;
//! - the landing is not made only of reverts a round made (subjects starting `revert: `, with no
//!   landing recorded at the hub), so a round never reverts a revert;
//! - the round has reverted fewer than [`MAX_REVERTS`] landings;
//! - one commit on the tip undoes all the landing's commits without a conflict, and the failing
//!   check passes at that commit (a cached pass for its inputs counts);
//! - the build checks this machine can run whose inputs the revert changes pass at that commit
//!   too. A failing check that is not a build (a test suite, say) can pass at a revert that does
//!   not compile, when a later landing uses something the reverted landing added. When a build
//!   fails there, or the revert conflicts, the later landings that use what the landing added
//!   are undone in the same commit and the revert is built again (see `dependents`). A revert
//!   that still does not build is not pushed, and the regression is reported, even if it was
//!   reported before, with a note naming the landings involved.
//!
//! The revert commit is pushed to `staging` under the hub's lease, as a landing pushes. When the
//! tip moved meanwhile, the revert is built again on the new tip, [`MAX_ATTEMPTS`] times at most;
//! a new tip that changed what the check reads (a fix, or another machine's revert) is left to
//! the next round, which runs the check there first. Another check blamed on a landing
//! the round reverted already needs only to pass at that revert.
//! The hub is then told with [`Hub::reverted`](ignite_core::traits::Hub::reverted), once for
//! each landing the commit undid, which reopens each landing's ticket. A report the hub did not take is kept in the verifier's memory
//! and sent again at the start of the next round.

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;

use ignite_core::ids::{CheckName, Commit, TicketId};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::Reverted;
use ignite_core::time::Timestamp;
use ignite_core::tipround::CulpritSearch;
use ignite_core::traits::{Push, RevertOutcome};

use super::dependents::Built;
use super::{Culprit, TipCheck, TipLanding, TipVerifier, tail, touches};
use crate::land::lease::{self, HeldLease};
use crate::server::Context;
use crate::update::scope::{Source, checks_dir, ignite_change};

/// The most landings one round reverts. A round that finds more culprits than this is looking
/// at something other than a bad landing, so the rest are reported.
pub const MAX_REVERTS: usize = 3;

/// How many times a revert is built for a tip that keeps moving before the round reports the
/// regression instead.
pub const MAX_ATTEMPTS: u32 = 3;

/// How many times the round waits for a lease another holder has before it reports the
/// regression instead. At up to [`lease::BUSY_WAIT_S`] each, this outlasts the lease's time to
/// live, so a holder that died is waited out.
const LEASE_WAITS: u32 = 30;

/// The longest subject a revert commit has, as the repository's commit check requires.
const SUBJECT_CHARS: usize = 60;

/// How a revert commit's subject starts.
const REVERT_PREFIX: &str = "revert: ";

/// What `blame` does with a failing check after [`TipVerifier::try_revert`].
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Handled {
    /// Its landing was reverted, now or earlier in the round, and the check passes at the
    /// revert. No regression is reported.
    Reverted,
    /// The tip moved with a change to what the check reads. Nothing is reverted. `blame` leaves
    /// the check to the next round, which runs it at the new tip first, or reports it when the
    /// check was left to a later round before.
    NextRound,
    /// The regression is reported, as for any failing check.
    Report,
    /// The check passes at the revert, but the tree there does not build, even with the later
    /// landings that use what the landing added undone too. Nothing is pushed. The regression
    /// is reported with this text, which names the landings, even when the failure was
    /// reported before.
    Unbuildable(String),
}

/// How [`TipVerifier::push_revert`] ended when it did not fail.
enum Pushed {
    /// The revert commit is on `staging`. It undoes these landings, oldest first: the culprit,
    /// then any later landings that had to go with it for the tree to build.
    Done(Commit, Vec<TipLanding>),
    /// The tip moved with a change to what the check reads; the next round runs it there.
    NextRound,
    /// The revert does not build; the text says what was tried (see [`Handled::Unbuildable`]).
    Unbuildable(String),
}

/// How the round waits between tries for a busy lease, in seconds.
pub type Pause = Arc<dyn Fn(f64) + Send + Sync>;

/// The pause of a running daemon: the thread sleeps.
pub(super) fn sleep() -> Pause {
    Arc::new(|seconds| std::thread::sleep(Duration::from_secs_f64(seconds)))
}

impl TipVerifier {
    /// Waits with `pause` instead of sleeping while another holder has the `staging` lease, as
    /// tests do.
    #[must_use]
    pub fn pausing_with(mut self, pause: Pause) -> Self {
        self.pause = pause;
        self
    }

    /// When the oldest landing between `master` and the failed tip has waited long enough
    /// to allow a non-build revert. The last commit of that landing supplies its committer
    /// date, so quiet time before the first unverified landing never counts toward the stall.
    pub(super) fn stall_revert_at(
        &self,
        ctx: &Context,
        master: &Commit,
        tip: &Commit,
    ) -> Option<Timestamp> {
        let minutes = ctx.config.revert.stall_minutes?;
        let delay = i64::try_from(minutes).ok()?.checked_mul(60_000)?;
        let landings = self.work.landings(ctx, master, tip).ok()?;
        let oldest = landings.first()?.last()?;
        let landed_at = ctx.git.committed_at(&ctx.repo, oldest).ok()?;
        Some(Timestamp::from_millis(
            landed_at.millis().checked_add(delay)?,
        ))
    }

    /// Reverts the landing `culprit` names for a failure eligible for automatic revert, when
    /// the module's conditions hold, and says what `blame` does next.
    pub(super) fn try_revert(
        &mut self,
        ctx: &Context,
        tip: &Commit,
        check: &TipCheck,
        culprit: &Culprit,
        output: Option<&str>,
    ) -> Handled {
        match self.revert(ctx, tip, check, culprit, output) {
            Ok(handled) => handled,
            Err(why) => {
                eprintln!(
                    "ignite: could not revert the culprit of {}: {why}",
                    check.check
                );
                Handled::Report
            }
        }
    }

    /// [`TipVerifier::try_revert`] with the reason when nothing was reverted.
    fn revert(
        &mut self,
        ctx: &Context,
        tip: &Commit,
        check: &TipCheck,
        culprit: &Culprit,
        output: Option<&str>,
    ) -> Result<Handled, String> {
        let ([landing], CulpritSearch::OneCandidate | CulpritSearch::Bisected { .. }) =
            (culprit.suspects.as_slice(), &culprit.search)
        else {
            return Err("the search did not end at one landing".to_owned());
        };
        if landing.commits.is_empty() {
            return Err("the landing has no commits".to_owned());
        }
        // A previous check in this round already triggered a revert of this landing. The
        // revert stands for this check too once the check passes there.
        if let Some((_, revert)) = self.reverted.iter().find(|(c, _)| *c == landing.commits) {
            let revert = revert.clone();
            if !self.passes_at(ctx, &revert, check) {
                return Err(format!("the check does not pass at the revert {revert}"));
            }
            for report in self
                .unsent_reverts
                .iter_mut()
                .filter(|r| r.revert == revert)
            {
                if !report.checks.contains(&check.check) {
                    report.checks.push(check.check.clone());
                }
            }
            return Ok(Handled::Reverted);
        }
        let checks = checks_dir(&ctx.config);
        if let Some(path) = landing
            .paths
            .iter()
            .find(|p| ignite_change(Source::ignite(&checks), p))
        {
            return Err(format!("it changed Ignite's own source, {path}"));
        }
        if self.reverted.len() >= MAX_REVERTS {
            return Err(format!(
                "this round already reverted {MAX_REVERTS} landings"
            ));
        }
        if landing.ticket.is_none()
            && subjects(ctx, landing)?
                .iter()
                .all(|s| s.starts_with(REVERT_PREFIX))
        {
            return Err("it is a revert that a round made".to_owned());
        }
        // One failed run may be a fault of that run, and a revert reopens a ticket, so the
        // check runs at the tip once more, ignoring cached results.
        if self.outcome_at(ctx, tip, check, true) != Some(Outcome::Fail) {
            return Err(format!("the check did not fail again at {tip}"));
        }
        let (revert, undone) = match self.push_revert(ctx, tip, check, landing)? {
            Pushed::Done(revert, undone) => (revert, undone),
            Pushed::NextRound => return Ok(Handled::NextRound),
            Pushed::Unbuildable(note) => return Ok(Handled::Unbuildable(note)),
        };
        let machine = if self.host.is_empty() {
            ctx.machine.to_string()
        } else {
            self.host.clone()
        };
        let output = output
            .map(|o| tail(o, &self.own_outputs))
            .unwrap_or_default();
        for undone in &undone {
            self.reverted.push((undone.commits.clone(), revert.clone()));
            // A later landing was undone only because it uses what the culprit added, so its
            // ticket says so before the check's output.
            let output = if undone.commits == landing.commits {
                output.clone()
            } else {
                format!(
                    "This landing was undone together with {}, which {} blamed: it uses what \
                     that landing added, so the tree did not build with only that landing \
                     undone.\n\n{output}",
                    landing_name(landing),
                    check.check
                )
            };
            self.unsent_reverts.push(Reverted {
                ticket: undone.ticket.clone(),
                landed: undone.commits.clone(),
                revert: revert.clone(),
                checks: vec![check.check.clone()],
                output,
                search: culprit.search.clone(),
                machine: machine.clone(),
            });
        }
        self.send_reverts(ctx);
        self.save(ctx);
        crate::steps::allow_stop();
        Ok(Handled::Reverted)
    }

    /// Whether `check` passes at the proposed revert. A cached pass counts.
    fn passes_at(&self, ctx: &Context, at: &Commit, check: &TipCheck) -> bool {
        matches!(
            self.outcome_at(ctx, at, check, false),
            Some(Outcome::Pass | Outcome::Flaky)
        )
    }

    fn outcome_at(
        &self,
        ctx: &Context,
        at: &Commit,
        check: &TipCheck,
        rerun: bool,
    ) -> Option<Outcome> {
        self.work
            .run(ctx, at, std::slice::from_ref(&check.check), rerun, false)
            .into_iter()
            .find(|r| r.check == check.check)
            .map(|r| r.outcome)
    }

    /// Builds the revert of `landing` on the `staging` tip, checks that `check` passes there and
    /// that the tree there builds, and pushes it under the lease. Builds it again when the tip
    /// moved, [`MAX_ATTEMPTS`] times at most.
    ///
    /// When the revert conflicts or does not build, the later landings that use what `landing`
    /// added are undone in the same commit, once (see `dependents`). A revert that still does
    /// not build is not pushed ([`Pushed::Unbuildable`]). The round gives up with
    /// [`Pushed::NextRound`] when the tip moved past `tip` with a change to the check's inputs:
    /// a fix, or another machine's revert, may have landed, and the next round runs the check
    /// there first, as `blame` leaves such a check.
    #[allow(clippy::too_many_lines)]
    fn push_revert(
        &self,
        ctx: &Context,
        tip: &Commit,
        check: &TipCheck,
        landing: &TipLanding,
    ) -> Result<Pushed, String> {
        let staging = &ctx.config.repository.staging;
        let holder = format!("{}:revert", ctx.machine);
        let mut undone = vec![landing.clone()];
        let mut widened = false;
        let mut attempts = 0;
        while attempts < MAX_ATTEMPTS {
            attempts += 1;
            let base = ctx.git.tip(staging).map_err(|e| e.to_string())?;
            if base != *tip {
                let changed = ctx.git.changed(tip, &base).map_err(|e| e.to_string())?;
                if touches(&changed, &check.inputs) {
                    eprintln!(
                        "ignite: {staging} moved to {base}, which changes what {} reads; the \
                         next round runs it there",
                        check.check
                    );
                    return Ok(Pushed::NextRound);
                }
            }
            let commits: Vec<Commit> = undone.iter().flat_map(|l| l.commits.clone()).collect();
            let message = message(ctx, tip, check, &undone)?;
            let revert = match ctx
                .git
                .revert(&ctx.repo, &base, &commits, &message)
                .map_err(|e| e.to_string())?
            {
                RevertOutcome::Clean { commit } => commit,
                RevertOutcome::Conflict { files } => {
                    if !widened {
                        widened = true;
                        if let Some(more) = self.widen(ctx, landing, &base, &mut undone)? {
                            eprintln!(
                                "ignite: undoing {} conflicts; undoing {more} with it",
                                landing_name(landing)
                            );
                            attempts -= 1;
                            continue;
                        }
                    }
                    return Err(format!(
                        "undoing it on {base} conflicts in {}",
                        files.join(", ")
                    ));
                }
            };
            if !self.passes_at(ctx, &revert, check) {
                return Err(format!("the check does not pass at the revert {revert}"));
            }
            let paths: Vec<String> = undone.iter().flat_map(|l| l.paths.clone()).collect();
            match self.builds_at(ctx, &revert, &paths, &check.check) {
                Built::Yes => {}
                Built::Unknown(names) => {
                    return Err(format!(
                        "{} gave no verdict at the revert {revert}",
                        join(&names)
                    ));
                }
                Built::Fails(names) => {
                    if !widened {
                        widened = true;
                        if let Some(more) = self.widen(ctx, landing, &base, &mut undone)? {
                            eprintln!(
                                "ignite: {} fails at the revert {revert}; undoing {more} too",
                                join(&names)
                            );
                            attempts -= 1;
                            continue;
                        }
                    }
                    return Ok(Pushed::Unbuildable(unbuildable(
                        check, landing, &undone, &names, &revert,
                    )));
                }
            }
            let held = self.take_lease(ctx, &holder, landing)?;
            if !held.still_held() {
                continue;
            }
            if ctx.git.tip(staging).map_err(|e| e.to_string())? != base {
                continue;
            }
            if !check.is_build() {
                let master = ctx
                    .git
                    .tip(&ctx.config.repository.master)
                    .map_err(|e| e.to_string())?;
                if self.master_at_start.as_ref() != Some(&master) {
                    return Err(format!("master advanced to {master} during the round"));
                }
            }
            // Keep the round alive until a pushed revert has been reported or saved for
            // retry, so the landing's ticket can be reopened.
            if !crate::steps::refuse_stop(&format!("its revert push to {staging} has begun")) {
                return Ok(Pushed::NextRound);
            }
            let pushed = ctx.git.push(&ctx.repo, &revert, staging);
            if matches!(pushed, Ok(Push::Pushed { .. })) {
                let names: Vec<String> = undone.iter().map(landing_name).collect();
                eprintln!(
                    "ignite: reverted {} on {staging} with {revert}",
                    names.join(", ")
                );
                return Ok(Pushed::Done(revert, undone));
            }
            crate::steps::allow_stop();
            pushed.map_err(|e| e.to_string())?;
        }
        Err(format!(
            "the tip moved each of {MAX_ATTEMPTS} times before the revert was pushed"
        ))
    }

    /// Adds to `undone` the later landings up to `base` that depend on `landing`, and names
    /// them; `None` when there are none. A landing that changed Ignite's own source or is a
    /// revert a round made is never undone, so finding one ends the attempt.
    fn widen(
        &self,
        ctx: &Context,
        landing: &TipLanding,
        base: &Commit,
        undone: &mut Vec<TipLanding>,
    ) -> Result<Option<String>, String> {
        let more = self.dependents(ctx, landing, base)?;
        if more.is_empty() {
            return Ok(None);
        }
        let checks = checks_dir(&ctx.config);
        for other in &more {
            if let Some(path) = other
                .paths
                .iter()
                .find(|p| ignite_change(Source::ignite(&checks), p))
            {
                return Err(format!(
                    "the later landing {} that uses what it added changed Ignite's own source, \
                     {path}",
                    landing_name(other)
                ));
            }
            if other.ticket.is_none()
                && subjects(ctx, other)?
                    .iter()
                    .all(|s| s.starts_with(REVERT_PREFIX))
            {
                return Err(format!(
                    "the later landing {} that uses what it added is a revert that a round made",
                    landing_name(other)
                ));
            }
        }
        let names: Vec<String> = more.iter().map(landing_name).collect();
        undone.extend(more);
        Ok(Some(names.join(", ")))
    }

    /// The `staging` lease, waiting while another holder has it, [`LEASE_WAITS`] times at most.
    fn take_lease(
        &self,
        ctx: &Context,
        holder: &str,
        landing: &TipLanding,
    ) -> Result<HeldLease, String> {
        let payload = json!({
            "revert": landing.ticket,
            "machine": ctx.machine,
        });
        for _ in 0..LEASE_WAITS {
            match lease::try_acquire(ctx, holder, payload.clone()).map_err(|e| e.to_string())? {
                Ok(held) => return Ok(held),
                Err(busy) => (self.pause)(lease::busy_wait(ctx, &busy)),
            }
        }
        Err(format!(
            "the {} lease stayed with another holder",
            ctx.config.repository.staging
        ))
    }

    /// Tells the hub of each revert it has not taken yet. One it refuses stays for the next
    /// round; the revert itself is on `staging` already, so only the ticket waits.
    pub(super) fn send_reverts(&mut self, ctx: &Context) {
        let mut unsent = Vec::new();
        for report in std::mem::take(&mut self.unsent_reverts) {
            match ctx.hub.reverted(&report) {
                Ok(filed) => eprintln!(
                    "ignite: the hub filed the revert {} as {}",
                    report.revert, filed.ticket
                ),
                Err(error) => {
                    eprintln!(
                        "ignite: the hub was not told of the revert {}: {error}",
                        report.revert
                    );
                    unsent.push(report);
                }
            }
        }
        self.unsent_reverts = unsent;
    }
}

/// The landing's ticket, or its first commit when no landing recorded it.
fn landing_name(landing: &TipLanding) -> String {
    landing.ticket.as_ref().map_or_else(
        || {
            landing
                .commits
                .first()
                .map(ToString::to_string)
                .unwrap_or_default()
        },
        TicketId::to_string,
    )
}

/// The subject of each of `landing`'s commits.
fn subjects(ctx: &Context, landing: &TipLanding) -> Result<Vec<String>, String> {
    landing
        .commits
        .iter()
        .map(|c| ctx.git.subject(&ctx.repo, c))
        .collect::<Result<Vec<String>, _>>()
        .map_err(|error| {
            format!(
                "the subjects of {} could not be read: {error}",
                landing_name(landing)
            )
        })
}

/// Each of `landing`'s commits with its subject, one per line.
fn commit_lines(ctx: &Context, landing: &TipLanding) -> Result<Vec<String>, String> {
    Ok(landing
        .commits
        .iter()
        .zip(subjects(ctx, landing)?)
        .map(|(commit, line)| format!("{commit} {line}"))
        .collect())
}

/// The revert commit's message: `revert: <the culprit's first subject>`, cut to
/// [`SUBJECT_CHARS`], then a body naming the check and the commits it undoes. `undone` starts
/// with the culprit; any landings after it were undone because they use what it added.
fn message(
    ctx: &Context,
    tip: &Commit,
    check: &TipCheck,
    undone: &[TipLanding],
) -> Result<String, String> {
    let Some((culprit, later)) = undone.split_first() else {
        return Err("there is nothing to undo".to_owned());
    };
    let culprit_lines = commit_lines(ctx, culprit)?;
    let first = culprit_lines
        .first()
        .and_then(|line| line.split_once(' '))
        .map_or("", |(_, subject)| subject);
    let subject: String = format!("{REVERT_PREFIX}{first}")
        .chars()
        .take(SUBJECT_CHARS)
        .collect();
    let landed = match &culprit.ticket {
        Some(ticket) => format!("the landing of {ticket}"),
        None => "commits no landing recorded".to_owned(),
    };
    let mut text = format!(
        "{subject}\n\nThe check {} failed at {tip} with {landed}, and passed with \
         them undone. This commit undoes:\n\n{}\n",
        check.check,
        culprit_lines.join("\n")
    );
    if !later.is_empty() {
        let mut lines = Vec::new();
        for landing in later {
            lines.extend(commit_lines(ctx, landing)?);
        }
        text.push_str(
            "\nThe tree did not build with only those undone, so it also undoes these later \
             commits, which use what they added:\n\n",
        );
        text.push_str(&lines.join("\n"));
        text.push('\n');
    }
    Ok(text)
}

/// The report's text for a revert that does not build: what was undone, and which build
/// checks failed at the revert.
fn unbuildable(
    check: &TipCheck,
    landing: &TipLanding,
    undone: &[TipLanding],
    fails: &[CheckName],
    revert: &Commit,
) -> String {
    let commits =
        |l: &TipLanding| -> Vec<String> { l.commits.iter().map(ToString::to_string).collect() };
    let mut text = format!(
        "Ignite did not revert {} ({}) automatically. The check {} passes with it undone, but \
         {} then fails at the revert {revert}.",
        landing_name(landing),
        commits(landing).join(", "),
        check.check,
        join(fails)
    );
    match undone.get(1..) {
        Some(later) if !later.is_empty() => {
            let names: Vec<String> = later
                .iter()
                .map(|l| format!("{} ({})", landing_name(l), commits(l).join(", ")))
                .collect();
            text.push_str(
                " That revert also undid these later landings, which use what it added: ",
            );
            text.push_str(&names.join("; "));
            text.push('.');
        }
        _ => {
            text.push_str(
                " No later landing uses a word it introduced or removes a line it added.",
            );
        }
    }
    text
}

/// Check names joined with commas.
fn join(names: &[CheckName]) -> String {
    names
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}
