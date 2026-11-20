//! Listing a family's checks, and keeping each listing so it is made once for the inputs it was
//! made from (see "Checks" in `docs/ignite.md`).
//!
//! A listing is kept under the family's name and its listing hash: the hash of everything the
//! family's `list` command can read, which is its `inputs`, its `package_inputs`, the inputs of
//! the products it needs, its commands and the toolchain, with `crate-closure` standing for
//! every package of the workspaces the family reaches into ([`Checks::closure_packages`]). It is looked for in this daemon's memory, then in the local
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
//! listing is a failed one), one in which a check carries a digest of a family that declares no
//! `digest_inputs` (the digest may stand for something no file holds, such as the commit
//! subjects since staging, so the listing hash cannot say when it changes), and one with checks
//! Ignite refused. A family that declares `digest_inputs` says its digests are computed from
//! those files and from what its listing hash already covers, so the listing hash covers them
//! too and its listing is kept like any other.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, PoisonError};

use serde::{Deserialize, Serialize};

use ignite_core::hash::{self, HashParts};
use ignite_core::ids::InputHash;
use ignite_core::inputs;
use ignite_core::manifest::{Adapter, Family, InputPattern};
use ignite_core::protocol::hub::ListingKey;

use super::observed::{self, Reads};
use super::{
    CheckRun, Checks, ListError, Listed, ListingTime, OpenTree, RunOptions, TRIES, adapters,
    base_env, execute,
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
    /// For a `nextest` family, what each package's compile units were observed to read (see
    /// [`observed`]). A `nextest` listing without it, as older daemons kept, is not used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reads: Option<Reads>,
    /// For a `nextest` family, the hash of what can change each package's test names, by
    /// package (see `package_listings`). A listing without it is never reused for another
    /// tree's inputs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub packages: Option<BTreeMap<String, InputHash>>,
}

/// Where a family's checks came from.
#[derive(Clone, Debug)]
pub(crate) enum Origin {
    /// The listing kept under `key`, which holds while the tree's sources hash to `sources`.
    Kept { key: ListingKey, sources: InputHash },
    /// Listed now; `key` is where to keep it, or `None` when it must not be kept.
    Made { key: Option<ListingKey> },
    /// Listed now for some of the family's packages only (see [`Checks::list`]). It names
    /// none of the other packages' checks, so it is neither kept nor stored as the family's
    /// newest listing.
    Scoped,
}

/// A family's checks and where they came from.
#[derive(Clone, Debug)]
pub(crate) struct Listing {
    pub checks: Vec<Listed>,
    pub origin: Origin,
    /// What each package's compile units were observed to read, for a `nextest` family whose
    /// target directory could be read; `None` otherwise.
    pub reads: Option<Reads>,
}

impl Checks {
    /// The patterns a family's `list` can read: its inputs, those of every product it needs to
    /// list or run, every package's `package_inputs`, and the files its checks' digests are
    /// computed from (`digest_inputs`).
    pub(crate) fn listing_patterns(&self, family: &Family) -> Vec<InputPattern> {
        let manifest = &self.settings.manifest;
        let mut every = hash::input_patterns(family, manifest);
        let list_needs = family.list_needs.iter().flatten();
        for product in list_needs.filter_map(|n| manifest.products.get(n)) {
            every.extend(product.inputs.iter().cloned());
        }
        every.extend(family.package_inputs.values().flatten().cloned());
        every.extend(family.digest_inputs.iter().cloned());
        every
    }

