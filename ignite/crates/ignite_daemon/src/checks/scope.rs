//! Which families a change can affect, decided before anything is listed or built (see
//! "Landing" in `docs/ignite.md`).
//!
//! A family is out of scope when no changed path matches anything its checks can read: its
//! `inputs`, `package_inputs` and `digest_inputs`, the inputs of the products it needs, the
//! lock file, manifest and toolchain file of every Cargo workspace those name, the manifest
//! file that declares the family, and every input of their own that its checks printed in its
//! kept listing (with the directory of each such file, where a new check would appear). A
//! family one of whose checks is named after a file the change edits stays in scope too. A
//! `jsonl` family with no kept listing for the tree's inputs cannot be judged, since its checks
//! may print inputs of their own, and stays in scope to be listed; a family whose checks carry digests has a kept listing only when it
//! declares `digest_inputs`. A family in the always set is treated the same way: it blocks
//! every landing its inputs touch, so it too is left out when the change touches none of them.
//!
//! A `nextest` family with no kept listing is judged by its declared inputs too. Its checks
//! print no inputs of their own: each test reads its package's closure, the family's `inputs`
//! and its package's `package_inputs`, all declared in the manifest, so the declared inputs
//! cover every check the family could list. On 2026-11-19 a change to one card file listed
//! Ignite's own test family (62 s waiting for Cargo's lock) and built its tests, because Ignite
//! changes land every few minutes and the family's kept listing rarely matched the tree.
//!
//! A change that the express paths (`ignite.toml`) cover entirely also judges any other family
//! with no kept listing by its declared inputs, so a fresh tree lists no unrelated family for a
//! change confined to Ignite's own files. Families in the always set are still listed.
//!
//! When a family is judged by its declared inputs, `crate-closure` stands for the packages of the
//! workspaces the family reaches ([`Checks::closure_packages`]), as in its listing hash.
//!
//! A `nextest` family's kept listing also names what its builds were observed to read (see
//! `observed`), and the kept read lists of its packages name what its tests opened at run time
//! (see `run_reads`). Both count like inputs the family declares.
//!
//! In a family with a kept listing, `crate-closure` stands for the packages its checks belong
//! to, with the packages each depends on, not for every package of every workspace. A change
//! to Ignite's own crates once kept the game's Rust test families in scope, because their
//! `crate-closure` reached every package the checkout holds.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;

use ignite_core::ids::CheckName;
use ignite_core::inputs::{self, CrateGraph};
use ignite_core::manifest::{Adapter, Family, InputPattern};
use ignite_git::changed::WorktreeChanges;

use super::listings::Kept;
use super::{Checks, Listed, OpenTree, observed, plan, run_reads};
use crate::server::Context;

/// Deletion evidence from before listing. Listing again can discard the only record of
/// a deleted path, and a family's listing might itself need a cold build.
#[derive(Default)]
pub(super) struct GateChanges<'a> {
    pub paths: &'a [String],
    pub deleted: &'a [String],
    pub deleted_input_families: BTreeSet<String>,
    pub deleted_input_checks: BTreeSet<CheckName>,
}

impl<'a> From<&'a WorktreeChanges> for GateChanges<'a> {
    fn from(changes: &'a WorktreeChanges) -> Self {
        Self {
            paths: &changes.paths,
            deleted: &changes.deleted,
            ..Self::default()
        }
    }
}

impl GateChanges<'_> {
    /// The Git index can still name a file the worktree has deleted but not staged.
    pub(super) fn remove_deleted_paths(&self, tree: &mut OpenTree) {
        if !self.deleted.is_empty() {
            let deleted: HashSet<_> = self.deleted.iter().collect();
            tree.tracked.retain(|path| !deleted.contains(path));
        }
    }
}

