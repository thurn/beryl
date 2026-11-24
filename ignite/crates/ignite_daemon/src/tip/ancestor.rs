//! Advances master past fully cached landings while the tip's failure is still reported.

use std::collections::HashSet;

use ignite_core::ids::{CheckName, Commit};
use ignite_core::protocol::Outcome;
use ignite_core::traits::Result;

use super::coverage::{Deferrals, Required, Scope, budget_after};
use super::round::require_running;
use super::{TipLanding, TipOutcome, TipVerifier, covered};
use crate::server::Context;

/// Rekeying the kept listings still costs a checkout and reads their inputs.
const LOOKBACK_LANDINGS: usize = 8;

impl TipVerifier {
    pub(super) fn promote_ancestor(
        &mut self,
        ctx: &Context,
        last: &Commit,
        tip: &Commit,
        culprits: &HashSet<Commit>,
        regressions: &HashSet<CheckName>,
    ) -> Result<Option<Commit>> {
        if last == tip || !ctx.git.is_ancestor(last, tip)? {
            return Ok(None);
        }
        let landings = self.work.landings(ctx, last, tip)?;
        // Search landing boundaries before the first culprit. With no culprit, stop at the
        // landing containing the failed tip. Do not rely on a cutoff outside this range.
        let Some(before) = landings
            .iter()
            .position(|landing| {
                landing
                    .commits
                    .iter()
                    .any(|c| c == tip || culprits.contains(c))
            })
            .filter(|before| *before > 0)
        else {
            return Ok(None);
        };
        if landings.last().and_then(TipLanding::last) != Some(tip)
            || !culprits.iter().all(|culprit| {
                landings
                    .iter()
                    .any(|landing| landing.commits.contains(culprit))
            })
        {
            return Ok(None);
        }
        let coverage = ctx.hub.tip_coverage()?;
        for candidate in landings[..before]
            .iter()
            .rev()
            .filter_map(TipLanding::last)
            .take(LOOKBACK_LANDINGS)
        {
            require_running()?;
            let scope = Scope::new(ctx, self, last, candidate, &coverage)?;
            let Some(deferred) = self.cached_deferrals(ctx, candidate, &scope, regressions)? else {
                continue;
            };
            require_running()?;
            self.require_machine_on()?;
            // Another round may have reached this landing while we read its results. The
            // push itself still uses Git's fast-forward check if master moves again.
            let master = ctx.git.tip(&ctx.config.repository.master)?;
            if master == *candidate || !ctx.git.is_ancestor(&master, candidate)? {
                return Ok(None);
            }
            let staging = ctx.git.tip(&ctx.config.repository.staging)?;
            if !ctx.git.is_ancestor(candidate, &staging)? {
                continue;
            }
            scope.record_deferrals(ctx, &master, candidate, &deferred)?;
            // A pass before the failure is not proof that a reported regression was fixed.
            if self.move_master(ctx, candidate, Vec::new(), Some(tip))? == TipOutcome::Verified {
                eprintln!("ignite: tip {tip} failed; master advanced to {candidate}");
                return Ok(Some(candidate.clone()));
            }
            return Ok(None);
        }
        Ok(None)
    }

    fn cached_deferrals(
        &self,
        ctx: &Context,
        candidate: &Commit,
        scope: &Scope<'_>,
        regressions: &HashSet<CheckName>,
    ) -> Result<Option<Deferrals>> {
        let set = match self.work.cached_checks(ctx, candidate) {
            Ok(Some(set)) => set,
            Ok(None) => return Ok(None),
            Err(error) => {
                eprintln!("ignite: tip {candidate}: could not read kept listings: {error}");
                return Ok(None);
            }
        };
        let listing = set.listing;
        let Required {
            checks,
            cached,
            unlisted,
            mut deferred,
        } = scope.required(
            ctx,
            covered::drop_covered(
                listing.checks,
                &set.covered_by,
                regressions,
                Some(&listing.defer_platforms),
            ),
            listing.unlisted,
            &listing.defer_platforms,
        )?;
        if !unlisted.is_empty() {
            return Ok(None);
        }
        let pending: Vec<_> = checks
            .into_iter()
            .zip(cached)
            .filter(|(check, hit)| {
                !check.reuse
                    || !hit
                        .as_ref()
                        .is_some_and(|hit| matches!(hit.outcome, Outcome::Pass | Outcome::Flaky))
            })
            .collect();
        if pending.iter().any(|(check, hit)| {
            !check.reuse
                || check.is_build()
                || regressions.contains(&check.check)
                || check.key.platform.is_none()
                || hit.as_ref().is_some_and(|hit| hit.outcome == Outcome::Fail)
        }) {
            return Ok(None);
        }
        let after = budget_after(ctx, candidate, pending.iter().map(|(check, _)| check))?;
        for (check, _) in pending {
            let Some(os) = check.key.platform.filter(|_| after.contains(&check.key)) else {
                return Ok(None);
            };
            deferred.entry(os).or_default().push(check.check);
        }
        Ok(Some(deferred))
    }
}
