//! The result cache in a round: the checks other machines answered meanwhile, and the records
//! that let the hub count families answered wholly from the cache.

use std::collections::{BTreeSet, HashMap};

use ignite_core::ids::{Commit, Handle};
use ignite_core::protocol::hub::{Record, ResultKey};
use ignite_core::protocol::{Class, Outcome};
use ignite_core::traits::Result;

use super::{TipCheck, TipRun, TipVerifier, verdict};
use crate::server::Context;

impl TipVerifier {
    /// Looks up the checks still without a verdict, which other machines may have passed by
    /// now. A failure from another machine that this one can run is left for the next round to
    /// run here.
    pub(super) fn fill_from_cache(
        ctx: &Context,
        checks: &[TipCheck],
        verdicts: &mut HashMap<ResultKey, TipRun>,
    ) -> Result<()> {
        let missing: Vec<&TipCheck> = checks
            .iter()
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

    /// Checks the shared cache even when the local verdicts already contain failures.
    /// When stopping an active round, require that round's own failure record.
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
        ctx.hub.lookup(&failed.checks).is_ok_and(|hits| {
            hits.len() == failed.checks.len()
                && hits.into_iter().zip(&failed.checks).all(|(hit, key)| {
                    hit.is_some_and(|hit| hit.key == *key && hit.outcome == Outcome::Pass)
                })
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
