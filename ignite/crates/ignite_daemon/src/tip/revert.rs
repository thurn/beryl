//! Reverting the landing that broke the build, instead of reporting a regression (see "A broken
//! build is reverted" in `docs/ignite.md`).
//!
//! A landing no longer type-checks on the exact tip, so two landings that each compile alone can
//! leave a tip that does not compile. Until someone fixes it, `green` cannot move and tip rounds
//! cannot build anything, while every later landing piles onto code nobody can compile. A fix
//! written by an agent takes tens of minutes; removing the landing takes one round.
//!
//! With `[revert] broken_build` set, a failing check of the round's build group (the families
//! declared `builds = true`, see `green`) is reverted when all of these hold, and otherwise
//! reported as a regression exactly as any other failing check is:
//!
//! - the culprit search ended at one landing ([`CulpritSearch::OneCandidate`] or
//!   [`CulpritSearch::Bisected`]);
//! - the failure came from a run on this machine in this round, the tip's own run or the rerun
//!   of a cached failure, not from a result another machine stored;
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
//! tip moved meanwhile, the revert is built again on the new tip, [`MAX_ATTEMPTS`] times at most.
//! The hub is then told with [`Hub::reverted`](ignite_core::traits::Hub::reverted), which
//! reopens the landing's ticket. A report the hub did not take is kept in the verifier's memory
//! and sent again at the start of the next round.

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;

use ignite_core::ids::{Commit, TicketId};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::Reverted;
use ignite_core::tipround::CulpritSearch;
use ignite_core::traits::{Push, RevertOutcome};

use super::{Culprit, TipCheck, TipLanding, TipVerifier, tail};
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

    /// Reverts the landing `culprit` names for `check`, a check that failed at `tip` with
    /// `output`, when the module's conditions hold. Returns true when the check needs no
    /// regression report: its landing was reverted now or earlier in this round.
    pub(super) fn try_revert(
        &mut self,
        ctx: &Context,
        tip: &Commit,
        check: &TipCheck,
        culprit: &Culprit,
        output: Option<&str>,
    ) -> bool {
        if !ctx.config.revert.broken_build || !self.build_families(ctx).contains(&check.family) {
            return false;
        }
        match self.revert(ctx, tip, check, culprit, output) {
            Ok(()) => true,
            Err(why) => {
                eprintln!(
                    "ignite: {} broke the build and was not reverted: {why}",
                    check.check
                );
                false
            }
        }
    }

    /// [`TipVerifier::try_revert`] once the check is known to be a build check, with the
    /// reason when nothing was reverted.
    fn revert(
        &mut self,
        ctx: &Context,
        tip: &Commit,
        check: &TipCheck,
        culprit: &Culprit,
        output: Option<&str>,
    ) -> Result<(), String> {
        let ([landing], CulpritSearch::OneCandidate | CulpritSearch::Bisected { .. }) =
            (culprit.suspects.as_slice(), &culprit.search)
        else {
            return Err("the search did not end at one landing".to_owned());
        };
        if landing.commits.is_empty() {
            return Err("the landing has no commits".to_owned());
        }
        // Another build check that failed with the same landing was reverted with it.
        if self.reverted.contains(&landing.commits) {
            return Ok(());
        }
        if !output.is_some_and(|o| self.own_outputs.contains(o)) {
            return Err("the failure was not run on this machine in this round".to_owned());
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
        let message = message(tip, check, landing, &subjects);
        let revert = self.push_revert(ctx, check, landing, &message)?;
        self.reverted.push(landing.commits.clone());
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
        Ok(())
    }

    /// Builds the revert of `landing` on the `staging` tip, checks that `check` passes there,
    /// and pushes it under the lease. Builds it again when the tip moved, [`MAX_ATTEMPTS`]
    /// times at most. Returns the pushed commit.
    fn push_revert(
        &self,
        ctx: &Context,
        check: &TipCheck,
        landing: &TipLanding,
        message: &str,
    ) -> Result<Commit, String> {
        let staging = &ctx.config.repository.staging;
        let holder = format!("{}:revert", ctx.machine);
        for _ in 0..MAX_ATTEMPTS {
            let base = ctx.git.tip(staging).map_err(|e| e.to_string())?;
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
            let outcome = self
                .work
                .run(ctx, &revert, std::slice::from_ref(&check.check), false)
                .into_iter()
                .find(|r| r.check == check.check)
                .map(|r| r.outcome);
            if !matches!(outcome, Some(Outcome::Pass | Outcome::Flaky)) {
                return Err(format!("the check does not pass at the revert {revert}"));
            }
            let held = self.take_lease(ctx, &holder, landing)?;
            if !held.still_held() {
                continue;
            }
            if ctx.git.tip(staging).map_err(|e| e.to_string())? != base {
                continue;
            }
            match ctx
                .git
                .push(&ctx.repo, &revert, staging)
                .map_err(|e| e.to_string())?
            {
                Push::Pushed { .. } => {
                    eprintln!(
                        "ignite: reverted {} on {staging} with {revert}",
                        landing_name(landing)
                    );
                    return Ok(revert);
                }
                Push::Rejected { .. } => {}
            }
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
        "{subject}\n\nThe build check {} failed at {tip} with {landed}, and passed with \
         them undone. This commit undoes:\n\n{}\n",
        check.check,
        undone.join("\n")
    )
}
