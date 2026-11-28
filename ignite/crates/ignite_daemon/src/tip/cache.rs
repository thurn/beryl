//! The result cache in a round: the checks other machines answered meanwhile, and the records
//! that let the hub count families answered wholly from the cache.

use std::collections::{BTreeSet, HashMap, HashSet};

use ignite_core::ids::{CheckName, Commit, Handle};
use ignite_core::protocol::hub::{CheckState, Record, ResultKey};
use ignite_core::protocol::{Class, Outcome};
use ignite_core::time::Timestamp;
use ignite_core::traits::Result;

use super::{TipCheck, TipRun, TipVerifier, verdict};
use crate::server::Context;

/// How long the tip loop keeps a family's check states before it reads them again. A check
/// switched off at the hub makes a settled failed tip due again within this time.
pub(super) const CHECK_STATES_TTL_S: f64 = 120.0;

/// Each family's check states as the hub last reported them, with when they were read.
pub(super) type CheckStates = HashMap<String, (Timestamp, HashMap<CheckName, CheckState>)>;

impl TipVerifier {
    /// Looks up the checks still without a verdict, which other machines may have passed by
    /// now. A failure from another machine that this one can run is left for the next round to
    /// run here.
    pub(super) fn fill_from_cache<'a>(
        ctx: &Context,
        checks: impl IntoIterator<Item = &'a TipCheck>,
        verdicts: &mut HashMap<ResultKey, TipRun>,
    ) -> Result<()> {
        let missing: Vec<&TipCheck> = checks
            .into_iter()
            .filter(|c| c.reuse && !verdicts.contains_key(&c.key))
            .collect();
        if missing.is_empty() {
            return Ok(());
        }
        let keys: Vec<ResultKey> = missing.iter().map(|c| c.key.clone()).collect();
        for (check, hit) in missing.into_iter().zip(ctx.hub.lookup(&keys)?) {
            let Some(hit) = hit else { continue };
            let foreign_failure =
                hit.outcome == Outcome::Fail && check.runnable && hit.machine != ctx.machine;
            if !foreign_failure {
                verdicts.insert(check.key.clone(), verdict(check, hit.outcome, hit.output));
            }
        }
        Ok(())
    }

    /// Refreshes deferred checks even if we saw a pass earlier. A newly cached failure
    /// must be confirmed on a runnable check before promotion. A peer's native failure
    /// already stands here because this verifier cannot confirm it.
    pub(super) fn refresh_deferred<'a>(
        ctx: &Context,
        checks: impl Iterator<Item = &'a TipCheck>,
        verdicts: &mut HashMap<ResultKey, TipRun>,
    ) -> Result<HashSet<ResultKey>> {
        let checks: Vec<_> = checks.filter(|check| check.reuse).collect();
        let keys: Vec<_> = checks.iter().map(|check| check.key.clone()).collect();
        let mut confirm = HashSet::new();
        if !keys.is_empty() {
            for (check, hit) in checks.into_iter().zip(ctx.hub.lookup(&keys)?) {
                let Some(hit) = hit else { continue };
                let previous = verdicts.get(&check.key).map(|run| run.outcome);
                if hit.outcome == Outcome::Fail && check.runnable && previous != Some(Outcome::Fail)
                {
                    verdicts.remove(&check.key);
                    confirm.insert(check.key.clone());
                } else if hit.outcome == Outcome::Fail
                    || matches!(previous, None | Some(Outcome::Incomplete))
                {
                    verdicts.insert(check.key.clone(), verdict(check, hit.outcome, hit.output));
                }
            }
        }
        Ok(confirm)
    }

    /// Whether every check that failed at `tip` has recovered: the hub has a pass for its exact
    /// key, or it switched the check off (`ignite flaky off`, or upkeep). A switched-off check
    /// never records a pass, so without the second test a tip whose only failures were switched
    /// off stayed settled until another landing replaced it, and during a landing freeze
    /// `master` stood still with nothing failing that was switched on. A hub that does not
    /// answer leaves the tip settled. When stopping an active round, require that round's own
    /// failure record.
    pub(super) fn failure_recovered(
        &self,
        ctx: &Context,
        tip: &Commit,
        round: Option<&Handle>,
    ) -> bool {
        let Some(failed) = self.failed.as_ref().filter(|failed| {
            failed.commit == *tip
                && !failed.checks.is_empty()
                && round.is_none_or(|handle| failed.round.as_ref() == Some(handle))
        }) else {
            return false;
        };
        let Ok(hits) = ctx.hub.lookup(&failed.checks) else {
            return false;
        };
        if hits.len() != failed.checks.len() {
            return false;
        }
        let unpassed: Vec<&ResultKey> = hits
            .into_iter()
            .zip(&failed.checks)
            .filter(|(hit, key)| {
                !hit.as_ref()
                    .is_some_and(|hit| hit.key == **key && hit.outcome == Outcome::Pass)
            })
            .map(|(_, key)| key)
            .collect();
        unpassed.is_empty() || self.switched_off(ctx, &unpassed)
    }

    /// Whether the hub has switched off every check of `keys`. Each family's states are read
    /// at most once per [`CHECK_STATES_TTL_S`], and a read that fails is not kept, so the next
    /// turn asks again.
    fn switched_off(&self, ctx: &Context, keys: &[&ResultKey]) -> bool {
        let now = ctx.clock.now();
        let families: BTreeSet<&str> = keys.iter().map(|key| key.check.family()).collect();
        let mut cache = self
            .check_states
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for family in families {
            let fresh = cache
                .get(family)
                .is_some_and(|(at, _)| now.seconds_since(*at) < CHECK_STATES_TTL_S);
            if fresh {
                continue;
            }
            // The error is not printed: the tip loop asks on every turn while the hub is down.
            let Ok(states) = self.work.check_states(ctx, family) else {
                return false;
            };
            cache.insert(family.to_owned(), (now, states));
        }
        keys.iter().all(|key| {
            cache
                .get(key.check.family())
                .and_then(|(_, states)| states.get(&key.check))
                == Some(&CheckState::Off)
        })
    }

    /// Sends one record with the verb `cached` and no cost for each family whose checks were
    /// all answered by the result cache, so the hub can count cache hits.
    pub(super) fn record_cached<'a>(
        &self,
        ctx: &Context,
        tip: &Commit,
        checks: impl IntoIterator<Item = &'a TipCheck>,
        to_run: &[&TipCheck],
        recheck: &[&TipCheck],
    ) {
        let families: BTreeSet<&str> = checks
            .into_iter()
            .filter(|c| c.runnable)
            .map(|c| c.family.as_str())
            .collect();
        let records: Vec<Record> = families
            .into_iter()
            .filter(|family| !to_run.iter().chain(recheck).any(|c| c.family == *family))
            .map(|family| Record {
                machine: ctx.machine.clone(),
                ticket: None,
                session: None,
                tree: None,
                verb: "cached".to_owned(),
                class: Class::Tip,
                family: Some(family.to_owned()),
                queued_s: 0.0,
                wall_s: 0.0,
                cpu_s: 0.0,
                instructions: None,
                peak_mb: 0,
                outcome: Some(Outcome::Pass),
                commit: Some(tip.clone()),
                ignite: self.ignite.clone(),
                at: ctx.clock.now(),
                compile_units: Vec::new(),
            })
            .collect();
        if records.is_empty() {
            return;
        }
        // Telemetry never decides a tip: a hub that refuses it is logged and the round goes on.
        if let Err(error) = ctx.hub.records(&records) {
            eprintln!("ignite: cache-hit records not sent: {error}");
        }
    }
}
