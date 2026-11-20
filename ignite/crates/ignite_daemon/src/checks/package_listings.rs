//! Reusing a `nextest` family's newest listing for the packages whose test names cannot have
//! changed (see "Kept listings" in `docs/ignite.md`).
//!
//! A `nextest` family's listing hash covers every package of its workspace, so a change to one
//! card file missed the kept listing of the game's test family, and listing it again built
//! every test binary of the workspace: 405 s and 97 crates for a one-line change. Yet a
//! package's test names are fixed by the package's own files, what its build script and
//! `include_str!` read (its observed reads), the workspace's manifests and lock file, and the
//! family's commands. No package here gets tests from another package's code: the repository
//! has no procedural macros and no exported macro that writes a test.
//!
//! So a `nextest` listing also keeps one hash per package over exactly those inputs. When a
//! tree misses the kept listing, the family's newest listing is compared package by package:
//! with no package changed it serves as it is and nothing is built; otherwise a family whose
//! `list` says `{packages}` is listed for the changed packages alone, and their tests replace
//! theirs in the newest listing. A family that cannot be listed for some packages is listed
//! whole, as before.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use ignite_core::ids::{InputHash, Oid};
use ignite_core::inputs;
use ignite_core::manifest::{Adapter, Family, InputPattern};
use ignite_core::protocol::hub::ListingKey;

use super::listings::{Kept, Listing, Origin};
use super::observed;
use super::{Checks, OpenTree};
use crate::server::Context;

/// What a `nextest` family's newest listing still says for the tree at hand.
#[derive(Debug)]
pub(crate) enum Reuse {
    /// No package's test names can have changed: the newest listing serves as it is.
    Whole(Kept),
    /// The packages in `changed` must be listed again; `base` holds every other package's.
    Partial { base: Kept, changed: Vec<String> },
}

impl Checks {
    /// How much of the newest listing of `name` the tree can reuse, or `None` when it must be
    /// listed whole: the family is not a `nextest` family this machine runs, it has no newest
    /// listing with package hashes, or a package changed and the family cannot be listed for
    /// some packages only.
    pub(crate) fn reuse(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        name: &str,
        family: &Family,
    ) -> Option<Reuse> {
        if family.adapter != Adapter::Nextest || !self.families().any(|(own, _)| own == name) {
            return None;
        }
        let base = self.latest_listing(name)?;
        let old = base.packages.clone()?;
        if base.checks.iter().any(|c| c.package.is_none()) {
            return None;
        }
        let now = self.package_hashes(ctx, tree, family, &base)?;
        let changed: BTreeSet<String> = now
            .keys()
            .chain(old.keys())
            .filter(|p| now.get(*p) != old.get(*p))
            .cloned()
            .collect();
        if changed.is_empty() {
            return Some(Reuse::Whole(Kept {
                packages: Some(now),
                ..base
            }));
        }
        if !Self::lists_packages(family) {
            return None;
        }
        // A package that is gone needs no listing: its tests leave with it.
        let changed = changed
            .into_iter()
            .filter(|p| now.contains_key(p))
            .collect();
        Some(Reuse::Partial { base, changed })
    }

    /// The listing made from `base` and a fresh listing of the `changed` packages: `base`'s
    /// tests of every other package, then the fresh tests, with the fresh observed reads of the
    /// changed packages. A fresh listing of the whole family is returned as it is.
    pub(crate) fn merged(
        base: Kept,
        changed: &[String],
        fresh: Listing,
        key: Option<ListingKey>,
    ) -> Listing {
        if !matches!(fresh.origin, Origin::Scoped) {
            return fresh;
        }
        // A package the fresh listing holds tests of is taken from it, even one that did not
        // change, so no test is listed twice.
        let mut changed: BTreeSet<String> = changed.iter().cloned().collect();
        changed.extend(fresh.checks.iter().filter_map(|c| c.package.clone()));
        let is_changed = |package: Option<&String>| package.is_some_and(|p| changed.contains(p));
        let mut checks: Vec<_> = base
            .checks
            .into_iter()
            .filter(|c| !is_changed(c.package.as_ref()))
            .collect();
        checks.extend(fresh.checks);
        let mut reads = base.reads.unwrap_or_default();
        reads.retain(|p, _| !changed.contains(p.as_str()));
        for (package, paths) in fresh.reads.unwrap_or_default() {
            if changed.contains(package.as_str()) {
                reads.insert(package, paths);
            }
        }
        Listing {
            checks,
            origin: Origin::Made { key },
            reads: Some(reads),
        }
    }