impl Checks {
    /// Splits `families` into those `changed` can affect, in order, and the names of those it
    /// cannot.
    /// A recent product catch brings its family into scope even if the declared inputs
    /// of the family are unaffected.
    pub(super) fn scope<'a>(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        families: &[(&'a str, &'a Family)],
        changed: &[String],
        recent: &BTreeSet<CheckName>,
    ) -> (Vec<(&'a str, &'a Family)>, Vec<String>) {
        self.scope_with_deletions(
            ctx,
            tree,
            families,
            recent,
            &mut GateChanges {
                paths: changed,
                ..GateChanges::default()
            },
        )
    }

    pub(super) fn scope_with_deletions<'a>(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        families: &[(&'a str, &'a Family)],
        recent: &BTreeSet<CheckName>,
        changes: &mut GateChanges<'_>,
    ) -> (Vec<(&'a str, &'a Family)>, Vec<String>) {
        let paths = changes.paths;
        let kept = self.kept_listings(ctx, tree, families);
        let required: BTreeSet<_> = recent.iter().map(CheckName::family).collect();
        let express_only = ctx
            .config
            .express
            .as_ref()
            .is_some_and(|express| express.covers(paths));
        let mut affected = Vec::new();
        let mut left_out = Vec::new();
        for &(name, family) in families {
            let listing = kept.get(name);
            if !changes.deleted.is_empty() {
                let latest = listing
                    .is_none()
                    .then(|| self.latest_listing(name))
                    .flatten();
                let previous = listing.map(|(_, kept)| kept.as_ref()).or(latest.as_ref());
                self.note_deleted_inputs(ctx, tree, (name, family), previous, changes);
            }
            let in_scope = match listing {
                _ if required.contains(name) || changes.deleted_input_families.contains(name) => {
                    true
                }
                // A family without a kept listing cannot be judged from what its checks
                // printed. When the express paths cover the whole change, the family's
                // declared inputs are judged instead, unless it is in the always set, whose
                // checks read repository-wide files its inputs do not name.
                None if family.adapter == Adapter::Nextest || (express_only && !family.always) => {
                    let patterns = self.declared_patterns(ctx, tree, name, family);
                    inputs::touched(&patterns, &tree.graph, paths)
                        .map_or(true, |touched| !touched.is_empty())
                }
                None => true,
                // A check named after a file the change edits always runs in the gate.
                Some((_, listing))
                    if listing.checks.iter().any(|c| plan::edited(&c.check, paths)) =>
                {
                    true
                }
                Some((_, listing)) => {
                    let mut patterns = self.listing_patterns(family);
                    patterns.extend(Self::source_patterns(tree, &listing.checks));
                    // What the family's builds were observed to read, which its declared
                    // inputs no longer name.
                    patterns.extend(
                        listing
                            .reads
                            .iter()
                            .flat_map(observed::every_path)
                            .map(InputPattern::Path),
                    );
                    // What the family's tests were seen to open at run time.
                    patterns.extend(self.run_read_paths(
                        ctx,
                        tree,
                        (name, family),
                        &listing.checks,
                    ));
                    let mut patterns = listed_packages(patterns, &listing.checks);
                    patterns.extend(self.manifest_file(ctx, name).map(InputPattern::Path));
                    // A pattern that cannot be matched, such as a crate that is gone, says
                    // nothing about the change, so the family is listed to find out.
                    inputs::touched(&patterns, &tree.graph, paths)
                        .map_or(true, |touched| !touched.is_empty())
                }
            };
            if in_scope {
                affected.push((name, family));
            } else {
                left_out.push(name.to_owned());
            }
        }
        (affected, left_out)
    }

    /// Collects matches before listing can drop old entries or defer a cold family.
    fn note_deleted_inputs(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        (name, family): (&str, &Family),
        listing: Option<&Kept>,
        changes: &mut GateChanges<'_>,
    ) {
        let mut checks = BTreeSet::new();
        let patterns = self.declared_patterns(ctx, tree, name, family);
        if let Some(listing) = listing {
            checks.extend(self.checks_with_deleted_inputs(
                tree,
                family,
                listing.checks.iter(),
                changes.deleted,
            ));
            if let Some(reads) = &listing.reads {
                let mut packages = HashMap::new();
                for listed in &listing.checks {
                    let Some(package) = &listed.package else {
                        continue;
                    };
                    if *packages.entry(package).or_insert_with(|| {
                        let closure = inputs::closure(package, &tree.graph)
                            .unwrap_or_else(|_| vec![package.clone()]);
                        let paths = observed::paths_for(reads, &closure);
                        reads_deleted(&paths, changes.deleted)
                    }) {
                        checks.insert(listed.check.clone());
                    }
                }
            }
        }
        if run_reads::records(family) {
            // The listing hash can change while its packages' read lists still hold.
            let packages = listing.map_or_else(
                || {
                    self.closure_packages(family, &patterns, &tree.graph)
                        .into_iter()
                        .collect()
                },
                |listing| {
                    listing
                        .checks
                        .iter()
                        .filter_map(|c| c.package.clone())
                        .collect()
                },
            );
            let lists = self.read_lists(ctx, tree, (name, family), &packages);
            for (key, lists) in lists.into_values() {
                let lists = lists.or_else(|| {
                    self.cache
                        .latest_listing(&key.family)
                        .and_then(|text| serde_json::from_str::<run_reads::Lists>(&text).ok())
                });
                for (check, paths) in lists.into_iter().flatten() {
                    if reads_deleted(&paths, changes.deleted)
                        && let Ok(check) = CheckName::new(check)
                    {
                        checks.insert(check);
                    }
                }
            }
        }
        if !checks.is_empty() || matches_deleted(&patterns, &tree.graph, changes.deleted) {
            changes.deleted_input_families.insert(name.to_owned());
            changes.deleted_input_checks.extend(checks);
        }
    }

    /// Checks whose declared inputs match deleted paths. Match the old path directly so a
    /// glob can name a file even after it has disappeared from the tracked files.
    pub(super) fn checks_with_deleted_inputs<'a>(
        &self,
        tree: &OpenTree,
        family: &Family,
        checks: impl Iterator<Item = &'a Listed>,
        deleted: &[String],
    ) -> BTreeSet<CheckName> {
        if deleted.is_empty() {
            return BTreeSet::new();
        }
        let observes = family.adapter == Adapter::Nextest && self.target_dir(family).is_some();
        let mut common = self.check_patterns(family, observes);
        common.extend(family.digest_inputs.iter().cloned());
        let mut packages = HashMap::new();
        let mut found = BTreeSet::new();
        for listed in checks {
            let package = listed.package.as_deref();
            let package_match = *packages.entry(package).or_insert_with(|| {
                let patterns = common
                    .iter()
                    .chain(family.package_patterns(package))
                    .cloned();
                matches_check_patterns(patterns, package, &tree.graph, deleted)
            });
            let own = listed.inputs.iter().cloned().map(InputPattern::from);
            if package_match || matches_check_patterns(own, package, &tree.graph, deleted) {
                found.insert(listed.check.clone());
            }
        }
        found
    }

    /// Everything a family's checks can read by its manifest alone: its listing patterns, the
    /// inputs of every product it needs and the manifest file that declares it, with
    /// `crate-closure` standing for the packages of the workspaces the family reaches.
    fn declared_patterns(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        name: &str,
        family: &Family,
    ) -> Vec<InputPattern> {
        let manifest = &self.settings.manifest;
        let mut patterns = self.listing_patterns(family);
        for product in family.needs.iter().filter_map(|n| manifest.products.get(n)) {
            patterns.extend(product.inputs.iter().cloned());
        }
        let closure = self.closure_packages(family, &patterns, &tree.graph);
        let mut patterns: Vec<InputPattern> = patterns
            .into_iter()
            .flat_map(|pattern| match pattern {
                InputPattern::CrateClosure => closure
                    .iter()
                    .map(|p| InputPattern::Crate(p.clone()))
                    .collect(),
                other => vec![other],
            })
            .collect();
        patterns.extend(self.manifest_file(ctx, name).map(InputPattern::Path));
        patterns
    }

    /// Every path the kept read lists of a recording family's packages name (see
    /// `run_reads`), as patterns; none for a family that does not record them.
    fn run_read_paths(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        (name, family): (&str, &Family),
        checks: &[Listed],
    ) -> Vec<InputPattern> {
        if !run_reads::records(family) {
            return Vec::new();
        }
        let packages: BTreeSet<String> = checks.iter().filter_map(|c| c.package.clone()).collect();
        let lists = self.read_lists(ctx, tree, (name, family), &packages);
        let paths: BTreeSet<&String> = lists
            .values()
            .filter_map(|(_, lists)| lists.as_ref())
            .flat_map(|lists| lists.values().flatten())
            .collect();
        paths.into_iter().cloned().map(InputPattern::Path).collect()
    }

    /// The manifest file that declares the family, relative to the checkout's root.
    fn manifest_file(&self, ctx: &Context, name: &str) -> Option<String> {
        let declared = self.settings.manifest.families.get(name)?;
        let file =
            Path::new(&ctx.config.repository.checks).join(format!("{}.toml", declared.group));
        Some(file.to_str()?.replace('\\', "/"))
    }
}

