//! Listing a family's checks, and keeping each listing so it is made once for the inputs it was
//! made from (see "Checks" in `docs/ignite.md`).
//!
//! A listing is kept under the family's name and its listing hash: the hash of everything the
//! family's `list` command can read, which is its `inputs`, its `package_inputs`, the inputs of
//! the products it needs, its commands and the toolchain, with `crate-closure` standing for
//! every workspace package. It is looked for in this daemon's memory, then in the local
//! database, then at the hub, and the family is built and listed only when all three miss. A
//! hub that cannot be reached is a miss. A listing made here is kept in all three, so a listing
//! one machine made serves every other machine whose tree has the same listing hash.
//!
//! A listing also records the hash of its sources: what its checks' own inputs name, and the
//! directory holding each file among them, so a new file beside a check's file (a new test
//! module, a new script) also counts. A kept listing whose sources differ in the tree at hand
//! was made from other files, and the family is listed again.
//!
//! Three kinds of listing are never kept: one with no checks (a `nextest` family always lists
//! tests, and a `jsonl` family that listed checks before does not suddenly list none, so such a
//! listing is a failed one), one in which a check carries a digest (it stands for something no
//! file holds, such as the commit subjects since staging, so the listing hash cannot say when it
//! changes), and one with checks Ignite refused.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, PoisonError};

use serde::{Deserialize, Serialize};

use ignite_core::hash::{self, HashParts};
use ignite_core::ids::InputHash;
use ignite_core::inputs;
use ignite_core::manifest::{Adapter, Family, InputPattern};
use ignite_core::protocol::hub::ListingKey;

use super::{
    CheckRun, Checks, ListError, Listed, OpenTree, RunOptions, TRIES, adapters, base_env, execute,
};
use crate::server::Context;

/// How many listings of one family a daemon keeps in memory. Trees at different commits ask
/// for different ones, and a Rust test family's listing is megabytes.
const IN_MEMORY: usize = 6;

/// Puts a listing in memory, first forgetting the family's others when it holds too many.
fn remember(memory: &mut HashMap<ListingKey, Arc<Kept>>, key: ListingKey, kept: Arc<Kept>) {
    let held = memory.keys().filter(|k| k.family == key.family).count();
    if held >= IN_MEMORY {
        memory.retain(|k, _| k.family != key.family);
    }
    memory.insert(key, kept);
}

/// A family's listing as it is kept in memory, in the local database and at the hub.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Kept {
    /// The hash of the listing's sources in the tree it was made in (see [`Checks::sources`]).
    pub sources: InputHash,
    pub checks: Vec<Listed>,
}

/// Where a family's checks came from.
#[derive(Clone, Debug)]
pub(crate) enum Origin {
    /// The listing kept under `key`, which holds while the tree's sources hash to `sources`.
    Kept { key: ListingKey, sources: InputHash },
    /// Listed now; `key` is where to keep it, or `None` when it must not be kept.
    Made { key: Option<ListingKey> },
}

/// A family's checks and where they came from.
#[derive(Clone, Debug)]
pub(crate) struct Listing {
    pub checks: Vec<Listed>,
    pub origin: Origin,
}

impl Checks {
    /// The patterns a family's `list` can read: its inputs, those of every product it needs to
    /// list or run, and every package's `package_inputs`.
    pub(crate) fn listing_patterns(&self, family: &Family) -> Vec<InputPattern> {
        let manifest = &self.settings.manifest;
        let mut every = hash::input_patterns(family, manifest);
        let list_needs = family.list_needs.iter().flatten();
        for product in list_needs.filter_map(|n| manifest.products.get(n)) {
            every.extend(product.inputs.iter().cloned());
        }
        every.extend(family.package_inputs.values().flatten().cloned());
        every
    }

