//! Native checks deferred until after promotion, and the coverage each platform still owes.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use ignite_core::config::Os;
use ignite_core::ids::{CheckName, Commit};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::{CachedResult, ResultKey};
use ignite_core::tipround::{
    DeferredPlatform, PlatformVerification, TipBudgetPlan, TipCoverage, TipCoverageUpdate,
};
use ignite_core::traits::Result;

use super::{TipCheck, TipRun, TipVerifier, Unlisted};
use crate::server::Context;

pub(super) type Deferrals = BTreeMap<Os, Vec<CheckName>>;

pub(super) struct Required {
    pub checks: Vec<TipCheck>,
    pub cached: Vec<Option<CachedResult>>,
    pub unlisted: Vec<Unlisted>,
    pub deferred: Deferrals,
}

pub(super) struct Scope<'a> {
    coverage: &'a TipCoverage,
    fleet_deferred: BTreeSet<Os>,
    pub local: Option<Os>,
    pub catching_up: Option<(Os, &'a DeferredPlatform)>,
    only: Option<Os>,
}

impl<'a> Scope<'a> {
    pub fn new(
        ctx: &Context,
        verifier: &TipVerifier,
        last: &Commit,
        tip: &Commit,
        coverage: &'a TipCoverage,
    ) -> Result<Self> {
        let fleet_deferred = verifier
            .fleet_notice()
            .map_or_else(BTreeSet::new, |notice| {
                let mut deferred = notice.off_platforms;
                deferred.extend(notice.checks_only_platforms);
                deferred
            });
        let local = verifier.local_platform(ctx);
        let mut catching_up = None;
        if let Some(os) = local
            && let Some(verified) = coverage.platforms.get(&os)
            && let Some(gap) = &verified.deferred
            && ctx.git.is_ancestor(&verified.through, tip)?
        {
            catching_up = Some((os, gap));
        }
        // When master is already at this tip, only the returning platform still needs a round.
        let only = catching_up.filter(|_| last == tip).map(|(os, _)| os);
        Ok(Self {
            coverage,
            fleet_deferred,
            local,
            catching_up,
            only,
        })
    }

    pub fn catch_up_only(&self) -> bool {
        self.only.is_some()
    }

    pub fn required(
        &self,
        ctx: &Context,
        checks: Vec<TipCheck>,
        unlisted: Vec<Unlisted>,
        defer_platforms: &BTreeMap<String, Vec<Os>>,
    ) -> Result<Required> {
        let (checks, unlisted) = self.select(checks, unlisted);
        let keys: Vec<ResultKey> = checks.iter().map(|check| check.key.clone()).collect();
        let cached = if keys.is_empty() {
            Vec::new()
        } else {
            ctx.hub.lookup(&keys)?
        };
        Ok(self.defer(checks, cached, unlisted, defer_platforms))
    }

    fn select(
        &self,
        mut checks: Vec<TipCheck>,
        mut unlisted: Vec<Unlisted>,
    ) -> (Vec<TipCheck>, Vec<Unlisted>) {
        if let Some(os) = self.only {
            checks.retain(|check| check.key.platform == Some(os));
            unlisted.retain_mut(|missing| {
                missing.platforms.retain(|platform| *platform == os);
                !missing.platforms.is_empty()
            });
        }
        (checks, unlisted)
    }

    pub fn last_good_for(&self, check: &TipCheck) -> Option<&Commit> {
        self.catching_up
            .filter(|(os, _)| check.key.platform == Some(*os))
            .map(|(_, gap)| &gap.last_good)
    }

    fn defer(
        &self,
        checks: Vec<TipCheck>,
        cached: Vec<Option<CachedResult>>,
        mut unlisted: Vec<Unlisted>,
        defer_platforms: &BTreeMap<String, Vec<Os>>,
    ) -> Required {
        let defers_family = |os, family: &str| {
            defer_platforms
                .get(family)
                .is_some_and(|platforms| platforms.contains(&os))
        };
        let mut deferred = Deferrals::new();
        let (checks, cached) = checks
            .into_iter()
            .zip(cached)
            .filter(|(check, hit)| {
                let passed = check.reuse
                    && hit
                        .as_ref()
                        .is_some_and(|hit| matches!(hit.outcome, Outcome::Pass | Outcome::Flaky));
                if let Some(os) = check.key.platform
                    && self.only.is_none()
                    && !passed
                    && (defers_family(os, &check.family) || self.fleet_deferred.contains(&os))
                {
                    deferred.entry(os).or_default().push(check.check.clone());
                    return false;
                }
                true
            })
            .unzip();
        unlisted.retain_mut(|missing| {
            if self.only.is_some() || missing.platforms.is_empty() {
                return true;
            }
            missing.platforms.retain(|os| {
                if defers_family(*os, &missing.family) || self.fleet_deferred.contains(os) {
                    deferred.entry(*os).or_default().push(missing.name.clone());
                    return false;
                }
                true
            });
            missing.reportable &= self.local.is_none_or(|os| missing.platforms.contains(&os));
            !missing.platforms.is_empty()
        });
        Required {
            checks,
            cached,
            unlisted,
            deferred,
        }
    }