fn reads_deleted(paths: &[String], deleted: &[String]) -> bool {
    paths
        .iter()
        .any(|path| deleted.iter().any(|file| inputs::path_matches(path, file)))
}

fn matches_deleted(patterns: &[InputPattern], graph: &CrateGraph, deleted: &[String]) -> bool {
    // An unknown crate must not hide a match to another pattern.
    patterns.iter().any(|pattern| {
        inputs::touched(std::slice::from_ref(pattern), graph, deleted)
            .is_ok_and(|matched| !matched.is_empty())
    })
}

fn matches_check_patterns(
    patterns: impl Iterator<Item = InputPattern>,
    package: Option<&str>,
    graph: &CrateGraph,
    deleted: &[String],
) -> bool {
    patterns.into_iter().any(|pattern| {
        let pattern = match (pattern, package) {
            (InputPattern::CrateClosure, Some(package)) => InputPattern::Crate(package.to_owned()),
            (pattern, _) => pattern,
        };
        matches_deleted(&[pattern], graph, deleted)
    })
}

/// `patterns` with `crate-closure` standing for the packages `checks` belong to. A check's
/// `crate-closure` reads its own package and the packages that package depends on, which
/// [`inputs::touched`] follows from a `crate:` pattern. Where a check belongs to no package,
/// nothing narrower can be said, and `crate-closure` is kept.
fn listed_packages(patterns: Vec<InputPattern>, checks: &[Listed]) -> Vec<InputPattern> {
    if !patterns.contains(&InputPattern::CrateClosure) {
        return patterns;
    }
    let Some(packages) = checks
        .iter()
        .map(|c| c.package.clone())
        .collect::<Option<std::collections::BTreeSet<String>>>()
    else {
        return patterns;
    };
    let mut narrowed: Vec<InputPattern> = patterns
        .into_iter()
        .filter(|p| *p != InputPattern::CrateClosure)
        .collect();
    narrowed.extend(packages.into_iter().map(InputPattern::Crate));
    narrowed
}
