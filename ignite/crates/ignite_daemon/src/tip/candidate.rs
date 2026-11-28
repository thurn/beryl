//! Automatic tip rounds use the same candidate until it is promoted or a newer landing may
//! fix a blocker. This lets required native checks meet at one commit during active landings.

use std::sync::{Mutex, PoisonError};

use ignite_core::ids::Commit;
use ignite_core::time::Timestamp;
use ignite_core::tipround::{TipCandidate, TipCandidateUpdate};
use ignite_core::traits::Result;

use super::TipVerifier;
use crate::server::Context;

impl TipVerifier {
    /// Selects the round's target before starting work. The hub compares against the
    /// selection we read so simultaneous starts agree even when their staging fetches differ.
    pub(super) fn next_target(&self, ctx: &Context) -> Result<Option<Commit>> {
        if self.machine_is_off() {
            return Ok(None);
        }
        let repository = &ctx.config.repository;
        loop {
            let candidate = ctx.hub.tip_candidate()?;
            let staging = ctx.git.tip(&repository.staging)?;
            if let Some(candidate) = &candidate {
                if candidate.commit == staging {
                    return Ok(self.due(ctx, &staging).then_some(staging));
                }
                if !candidate.retry_on_new_tip
                    && ctx.git.is_ancestor(&candidate.commit, &staging)?
                {
                    let master = ctx.git.tip(&repository.master)?;
                    if master != candidate.commit
                        && ctx.git.is_ancestor(&master, &candidate.commit)?
                    {
                        let tip = candidate.commit.clone();
                        return Ok(self.due(ctx, &tip).then_some(tip));
                    }
                }
            }
            if !self.due(ctx, &staging) {
                return Ok(None);
            }
            let update = TipCandidateUpdate {
                expected: candidate,
                replacement: TipCandidate {
                    commit: staging.clone(),
                    retry_on_new_tip: false,
                },
            };
            if ctx.hub.update_tip_candidate(&update)? {
                return Ok(Some(staging));
            }
        }
    }
}

/// A different commit selected by the hub ends the round. A retry flag, absent selection,
/// or failed read leaves the round free to continue.
pub(super) fn replacement(ctx: &Context, tip: &Commit) -> Option<Commit> {
    let candidate = ctx.hub.tip_candidate().ok()??;
    (candidate.commit != *tip).then_some(candidate.commit)
}

/// How long a round running its own checks before `master` moves may continue after the hub
/// selects a different candidate. Long enough for fresh rounds to finish short checks and
/// report failures during rapid candidate churn, and short enough that a slow round on a
/// dead or reverted commit does not starve the newer candidate or hit `TIP_STALLS_AFTER`.
pub(crate) const CHECKING_GRACE_S: f64 = 900.0;

/// Whether `commit` is a revert commit (from an automatic round revert or a manual git revert).
fn is_revert(ctx: &Context, commit: &Commit) -> bool {
    ctx.git.subject(&ctx.repo, commit).is_ok_and(|subject| {
        subject.starts_with(super::revert::REVERT_PREFIX) || subject.starts_with("Revert ")
    })
}

/// Whether a checking round whose target has been replaced by `by` should stop: either `by` is
/// a revert commit, or `tip` has been replaced for at least [`CHECKING_GRACE_S`]. Records the
/// first time a replacement was seen in `since`.
pub(super) fn checking_superseded(
    ctx: &Context,
    by: &Commit,
    since: &mut Option<Timestamp>,
) -> bool {
    if is_revert(ctx, by) {
        return true;
    }
    let now = ctx.clock.now();
    let first = *since.get_or_insert(now);
    now.seconds_since(first) >= CHECKING_GRACE_S
}

/// The newer candidate that ends a round's own checks before they finish: once `master` has
/// reached `tip`, once a revert commit has replaced `tip`, or once `tip` has been replaced by
/// a newer candidate for at least [`CHECKING_GRACE_S`].
///
/// A fresh replacement alone does not end them immediately. The hub replaces a candidate as
/// soon as any machine reports a required failure at it, so that a newer landing can bring the
/// fix. On 2026-11-25 a Windows-only lint failed at every tip from 14:21 to 20:50, and every
/// landing in that time replaced the candidate within minutes and ended the other machines'
/// rounds before they ran their checks: 84 rounds between 15:40 and 21:50 ran about 5,000
/// checks between them, against some 24,000 in one full round. Breakages that landed meanwhile
/// went unseen and were found one at a time after the lint stopped failing, and the last of
/// them was reverted at 22:14, 1 h 40 min after it landed. Giving a checking round a bounded
/// grace window lets fresh rounds finish short checks and report failures at `tip`, while
/// preventing a slow round on a dead or reverted commit from running for hours and starving
/// the newer candidate. A round with a failure of its own stops starting other families, finishes
/// the family that failed, and goes on to its verdict and culprit search (see `families`).
pub(super) fn ends_checks(
    ctx: &Context,
    tip: &Commit,
    superseded_since: &Mutex<Option<Timestamp>>,
) -> Option<Commit> {
    let mut since = superseded_since
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let Some(by) = replacement(ctx, tip) else {
        *since = None;
        return None;
    };
    if super::round::master_reached(ctx, tip).is_some() || checking_superseded(ctx, &by, &mut since)
    {
        return Some(by);
    }
    None
}

/// Lets a later staging tip try all required checks when this tip has a failed or blocked
/// check. If another round has already selected a different tip, leave that selection alone.
pub(super) fn allow_newer(ctx: &Context, tip: &Commit) -> Result<()> {
    ctx.hub.update_tip_candidate(&TipCandidateUpdate {
        expected: Some(TipCandidate {
            commit: tip.clone(),
            retry_on_new_tip: false,
        }),
        replacement: TipCandidate {
            commit: tip.clone(),
            retry_on_new_tip: true,
        },
    })?;
    Ok(())
}

/// Moves the shared candidate forward to `target` when it is absent or a strict ancestor of
/// `target`, so rounds do not spend time on commits before a revert or proven fix. Leaves a
/// candidate that is already at `target` (preserving its `retry_on_new_tip` flag) or not an
/// ancestor of `target` alone.
pub(super) fn advance_to(ctx: &Context, target: &Commit) -> Result<()> {
    loop {
        let current = ctx.hub.tip_candidate()?;
        if let Some(candidate) = &current
            && (candidate.commit == *target || !ctx.git.is_ancestor(&candidate.commit, target)?)
        {
            return Ok(());
        }
        let update = TipCandidateUpdate {
            expected: current,
            replacement: TipCandidate {
                commit: target.clone(),
                retry_on_new_tip: false,
            },
        };
        if ctx.hub.update_tip_candidate(&update)? {
            return Ok(());
        }
    }
}

/// Like [`advance_to`], logging any error instead of failing the caller's revert or blame step.
pub(super) fn advance_to_best_effort(ctx: &Context, target: &Commit) {
    if let Err(error) = advance_to(ctx, target) {
        eprintln!("ignite: could not advance tip candidate to {target}: {error}");
    }
}
