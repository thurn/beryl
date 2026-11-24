//! Keying a family's listed checks: the paths each check's input hash covers, their git object
//! IDs from one reading of the tree, and the hash of the listing's sources (see "Input hashes"
//! in `docs/ignite.md`).

use std::collections::{BTreeSet, HashMap};

use ignite_core::hash;
use ignite_core::ids::{InputHash, Oid};
use ignite_core::inputs;
use ignite_core::manifest::{Family, InputPattern};
use ignite_core::protocol::hub::ResultKey;
use ignite_core::traits::{Error, Result};

use super::listings::Listing;
use super::run_reads::{self, RunReads};
use super::{Checks, Failure, Keyed, Listed, OpenTree, incomplete, invalid_entry, observed};
use crate::server::Context;

impl Checks {
    /// Keys a listing's checks, and hashes its sources, from one reading of the tree. Returns
    /// the keyed checks, the checks whose inputs cannot be resolved, and the sources hash
    /// (`None` when the tree could not be read).
    ///
    /// A `nextest` family's checks also hash the files each test opened at run time, as its
    /// last run at the same coarse hash recorded them (see [`run_reads`]).
    pub(super) fn key_listing(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        (name, family): (&str, &Family),
        listing: &Listing,
    ) -> (Vec<Keyed>, Vec<Failure>, Option<InputHash>) {
        let mut keyed = Vec::new();
        let mut broken = Vec::new();
        // Each check's input paths first, then the object IDs of all of them from one reading
        // of the tree. Reading per check ran two git commands per check, which for the
        // thousands of checks in a round took most of an hour under load.
        let mut resolved = Vec::new();
        // Checks of one package usually share every pattern, and matching patterns against
        // every tracked file is the costly part, so each distinct set is matched once.
        let mut memo: HashMap<String, Vec<String>> = HashMap::new();
        // The observed reads that are tracked in this tree: a file the build read that is gone
        // now names no path, which changes the hash.
        let reads = listing
            .reads
            .as_ref()
            .map(|reads| observed::tracked_only(reads, &tree.tracked));
        let lists = if run_reads::records(family) {
            let packages: BTreeSet<String> = listing
                .checks
                .iter()
                .filter_map(|l| l.package.clone())
                .collect();
            Some(self.read_lists(ctx, tree, (name, family), &packages))
        } else {
            None
        };
        for listed in &listing.checks {
            if let Some(why) = &listed.refused {
                eprintln!("ignite: {why}");
                broken.push(invalid_entry(&listed.check, &Error::Invalid(why.clone())));
                continue;
            }
            match self.key_paths(tree, family, listed, reads.as_ref(), &mut memo) {
                Ok(mut paths) => {
                    let run = lists.as_ref().map_or(RunReads::NotRecorded, |lists| {
                        run_reads::for_check(lists, listed, &paths, &tree.tracked)
                    });
                    if let RunReads::Known { added, .. } = &run {
                        paths.extend(added.iter().cloned());
                        paths.sort();
                    }
                    resolved.push((listed, paths, run));
                }
                Err(error) => broken.push(invalid_entry(&listed.check, &error)),
            }
        }
        let mut sources = Self::source_paths(tree, &listing.checks);
        // A kept listing holds only while the files its build was observed to read are as they
        // were, since a build script may generate tests from them.
        sources.extend(reads.iter().flat_map(observed::every_path));
        sources.sort();
        sources.dedup();
        let mut every: Vec<String> = resolved
            .iter()
            .flat_map(|(_, paths, _)| paths.iter().cloned())
            .chain(sources.iter().cloned())
            .collect();
        every.sort();
        every.dedup();
        let ids = match inputs::object_ids(ctx.git.as_ref(), &tree.path, &every) {
            Ok(ids) => ids,
            Err(error) => {
                for (listed, _, _) in resolved {
                    broken.push(incomplete(&listed.check, &error));
                }
                return (keyed, broken, None);
            }
        };
        let ids: HashMap<String, Oid> = ids.into_iter().collect();
        let own_ids = |paths: &[String]| -> Vec<(String, Oid)> {
            let mut own: Vec<(String, Oid)> = paths
                .iter()
                .filter_map(|p| ids.get(p).map(|oid| (p.clone(), oid.clone())))
                .collect();
            own.sort();
            own
        };
        for (listed, paths, run) in resolved {
            keyed.push(self.keyed(family, listed.clone(), own_ids(&paths), run));
        }
        // A family whose listing ignores comments compares its sources by their code alone.
        let sources = Self::sources_hash(&Self::listing_ids(ctx, tree, family, own_ids(&sources)));
        (keyed, broken, Some(sources))
    }

    /// The paths a check's input hash covers: its family's inputs, the family's
    /// `package_inputs` for the check's package, the check's own inputs and, where the listing
    /// carries observed reads, what the packages of the check's closure were observed to read.
    /// `memo` holds the paths already resolved for a set of patterns and a package.
    fn key_paths(
        &self,
        tree: &OpenTree,
        family: &Family,
        listed: &Listed,
        reads: Option<&observed::Reads>,
        memo: &mut HashMap<String, Vec<String>>,
    ) -> Result<Vec<String>> {
        let mut patterns = self.check_patterns(family, reads.is_some());
        patterns.extend_from_slice(family.package_patterns(listed.package.as_deref()));
        patterns.extend(listed.inputs.iter().cloned().map(InputPattern::from));
        let memo_key = format!("{:?}\0{patterns:?}", listed.package);
        if let Some(paths) = memo.get(&memo_key) {
            return Ok(paths.clone());
        }
        let mut paths = inputs::resolve(
            &patterns,
            listed.package.as_deref(),
            &tree.graph,
            &tree.tracked,
        )
        .map_err(|e| Error::Invalid(e.to_string()))?;
        if let (Some(reads), Some(package)) = (reads, listed.package.as_deref()) {
            // A package the crate graph does not hold has no compile units of its own to read.
            let closure =
                inputs::closure(package, &tree.graph).unwrap_or_else(|_| vec![package.to_owned()]);
            paths.extend(observed::paths_for(reads, &closure));
            paths.sort();
            paths.dedup();
        }
        memo.insert(memo_key, paths.clone());
        Ok(paths)
    }

    /// The patterns every check of a family hashes: its inputs and those of the products it
    /// needs. With observed reads, a product's path patterns are left out and its crate
    /// patterns kept: a product lists the files its build reads so that it is rebuilt when they
    /// change, and the observed reads already name every repository file a Cargo build read.
    pub(super) fn check_patterns(&self, family: &Family, observed: bool) -> Vec<InputPattern> {
        let manifest = &self.settings.manifest;
        if !observed {
            return hash::input_patterns(family, manifest);
        }
        let mut patterns = family.inputs.clone();
        for product in family.needs.iter().filter_map(|n| manifest.products.get(n)) {
            patterns.extend(
                product
                    .inputs
                    .iter()
                    .filter(|p| !matches!(p, InputPattern::Path(_)))
                    .cloned(),
            );
        }
        patterns
    }

    /// The check's key from the object IDs of its paths, sorted by path.
    pub(super) fn keyed(
        &self,
        family: &Family,
        listed: Listed,
        ids: Vec<(String, Oid)>,
        reads: RunReads,
    ) -> Keyed {
        let key = ResultKey {
            check: listed.check.clone(),
            hash: self.hash(family, &ids, listed.digest.as_deref()),
            platform: hash::platform(&family.platforms, self.settings.os),
        };
        Keyed {
            listed,
            key,
            inputs: ids,
            reads,
        }
    }
}