    /// The hash of what can change each package's test names, for every package whose tests
    /// `listing` may hold (see the module's header). `None` when the tree cannot be read, or a
    /// test of `listing` belongs to no package the hash can cover.
    pub(crate) fn package_hashes(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        family: &Family,
        listing: &Kept,
    ) -> Option<BTreeMap<String, InputHash>> {
        let packages = self.listed_packages(tree, family, listing);
        // A test of a package the crate graph does not hold, or that the family's commands
        // leave out, has no inputs this hash can name.
        let unknown = listing
            .checks
            .iter()
            .any(|c| c.package.as_ref().is_none_or(|p| !packages.contains(p)));
        if unknown {
            return None;
        }
        let reads = listing
            .reads
            .as_ref()
            .map(|reads| observed::tracked_only(reads, &tree.tracked))
            .unwrap_or_default();
        let shared = Self::shared_paths(tree, family, &packages);
        let mut own: Vec<(String, Vec<String>)> = Vec::new();
        for package in &packages {
            let Some(found) = tree.graph.packages.iter().find(|p| p.name == *package) else {
                continue;
            };
            let mut paths = shared.clone();
            paths.push(found.dir.clone());
            paths.extend(reads.get(package).into_iter().flatten().cloned());
            let extra = family.package_patterns(Some(package));
            if let Ok(more) = inputs::resolve(extra, Some(package), &tree.graph, &tree.tracked) {
                paths.extend(more);
            }
            paths.sort();
            paths.dedup();
            own.push((package.clone(), paths));
        }
        let mut every: Vec<String> = own.iter().flat_map(|(_, p)| p.iter().cloned()).collect();
        every.sort();
        every.dedup();
        let ids = inputs::object_ids(ctx.git.as_ref(), &tree.path, &every).ok()?;
        let ids: HashMap<String, Oid> = Self::listing_ids(ctx, tree, family, ids)
            .into_iter()
            .collect();
        Some(
            own.into_iter()
                .map(|(package, paths)| {
                    let found: Vec<(String, Oid)> = paths
                        .into_iter()
                        .filter_map(|p| ids.get(&p).map(|oid| (p, oid.clone())))
                        .collect();
                    (package, self.listing_hash(family, &found))
                })
                .collect(),
        )
    }

    /// The packages whose tests a `nextest` family's listing can hold: the packages of the
    /// workspaces its listed tests belong to, less those its whole-workspace command leaves
    /// out, and only those its `list` names with `-p` when it names any.
    fn listed_packages(&self, tree: &OpenTree, family: &Family, listing: &Kept) -> Vec<String> {
        let graph = &tree.graph;
        let holding: BTreeSet<&str> = listing
            .checks
            .iter()
            .filter_map(|c| c.package.as_deref())
            .filter_map(|p| graph.packages.iter().find(|found| found.name == p))
            .map(|p| p.workspace.as_str())
            .collect();
        let named = named_packages(family.list.as_deref().unwrap_or_default());
        let exclude = self.nextest_exclude(family);
        graph
            .packages
            .iter()
            .filter(|p| holding.contains(p.workspace.as_str()))
            .filter(|p| named.is_empty() || named.contains(&p.name))
            .filter(|p| !exclude.contains(&p.name))
            .map(|p| p.name.clone())
            .collect()
    }

    /// The paths every package's hash covers: each manifest, lock file and toolchain file of
    /// the workspaces holding `packages`, since a dependent's manifest can turn on a feature
    /// that adds a test, and the paths the family's `inputs` name outside `crate-closure`.
    fn shared_paths(tree: &OpenTree, family: &Family, packages: &[String]) -> Vec<String> {
        let graph = &tree.graph;
        let workspaces: BTreeSet<&str> = graph
            .packages
            .iter()
            .filter(|p| packages.contains(&p.name))
            .map(|p| p.workspace.as_str())
            .collect();
        let mut paths: Vec<String> = Vec::new();
        for package in graph
            .packages
            .iter()
            .filter(|p| workspaces.contains(p.workspace.as_str()))
        {
            paths.push(join(&package.dir, "Cargo.toml"));
        }
        for workspace in workspaces {
            for name in [
                "Cargo.toml",
                "Cargo.lock",
                "rust-toolchain.toml",
                "rust-toolchain",
            ] {
                paths.push(join(workspace, name));
            }
        }
        paths.retain(|p| tree.tracked.contains(p));
        let declared: Vec<InputPattern> = family
            .inputs
            .iter()
            .filter(|p| matches!(p, InputPattern::Path(_)))
            .cloned()
            .collect();
        if let Ok(more) = inputs::resolve(&declared, None, graph, &tree.tracked) {
            paths.extend(more);
        }
        paths
    }
}

/// The packages a command names with `-p <package>` or `--package <package>`.
fn named_packages(command: &str) -> Vec<String> {
    let words: Vec<&str> = command.split_whitespace().collect();
    words
        .windows(2)
        .filter(|w| w[0] == "-p" || w[0] == "--package")
        .map(|w| w[1].to_owned())
        .collect()
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_owned()
    } else {
        format!("{dir}/{name}")
    }
}
