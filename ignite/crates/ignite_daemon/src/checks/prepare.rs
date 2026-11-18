//! Listing and keying the checks of a set of families, for every verb and for the landing gate,
//! which stops at the first fault it knows of (see "Landing" in `docs/ignite.md`).

use ignite_core::manifest::Family;
use ignite_core::protocol::hub::ListingKey;

use super::listings::{Kept, Listing, Origin};
use super::{CheckRun, Checks, ListError, OpenTree, Prepared, RunOptions};
use crate::server::Context;

impl Checks {
    /// Lists and keys the checks of each family named that this machine runs. A family whose
    /// listing fails is recorded in `run` as incomplete; a check whose inputs cannot be
    /// resolved is kept in `broken`.
    pub(crate) fn prepare(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        families: &[&str],
        options: &RunOptions,
        run: &mut CheckRun,
    ) -> Vec<Prepared> {
        let among: Vec<(&str, &Family)> = self
            .families()
            .filter(|(n, _)| families.contains(n))
            .collect();
        self.prepare_among(ctx, tree, &among, options, run)
    }

    /// [`Checks::prepare`] for exactly the families given, whether or not this machine runs
    /// them, which tip verification needs to know the checks other machines run.
    pub(crate) fn prepare_among(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        among: &[(&str, &Family)],
        options: &RunOptions,
        run: &mut CheckRun,
    ) -> Vec<Prepared> {
        self.prepare_families(ctx, tree, among, options, run, false)
    }

    /// [`Checks::prepare_among`] for the landing gate, which stops at the first fault it knows
    /// of: a check whose manifest entry is invalid, a listing that failed, or a build that
    /// failed. The families that can be listed without building anything (a kept listing, or a
    /// `list` that needs no product) are listed first, so such a fault refuses the landing
    /// before anything is built. The families not listed are named in `run.unlisted`.
    pub(crate) fn prepare_gate(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        among: &[(&str, &Family)],
        options: &RunOptions,
        run: &mut CheckRun,
    ) -> Vec<Prepared> {
        self.prepare_families(ctx, tree, among, options, run, true)
    }

    fn prepare_families(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        among: &[(&str, &Family)],
        options: &RunOptions,
        run: &mut CheckRun,
        fail_fast: bool,
    ) -> Vec<Prepared> {
        // Every family's listing hash from one reading of the tree, and one request for every
        // kept listing, so a hub that cannot be reached costs its timeout once, not once per
        // family.
        let hashes = self.listing_hashes(ctx, tree, among);
        let mut kept = self.kept_for(hashes.clone());
        let mut among = among.to_vec();
        if fail_fast {
            // A stable sort: the families that build nothing to be listed come first.
            among.sort_by_key(|&(name, family)| {
                let needs = family.list_needs.as_ref().unwrap_or(&family.needs);
                !(kept.contains_key(name) || needs.is_empty())
            });
        }
        let mut prepared: Vec<Prepared> = Vec::new();
        for &(name, family) in &among {
            if fail_fast && known_fault(&prepared, run) {
                run.unlisted.push(name.to_owned());
                continue;
            }
            let key = hashes.get(name).map(|hash| ListingKey {
                family: name.to_owned(),
                hash: hash.clone(),
            });
            let listed = self.list_remembering_foreign(
                ctx,
                tree,
                (name, family),
                options,
                run,
                key.clone(),
                kept.remove(name),
            );
            let Some(mut listing) = Self::listed_or_noted(name, listed, run) else {
                continue;
            };
            let (mut keyed, mut broken, mut sources) =
                self.key_listing(ctx, tree, family, &listing);
            if let Origin::Kept {
                key: old,
                sources: kept,
            } = &listing.origin
                && sources.as_ref() != Some(kept)
            {
                // The listing was made from other files than the tree's: list the family again.
                self.forget(old);
                self.forget_foreign(name);
                let listed = self.list(ctx, tree, name, family, options, run, key, None);
                let Some(fresh) = Self::listed_or_noted(name, listed, run) else {
                    continue;
                };
                listing = fresh;
                (keyed, broken, sources) = self.key_listing(ctx, tree, family, &listing);
            }
            if let (Origin::Made { key: Some(key) }, Some(sources)) = (&listing.origin, sources) {
                let kept = Kept {
                    sources,
                    checks: listing.checks,
                };
                self.keep(key.clone(), kept);
            }
            prepared.push(Prepared {
                family: name.to_owned(),
                keyed,
                broken,
            });
        }
        prepared
    }

    /// The listing, or `None` after recording in `run` why there is none.
    fn listed_or_noted(
        name: &str,
        listed: std::result::Result<Listing, ListError>,
        run: &mut CheckRun,
    ) -> Option<Listing> {
        match listed {
            Ok(listing) => Some(listing),
            // The build is already in `run.build_failures`; the family's list never ran, so it
            // is not an incomplete family.
            Err(ListError::Build) => None,
            Err(ListError::Output(output)) => {
                run.incomplete_families.push((name.to_owned(), output));
                None
            }
        }
    }
}

/// Whether listing has already found a fault that refuses a landing: a failed build or
/// listing, or a listed check that cannot be keyed, such as one whose manifest entry is invalid.
fn known_fault(prepared: &[Prepared], run: &CheckRun) -> bool {
    !run.build_failures.is_empty()
        || !run.incomplete_families.is_empty()
        || prepared.iter().any(|p| !p.broken.is_empty())
}
