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
//!   check passes at that commit (a cached pass for its inputs counts).
//!
//! The revert commit is pushed to `staging` under the hub's lease, as a landing pushes. When the
//! tip moved meanwhile, the revert is built again on the new tip, [`MAX_ATTEMPTS`] times at most;
//! a new tip that changed what the check reads (a fix, or another machine's revert) is left to
//! the next round, which runs the check there first. Another check blamed on a landing
//! the round reverted already needs only to pass at that revert.
//! The hub is then told with [`Hub::reverted`](ignite_core::traits::Hub::reverted), which
//! reopens the landing's ticket. A report the hub did not take is kept in the verifier's memory
//! and sent again at the start of the next round.

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;

use ignite_core::ids::{Commit, TicketId};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::Reverted;
use ignite_core::time::Timestamp;
use ignite_core::tipround::CulpritSearch;
use ignite_core::traits::{Push, RevertOutcome};

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
            if let Some(report) = self.unsent_reverts.iter_mut().find(|r| r.revert == revert) {
                report.checks.push(check.check.clone());
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
        let subjects = landing
            .commits
            .iter()
            .map(|c| ctx.git.subject(&ctx.repo, c))
            .collect::<Result<Vec<String>, _>>()
            .map_err(|error| format!("its subjects could not be read: {error}"))?;
        if landing.ticket.is_none() && subjects.iter().all(|s| s.starts_with(REVERT_PREFIX)) {
            return Err("it is a revert that a round made".to_owned());
        }
        // One failed run may be a fault of that run, and a revert reopens a ticket, so the
        // check runs at the tip once more, ignoring cached results.
        if self.outcome_at(ctx, tip, check, true) != Some(Outcome::Fail) {
            return Err(format!("the check did not fail again at {tip}"));
        }
        let message = message(tip, check, landing, &subjects);
        let Some(revert) = self.push_revert(ctx, tip, check, landing, &message)? else {
            return Ok(Handled::NextRound);
        };
        self.reverted
            .push((landing.commits.clone(), revert.clone()));
        let machine = if self.host.is_empty() {
            ctx.machine.to_string()
        } else {
            self.host.clone()
        };
        let report = Reverted {
            ticket: landing.ticket.clone(),
            landed: landing.commits.clone(),
            revert,
            checks: vec![check.check.clone()],
            output: output
                .map(|o| tail(o, &self.own_outputs))
                .unwrap_or_default(),
            search: culprit.search.clone(),
            machine,
        };
        self.unsent_reverts.push(report);
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
            .run(ctx, at, std::slice::from_ref(&check.check), rerun)
            .into_iter()
            .find(|r| r.check == check.check)
            .map(|r| r.outcome)
    }

    /// Builds the revert of `landing` on the `staging` tip, checks that `check` passes there,
    /// and pushes it under the lease. Builds it again when the tip moved, [`MAX_ATTEMPTS`]
    /// times at most. Returns the pushed commit, or `None` when the tip moved past `tip` with a
    /// change to the check's inputs: a fix, or another machine's revert, may have landed, and
    /// the next round runs the check there first, as `blame` leaves such a check.
    fn push_revert(
        &self,
        ctx: &Context,
        tip: &Commit,
        check: &TipCheck,
        landing: &TipLanding,
        message: &str,
    ) -> Result<Option<Commit>, String> {
        let staging = &ctx.config.repository.staging;
        let holder = format!("{}:revert", ctx.machine);
        for _ in 0..MAX_ATTEMPTS {
            let base = ctx.git.tip(staging).map_err(|e| e.to_string())?;
            if base != *tip {
                let changed = ctx.git.changed(tip, &base).map_err(|e| e.to_string())?;
                if touches(&changed, &check.inputs) {
                    eprintln!(
                        "ignite: {staging} moved to {base}, which changes what {} reads; the \
                         next round runs it there",
                        check.check
                    );
                    return Ok(None);
                }
            }
            let revert = match ctx
                .git
                .revert(&ctx.repo, &base, &landing.commits, message)
                .map_err(|e| e.to_string())?
            {
                RevertOutcome::Clean { commit } => commit,
                RevertOutcome::Conflict { files } => {
                    return Err(format!(
                        "undoing it on {base} conflicts in {}",
                        files.join(", ")
                    ));
                }
            };
            if !self.passes_at(ctx, &revert, check) {
                return Err(format!("the check does not pass at the revert {revert}"));
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
                return Ok(None);
            }
            let pushed = ctx.git.push(&ctx.repo, &revert, staging);
            if matches!(pushed, Ok(Push::Pushed { .. })) {
                eprintln!(
                    "ignite: reverted {} on {staging} with {revert}",
                    landing_name(landing)
                );
                return Ok(Some(revert));
            }
            crate::steps::allow_stop();
            pushed.map_err(|e| e.to_string())?;
        }
        Err(format!(
            "the tip moved each of {MAX_ATTEMPTS} times before the revert was pushed"
        ))
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

/// The revert commit's message: `revert: <the landing's first subject>`, cut to
/// [`SUBJECT_CHARS`], then a body naming the check and the commits it undoes.
fn message(tip: &Commit, check: &TipCheck, landing: &TipLanding, subjects: &[String]) -> String {
    let first = subjects.first().map_or("", String::as_str);
    let subject: String = format!("{REVERT_PREFIX}{first}")
        .chars()
        .take(SUBJECT_CHARS)
        .collect();
    let landed = match &landing.ticket {
        Some(ticket) => format!("the landing of {ticket}"),
        None => "commits no landing recorded".to_owned(),
    };
    let undone: Vec<String> = landing
        .commits
        .iter()
        .zip(subjects)
        .map(|(commit, line)| format!("{commit} {line}"))
        .collect();
    format!(
        "{subject}\n\nThe check {} failed at {tip} with {landed}, and passed with \
         them undone. This commit undoes:\n\n{}\n",
        check.check,
        undone.join("\n")
    )
}