    /// The listing hash of each family given, from one reading of the tree. A family whose
    /// patterns cannot be resolved is left out, and is listed every time.
    pub(crate) fn listing_hashes(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        among: &[(&str, &Family)],
    ) -> HashMap<String, InputHash> {
        let mut resolved: Vec<(&str, &Family, Vec<String>)> = Vec::new();
        for &(name, family) in among {
            let patterns: Vec<InputPattern> = self
                .listing_patterns(family)
                .into_iter()
                .flat_map(|pattern| match pattern {
                    InputPattern::CrateClosure => tree
                        .graph
                        .packages
                        .iter()
                        .map(|p| InputPattern::Crate(p.name.clone()))
                        .collect(),
                    other => vec![other],
                })
                .collect();
            if let Ok(paths) = inputs::resolve(&patterns, None, &tree.graph, &tree.tracked) {
                resolved.push((name, family, paths));
            }
        }
        let every: BTreeSet<&String> = resolved.iter().flat_map(|(_, _, p)| p).collect();
        let every: Vec<String> = every.into_iter().cloned().collect();
        let Ok(ids) = inputs::object_ids(ctx.git.as_ref(), &tree.path, &every) else {
            return HashMap::new();
        };
        let ids: HashMap<String, _> = ids.into_iter().collect();
        resolved
            .into_iter()
            .map(|(name, family, paths)| {
                let own: Vec<_> = paths
                    .iter()
                    .filter_map(|p| ids.get(p).map(|oid| (p.clone(), oid.clone())))
                    .collect();
                (name.to_owned(), self.hash(family, &own, None))
            })
            .collect()
    }

    /// The kept listing of each family given that has one for the tree's inputs, with its key.
    pub(crate) fn kept_listings(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        among: &[(&str, &Family)],
    ) -> HashMap<String, (ListingKey, Arc<Kept>)> {
        self.kept_for(self.listing_hashes(ctx, tree, among))
    }

    /// The kept listing of each family in `hashes` that has one under its listing hash, with
    /// its key. Memory is asked first, then the local database and the hub, in one request for
    /// all the families memory lacks.
    pub(crate) fn kept_for(
        &self,
        hashes: HashMap<String, InputHash>,
    ) -> HashMap<String, (ListingKey, Arc<Kept>)> {
        let mut found = HashMap::new();
        let mut asked = Vec::new();
        {
            let memory = self.listings.lock().unwrap_or_else(PoisonError::into_inner);
            let not_kept = self.not_kept.lock().unwrap_or_else(PoisonError::into_inner);
            for (name, hash) in hashes {
                let key = ListingKey { family: name, hash };
                if let Some(kept) = memory.get(&key) {
                    found.insert(key.family.clone(), (key, Arc::clone(kept)));
                } else if !not_kept.contains(&key) {
                    asked.push(key);
                }
            }
        }
        if asked.is_empty() {
            return found;
        }
        let answers = self.cache.listings(&asked);
        let mut memory = self.listings.lock().unwrap_or_else(PoisonError::into_inner);
        for (key, answer) in asked.into_iter().zip(answers) {
            let Some(kept) = answer.and_then(|text| serde_json::from_str::<Kept>(&text).ok())
            else {
                continue;
            };
            let kept = Arc::new(kept);
            remember(&mut memory, key.clone(), Arc::clone(&kept));
            found.insert(key.family.clone(), (key, kept));
        }
        found
    }

    /// The patterns that a listing's sources hash covers: each check's own inputs, and the
    /// directory holding each tracked file among them.
    pub(crate) fn source_patterns(tree: &OpenTree, checks: &[Listed]) -> Vec<InputPattern> {
        let tracked: HashSet<&str> = tree.tracked.iter().map(String::as_str).collect();
        let mut patterns = BTreeSet::new();
        for text in checks.iter().flat_map(|c| &c.inputs) {
            if tracked.contains(text.as_str())
                && let Some((dir, _)) = text.rsplit_once('/')
            {
                patterns.insert(dir.to_owned());
            }
            patterns.insert(text.clone());
        }
        patterns.into_iter().map(InputPattern::from).collect()
    }

    /// The paths of a listing's sources in the tree. A pattern that names nothing now names no
    /// path, so a file that is gone changes the hash.
    pub(crate) fn source_paths(tree: &OpenTree, checks: &[Listed]) -> Vec<String> {
        let mut paths = BTreeSet::new();
        for pattern in Self::source_patterns(tree, checks) {
            let one = std::slice::from_ref(&pattern);
            if let Ok(found) = inputs::resolve(one, None, &tree.graph, &tree.tracked) {
                paths.extend(found);
            }
        }
        paths.into_iter().collect()
    }

    /// The hash of a listing's sources from their object IDs, sorted by path.
    pub(crate) fn sources_hash(ids: &[(String, ignite_core::ids::Oid)]) -> InputHash {
        hash::input_hash(&HashParts {
            inputs: ids,
            digest: None,
            commands: &[],
            toolchain: "",
            platform: None,
        })
    }