    /// After the local gate passes, shares the native results it left until after promotion.
    pub fn publish_budget(&self, ctx: &Context, tip: &Commit, after: &[&TipCheck]) -> Result<()> {
        if let Some(os) = self.local {
            let after: Vec<_> = after
                .iter()
                .filter(|check| check.key.platform == Some(os))
                .map(|check| check.key.clone())
                .collect();
            if !after.is_empty() {
                ctx.hub.put_tip_budget(&TipBudgetPlan {
                    commit: tip.clone(),
                    platform: os,
                    after,
                })?;
            }
        }
        Ok(())
    }

    /// A peer's split excuses only the keys it selected for after promotion.
    pub fn peer_after(
        &self,
        ctx: &Context,
        tip: &Commit,
        checks: &[TipCheck],
        verdicts: &HashMap<ResultKey, TipRun>,
    ) -> Result<HashSet<ResultKey>> {
        if self.only.is_some() {
            return Ok(HashSet::new());
        }
        let platforms: BTreeSet<_> = checks
            .iter()
            .filter(|check| !check.runnable && check.reuse && !check.is_build())
            .filter(|check| {
                verdicts
                    .get(&check.key)
                    .is_none_or(|run| run.outcome == Outcome::Incomplete)
            })
            .filter_map(|check| check.key.platform)
            .collect();
        let mut after = HashSet::new();
        for os in platforms {
            if let Some(plan) = ctx.hub.tip_budget(tip, os)? {
                after.extend(plan.after);
            }
        }
        Ok(after)
    }

    pub fn record_deferrals(
        &self,
        ctx: &Context,
        last: &Commit,
        tip: &Commit,
        deferred: &Deferrals,
    ) -> Result<()> {
        // Save the gap before pushing master, so a crash between the two cannot lose the
        // first missed tip or its blame range.
        for &platform in deferred.keys() {
            self.advance(ctx, platform, tip, Some(last))?;
        }
        Ok(())
    }

    pub fn record_pass(
        &self,
        ctx: &Context,
        tip: &Commit,
        checks: &[TipCheck],
        verdicts: &HashMap<ResultKey, TipRun>,
        unlisted: &[Unlisted],
        deferred_here: bool,
    ) -> Result<bool> {
        let Some(os) = self
            .local
            .filter(|_| deferred_here || self.catching_up.is_some())
        else {
            return Ok(false);
        };
        let passed = checks
            .iter()
            .filter(|check| check.key.platform == Some(os))
            .all(|check| {
                verdicts
                    .get(&check.key)
                    .is_some_and(|run| matches!(run.outcome, Outcome::Pass | Outcome::Flaky))
            });
        if passed
            && !unlisted
                .iter()
                .any(|missing| missing.platforms.contains(&os))
        {
            self.advance(ctx, os, tip, None)?;
            return Ok(true);
        }
        Ok(false)
    }

    /// Compare commits by ancestry and retry if another round updated the platform meanwhile.
    /// An old defer cannot reopen a gap already covered by an equal or newer passing tip.
    fn advance(
        &self,
        ctx: &Context,
        platform: Os,
        tip: &Commit,
        deferring_from: Option<&Commit>,
    ) -> Result<()> {
        let mut expected = self.coverage.platforms.get(&platform).cloned();
        loop {
            if let Some(prior) = &expected {
                if !ctx.git.is_ancestor(&prior.through, tip)? {
                    return Ok(());
                }
                if prior.through == *tip && (deferring_from.is_some() || prior.deferred.is_none()) {
                    return Ok(());
                }
            }
            let deferred = deferring_from.map(|last_good| {
                expected
                    .as_ref()
                    .and_then(|prior| prior.deferred.clone())
                    .unwrap_or_else(|| DeferredPlatform {
                        since: tip.clone(),
                        last_good: last_good.clone(),
                    })
            });
            let update = TipCoverageUpdate {
                platform,
                checks_deferred: deferring_from.is_some()
                    && !self.fleet_deferred.contains(&platform),
                expected,
                replacement: PlatformVerification {
                    through: tip.clone(),
                    deferred,
                },
            };
            if ctx.hub.update_tip_coverage(&update)? {
                return Ok(());
            }
            expected = ctx.hub.tip_coverage()?.platforms.remove(&platform);
        }
    }
}
