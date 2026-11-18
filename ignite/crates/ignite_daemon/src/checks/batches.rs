//! Running one family's checks in batches from one listing, for tip verification.
//!
//! [`Checks::run`] lists and keys the whole family each time it is called. A tip round runs a
//! family in batches of a few dozen checks so that machines can share the work, and listing a
//! family of thousands of checks once per batch cost most of a round's time. Here the tree is
//! opened and the family listed and keyed once, and each batch runs from that listing.

use ignite_core::ids::{CheckName, TreeId};
use ignite_core::protocol::Outcome;
use ignite_core::traits::Error;

use super::{CheckRun, Checks, RunOptions, incomplete};
use crate::server::Context;

impl Checks {
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
        claim: &dyn Fn(u32) -> bool,
    ) -> (CheckRun, usize) {
        let mut run = CheckRun::default();
        let tree = match self.open(tree) {
            Ok(tree) => tree,
            Err(error) => {
                for name in batches.iter().flat_map(|(_, names)| names) {
                    run.finish(name, Outcome::Incomplete, Some(incomplete(name, &error)));
                }
                return (run, 0);
            }
        };
        let prepared = self.prepare(ctx, &tree, &[family], options, &mut run);
        let family_failed = run.incomplete_families.iter().any(|(f, _)| f == family);
        let mut ran = 0;
        for (batch, names) in batches {
            if !claim(*batch) {
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
            self.run_prepared(ctx, &tree, wanted, options, &mut run);
        }
        (run, ran)
    }
}
