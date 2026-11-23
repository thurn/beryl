//! Automatic tip rounds use the same candidate until it is promoted or a newer landing may
//! fix a blocker. This lets required native checks meet at one commit during active landings.

use ignite_core::ids::Commit;
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
