//! Deferring checks for platforms that are off and verifying those checks when they return.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use ignite_core::config::Os;
use ignite_core::ids::{CheckName, Commit};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::{CachedResult, ResultKey};
use ignite_core::tipround::{
    DeferredPlatform, PlatformVerification, TipCoverage, TipCoverageUpdate,
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
    off: BTreeSet<Os>,
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
        let off = verifier
            .fleet_notice()
            .map_or_else(BTreeSet::new, |notice| notice.off_platforms);
        let local = ctx
            .config
            .machine(&verifier.host)
            .and_then(|machine| machine.os)
            .or_else(Os::current);
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
            off,
            catching_up,
            only,
        })
    }

    pub fn required(
        &self,
        ctx: &Context,
        checks: Vec<TipCheck>,
        unlisted: Vec<Unlisted>,
    ) -> Result<Required> {
        let (checks, unlisted) = self.select(checks, unlisted);
        let keys: Vec<ResultKey> = checks.iter().map(|check| check.key.clone()).collect();
        let cached = if keys.is_empty() {
            Vec::new()
        } else {
            ctx.hub.lookup(&keys)?
        };
        Ok(self.defer(checks, cached, unlisted))
    }

    fn select(
        &self,
        mut checks: Vec<TipCheck>,
        mut unlisted: Vec<Unlisted>,
    ) -> (Vec<TipCheck>, Vec<Unlisted>) {
        if let Some(os) = self.only {
            checks.retain(|check| check.key.platform == Some(os));
            unlisted.retain(|missing| missing.platforms.contains(&os));
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
    ) -> Required {
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
                    && self.off.contains(&os)
                    && !check.runnable
                    && !passed
                {
                    deferred.entry(os).or_default().push(check.check.clone());
                    return false;
                }
                true
            })
            .unzip();
        unlisted.retain_mut(|missing| {
            if missing.reportable || missing.platforms.is_empty() {
                return true;
            }
            missing.platforms.retain(|os| {
                if self.off.contains(os) {
                    deferred.entry(*os).or_default().push(missing.name.clone());
                    return false;
                }
                true
            });
            !missing.platforms.is_empty()
        });
        for checks in deferred.values_mut() {
            checks.sort();
            checks.dedup();
        }
        Required {
            checks,
            cached,
            unlisted,
            deferred,
        }
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
    ) -> Result<()> {
        let Some((os, _)) = self.catching_up else {
            return Ok(());
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
        }
        Ok(())
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