    /// The packages `crate-closure` stands for in a family's listing hash: every package of
    /// each workspace the family's other inputs reach into, with the packages they depend on.
    /// A workspace is reached when one of the family's paths lies in it (or holds it), when a
    /// `crate:` pattern names one of its packages, or when a product the family needs is built
    /// in it. A family that reaches no workspace, or has a path pattern that starts with a
    /// wildcard, hashes every package, as before.
    ///
    /// A family's `list` enumerates the packages of its own workspaces, so a package in
    /// another workspace cannot change what it lists. Hashing every package made each change
    /// to Ignite's crates miss the kept listing of the game's Rust test families, which then
    /// built the game's tests only to list them again.
    pub(crate) fn closure_packages(
        &self,
        family: &Family,
        patterns: &[InputPattern],
        graph: &inputs::CrateGraph,
    ) -> Vec<String> {
        let every = || graph.packages.iter().map(|p| p.name.clone()).collect();
        let manifest = &self.settings.manifest;
        let mut dirs: Vec<String> = Vec::new();
        let mut workspaces: BTreeSet<&str> = BTreeSet::new();
        for pattern in patterns {
            match pattern {
                InputPattern::CrateClosure => {}
                InputPattern::Crate(package) => {
                    if let Some(found) = graph.packages.iter().find(|p| p.name == *package) {
                        workspaces.insert(found.workspace.as_str());
                    }
                }
                InputPattern::Path(text) => {
                    let literal: Vec<&str> = text
                        .split('/')
                        .take_while(|segment| !segment.contains(['*', '?', '[']))
                        .collect();
                    if literal.is_empty() {
                        return every();
                    }
                    dirs.push(literal.join("/"));
                }
            }
        }
        let products = family
            .needs
            .iter()
            .chain(family.list_needs.iter().flatten());
        dirs.extend(
            products
                .filter_map(|n| manifest.products.get(n))
                .filter_map(|p| p.dir.clone()),
        );
        for package in &graph.packages {
            let workspace = package.workspace.as_str();
            if dirs
                .iter()
                .any(|dir| within(dir, workspace) || within(workspace, dir))
            {
                workspaces.insert(workspace);
            }
        }
        if workspaces.is_empty() {
            return every();
        }
        let mut packages = BTreeSet::new();
        for package in graph
            .packages
            .iter()
            .filter(|p| workspaces.contains(p.workspace.as_str()))
        {
            match inputs::closure(&package.name, graph) {
                Ok(closure) => packages.extend(closure),
                Err(_) => {
                    packages.insert(package.name.clone());
                }
            }
        }
        packages.into_iter().collect()
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
            let listing = self.listing_patterns(family);
            let closure = self.closure_packages(family, &listing, &tree.graph);
            let patterns: Vec<InputPattern> = listing
                .into_iter()
                .flat_map(|pattern| match pattern {
                    InputPattern::CrateClosure => closure
                        .iter()
                        .map(|p| InputPattern::Crate(p.clone()))
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
                let own = Self::listing_ids(ctx, tree, family, own);
                (name.to_owned(), self.listing_hash(family, &own))
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
            let Some(kept) = answer
                .and_then(|text| serde_json::from_str::<Kept>(&text).ok())
                .filter(|kept| kept.reads.is_some() || !self.observes(&key.family))
            else {
                continue;
            };
            let kept = Arc::new(kept);
            remember(&mut memory, key.clone(), Arc::clone(&kept));
            found.insert(key.family.clone(), (key, kept));
        }
        found
    }

    /// The newest listing of `family` this machine keeps, whatever inputs it was made from. It
    /// may be out of date, so it only says which checks the family probably holds.
    pub(crate) fn latest_listing(&self, family: &str) -> Option<Kept> {
        let text = self.cache.latest_listing(family)?;
        serde_json::from_str(&text).ok()
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

    /// Stores, on this machine only, a listing that must not be kept for its inputs, such as one
    /// whose checks carry digests, so that [`Checks::latest_listing`] names its checks. Without
    /// it, `ignite test <word>` found no listing of such a family and listed it every time. Two
    /// such families whose `list` needs a large build were built and listed for a test of one
    /// card file, which took 274 s, 245 s of it listing families that held no such check.
    pub(crate) fn keep_names(&self, family: &str, kept: &Kept) {
        if let Ok(text) = serde_json::to_string(kept) {
            self.cache.store_unkept_listing(family, &text);
        }
    }

    /// Forgets a kept listing that turned out not to hold for a tree, so the next tree with
    /// the same listing hash is not offered it from memory.
    pub(crate) fn forget(&self, key: &ListingKey) {
        let mut memory = self.listings.lock().unwrap_or_else(PoisonError::into_inner);
        memory.remove(key);
    }

    /// The family's checks: `kept` when there is a kept listing, or else a new listing, to be
    /// kept under `key` if it may be. `Err` holds the output of the last failed try.
    ///
    /// With `scope`, a `nextest` family whose `list` says `{packages}` is built and listed for
    /// those packages only, unless building them alone would build the whole workspace anyway
    /// ([`ignite_core::traits::Builds::selected`]). A Rust test family's listing builds every
    /// test binary it lists, so after a change to one card file a listing of the whole
    /// workspace's tests recompiled 35 crates, 33 s alone and 77 s beside other builds, to run
    /// one test of one package.
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
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
        scope: Option<&[String]>,
    ) -> std::result::Result<Listing, ListError> {
        if let Some((key, kept)) = kept {
            run.listings_kept += 1;
            self.note_listed(name, &kept.checks);
            return Ok(Listing {
                checks: kept.checks.clone(),
                origin: Origin::Kept {
                    key,
                    sources: kept.sources.clone(),
                },
                reads: kept.reads.clone(),
            });
        }
        let _listing = crate::steps::during(format!("listing family {name} in {}", tree.id));
        // `list` may read what a product builds, so it waits for the products it needs.
        let needs = family.list_needs.as_ref().unwrap_or(&family.needs);
        let packages = match scope {
            Some(packages) if Self::lists_packages(family) => {
                Self::selected(ctx, tree, family, packages)
            }
            _ => Vec::new(),
        };
        let scoped = !packages.is_empty();
        let key = key.filter(|_| !scoped);
        let who = execute::build_for(options);
        self.build_products(ctx, tree, needs, &packages, &who, run)
            .map_err(|_| ListError::Build)?;
        let (args, cwd) = match family.adapter {
            Adapter::Jsonl => (
                adapters::words(family.list.as_deref().unwrap_or_default()),
                tree.path.clone(),
            ),
            Adapter::Nextest => (
                adapters::nextest_list_command(
                    family.list.as_deref(),
                    &packages,
                    &self.nextest_exclude(family),
                ),
                self.nextest_dir(tree, family),
            ),
        };
        let mut output = PathBuf::new();
        let mut time = ListingTime {
            family: name.to_owned(),
            ..ListingTime::default()
        };
        // The tries run in a closure so that their time is recorded however the listing ends.
        let listing = (|| {
            for _ in 0..TRIES {
                let mut env = base_env(ctx);
                // A Cargo command records its compiles, so the `listed` line can say how much of
                // the listing went to compiling.
                if family.adapter == Adapter::Nextest {
                    env.extend(self.unit_recording_env());
                }
                let (result, span) = self.launch(
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
                time.add_try(&result, span, ctx.clock.now().millis());
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
                let digests_from_files = !family.digest_inputs.is_empty();
                let keepable = !listed.is_empty()
                    && listed
                        .iter()
                        .all(|l| (l.digest.is_none() || digests_from_files) && l.refused.is_none());
                let key = key.filter(|key| {
                    if !keepable {
                        let mut not_kept =
                            self.not_kept.lock().unwrap_or_else(PoisonError::into_inner);
                        not_kept.insert(key.clone());
                    }
                    keepable
                });
                // `cargo nextest list` has just compiled every test binary it listed, so the
                // dep-info of every unit they use is current.
                let reads = (family.adapter == Adapter::Nextest)
                    .then(|| self.target_dir(family))
                    .flatten()
                    .and_then(|target| {
                        observed::observe(&tree.path, &target, &tree.graph, &tree.tracked)
                    });
                let origin = if scoped {
                    Origin::Scoped
                } else {
                    Origin::Made { key }
                };
                return Ok(Listing {
                    checks: listed,
                    origin,
                    reads,
                });
            }
            Err(ListError::Output(output))
        })();
        run.listings.push(time);
        listing
    }

    /// Whether the family's listings carry observed reads: a `nextest` family whose target
    /// directory is known.
    pub(crate) fn observes(&self, name: &str) -> bool {
        self.settings
            .manifest
            .families
            .get(name)
            .is_some_and(|declared| {
                declared.family.adapter == Adapter::Nextest
                    && self.target_dir(&declared.family).is_some()
            })
    }

    /// The target directory of a `nextest` family's builds, relative to the checkout's root:
    /// the first output of the first product it needs.
    pub(crate) fn target_dir(&self, family: &Family) -> Option<String> {
        let product = self.settings.manifest.products.get(family.needs.first()?)?;
        product.outputs.first().cloned()
    }

    /// The packages to list a family for when `ignite test` named only checks of `matched`, as
    /// its newest listing has them: those packages, and every package of their workspaces
    /// that holds a path of `changed`, the files the tree's branch changes, so that a test the
    /// branch added to a package is listed too.
    pub(crate) fn test_scope(
        tree: &OpenTree,
        matched: &BTreeSet<String>,
        changed: &[String],
    ) -> Vec<String> {
        let packages = &tree.graph.packages;
        let workspaces: BTreeSet<&str> = packages
            .iter()
            .filter(|p| matched.contains(&p.name))
            .map(|p| p.workspace.as_str())
            .collect();
        let mut scope = matched.clone();
        for package in packages {
            let holds = changed.iter().any(|path| within(path, &package.dir));
            if holds && workspaces.contains(package.workspace.as_str()) {
                scope.insert(package.name.clone());
            }
        }
        scope.into_iter().collect()
    }

    /// Whether the family's `list` can list some of its packages: a `nextest` family whose
    /// `list` says `{packages}`.
    pub(crate) fn lists_packages(family: &Family) -> bool {
        family.adapter == Adapter::Nextest
            && family
                .list
                .as_deref()
                .is_some_and(|list| list.split_whitespace().any(|w| w == "{packages}"))
    }

    /// Remembers that `name` has listed checks, if it has, so a later empty listing of it is
    /// taken for a broken `list` command.
    pub(super) fn note_listed(&self, name: &str, listed: &[Listed]) {
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

/// Whether `path` is `dir` or lies below it; every path lies in the checkout's root, `""`.
fn within(path: &str, dir: &str) -> bool {
    dir.is_empty()
        || path == dir
        || path
            .strip_prefix(dir)
            .is_some_and(|rest| rest.starts_with('/'))
}