    /// Keeps a listing made here, in memory, locally and at the hub.
    pub(crate) fn keep(&self, key: ListingKey, kept: Kept) {
        let Ok(text) = serde_json::to_string(&kept) else {
            return;
        };
        self.cache.store_listing(&key, &text);
        let mut memory = self.listings.lock().unwrap_or_else(PoisonError::into_inner);
        remember(&mut memory, key, Arc::new(kept));
    }

    /// Forgets a kept listing that turned out not to hold for a tree, so the next tree with
    /// the same listing hash is not offered it from memory.
    pub(crate) fn forget(&self, key: &ListingKey) {
        let mut memory = self.listings.lock().unwrap_or_else(PoisonError::into_inner);
        memory.remove(key);
    }

    /// The family's checks: `kept` when there is a kept listing, or else a new listing, to be
    /// kept under `key` if it may be. `Err` holds the output of the last failed try.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn list(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        name: &str,
        family: &Family,
        options: &RunOptions,
        run: &mut CheckRun,
        key: Option<ListingKey>,
        kept: Option<(ListingKey, Arc<Kept>)>,
    ) -> std::result::Result<Listing, ListError> {
        if let Some((key, kept)) = kept {
            self.note_listed(name, &kept.checks);
            return Ok(Listing {
                checks: kept.checks.clone(),
                origin: Origin::Kept {
                    key,
                    sources: kept.sources.clone(),
                },
            });
        }
        let _listing = crate::steps::during(format!("listing family {name} in {}", tree.id));
        // `list` may read what a product builds, so it waits for the products it needs.
        let needs = family.list_needs.as_ref().unwrap_or(&family.needs);
        let who = execute::build_for(options);
        Self::build_products(ctx, tree, needs, &[], &who, run).map_err(|_| ListError::Build)?;
        let (args, cwd) = match family.adapter {
            Adapter::Jsonl => (
                adapters::words(family.list.as_deref().unwrap_or_default()),
                tree.path.clone(),
            ),
            Adapter::Nextest => (
                adapters::nextest_list_command(family.list.as_deref()),
                self.nextest_dir(tree, family),
            ),
        };
        let mut output = PathBuf::new();
        for _ in 0..TRIES {
            let env = base_env(ctx);
            let result = self.launch(
                ctx,
                tree,
                name,
                family,
                execute::LIST,
                args.clone(),
                &cwd,
                env,
                options,
            );
            output.clone_from(&result.output);
            if result.exit != Some(0) {
                let _ = crate::supervise::keep(&result.output);
                if result.hung {
                    // A hung command hangs again.
                    break;
                }
                continue;
            }
            let Ok(text) = fs::read_to_string(&result.output) else {
                continue;
            };
            let listed = match family.adapter {
                Adapter::Jsonl => adapters::jsonl_list(&text, name),
                Adapter::Nextest => adapters::nextest_list(&text, name),
            };
            if listed.is_empty() && self.cannot_be_empty(name, family) {
                // A zero-check listing that exited 0 would count as a vacuous pass. It counts
                // as a failed try instead, and is never kept.
                let _ = crate::supervise::keep(&result.output);
                continue;
            }
            self.note_listed(name, &listed);
            let keepable = !listed.is_empty()
                && listed
                    .iter()
                    .all(|l| l.digest.is_none() && l.refused.is_none());
            let key = key.filter(|key| {
                if !keepable {
                    let mut not_kept = self.not_kept.lock().unwrap_or_else(PoisonError::into_inner);
                    not_kept.insert(key.clone());
                }
                keepable
            });
            return Ok(Listing {
                checks: listed,
                origin: Origin::Made { key },
            });
        }
        Err(ListError::Output(output))
    }

    /// Remembers that `name` has listed checks, if it has, so a later empty listing of it is
    /// taken for a broken `list` command.
    fn note_listed(&self, name: &str, listed: &[Listed]) {
        if !listed.is_empty() {
            let mut seen = self
                .listed_before
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            seen.insert(name.to_owned());
        }
    }

    /// Whether an empty listing of `family` is a failure. The manifest has no way to say a
    /// family may list nothing, so a Rust test family never may, and any other family may not
    /// once it has listed checks.
    fn cannot_be_empty(&self, name: &str, family: &Family) -> bool {
        family.adapter == Adapter::Nextest
            || self
                .listed_before
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .contains(name)
    }
}
