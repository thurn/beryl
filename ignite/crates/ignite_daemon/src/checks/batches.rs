//! Running one family's checks in batches from one listing, for tip verification.
//!
//! [`Checks::run`] lists and keys the whole family each time it is called. A tip round runs a
//! family in batches, each about a minute of recorded cost, so that machines can share the
//! work, and listing a family of thousands of checks once per batch cost most of a round's
//! time. Here the tree is opened and the family listed and keyed once, and each batch runs from
//! that listing.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};

use ignite_core::ids::{CheckName, TreeId};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::ResultKey;
use ignite_core::traits::Error;

use super::{CheckRun, Checks, RunOptions, incomplete};
use crate::server::Context;

pub struct BatchCallbacks<'a> {
    /// Claims a batch immediately before it runs.
    pub claim: &'a dyn Fn(u32) -> bool,
    /// Receives the exact result keys of failures after each batch.
    pub failed: &'a dyn Fn(&[ResultKey]),
    /// When raised (for example because a sibling family failed a required check), skips new
    /// builds and check runs in this family as well.
    pub stop: Option<&'a AtomicBool>,
}

impl Checks {
    /// Each family's `covered_by` from the manifest: the family whose checks find every fault
    /// its checks find. Tip verification runs only the covering family's check of a name both
    /// families list.
    pub(crate) fn covered_by(&self) -> &BTreeMap<String, String> {
        &self.settings.manifest.covered_by
    }

    /// The families declared `single_batch = true`, whose `run` command checks the whole
    /// family regardless of `{checks}` and therefore runs in one tip batch.
    pub(crate) fn single_batch(&self) -> BTreeSet<String> {
        self.settings
            .manifest
            .families
            .iter()
            .filter(|(_, declared)| declared.family.single_batch)
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Runs `batches` of the family `family` in `tree`, listing and keying the family once.
    /// `claim` is asked right before each batch, with the batch's number, and a batch it
    /// refuses is skipped. Returns the run and how many batches ran.
    pub(crate) fn run_batches(
        &self,
        ctx: &Context,
        tree: &TreeId,
        family: &str,
        batches: &[(u32, Vec<CheckName>)],
        options: &RunOptions,
        callbacks: &BatchCallbacks<'_>,
    ) -> (CheckRun, usize) {
        let mut run = CheckRun::default();
        let (checks, tree) = match self.open(ctx, tree) {
            Ok(opened) => opened,
            Err(error) => {
                for name in batches.iter().flat_map(|(_, names)| names) {
                    run.finish(name, Outcome::Incomplete, Some(incomplete(name, &error)));
                }
                return (run, 0);
            }
        };
        if callbacks
            .stop
            .is_some_and(|stop| stop.load(Ordering::SeqCst))
        {
            return (run, 0);
        }
        let prepared = checks.prepare(ctx, &tree, &[family], options, &mut run);
        let family_failed = run.incomplete_families.iter().any(|(f, _)| f == family);
        let mut ran = 0;
        for (batch, names) in batches {
            if callbacks
                .stop
                .is_some_and(|stop| stop.load(Ordering::SeqCst))
            {
                break;
            }
            if !(callbacks.claim)(*batch) {
                continue;
            }
            ran += 1;
            if family_failed {
                // The listing failed every try; the family's failure stands for its checks.
                continue;
            }
            let mut wanted = prepared.clone();
            for name in names {
                let listed = wanted.iter().any(|p| {
                    p.keyed.iter().any(|k| k.listed.check == *name)
                        || p.broken.iter().any(|f| f.check == *name)
                });
                if !listed {
                    let error = Error::NotFound(format!("{name} is not listed by its family"));
                    run.finish(name, Outcome::Incomplete, Some(incomplete(name, &error)));
                }
            }
            for part in &mut wanted {
                part.keep(|check| names.contains(check), &mut run);
            }
            let previous_failures = run.failed_keys.len();
            checks.run_prepared_with_stop(ctx, &tree, wanted, options, &mut run, callbacks.stop);
            (callbacks.failed)(&run.failed_keys[previous_failures..]);
        }
        (run, ran)
    }
}
