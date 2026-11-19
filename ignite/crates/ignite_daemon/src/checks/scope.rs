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

use std::collections::BTreeSet;
use std::path::Path;

use ignite_core::inputs;
use ignite_core::manifest::{Adapter, Family, InputPattern};

use super::{Checks, Listed, OpenTree, observed, plan, run_reads};
use crate::server::Context;

impl Checks {
    /// Splits `families` into those `changed` can affect, in order, and the names of those it
    /// cannot.
    pub(crate) fn scope<'a>(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        families: &[(&'a str, &'a Family)],
        changed: &[String],
    ) -> (Vec<(&'a str, &'a Family)>, Vec<String>) {
        let kept = self.kept_listings(ctx, tree, families);
        let express_only = ctx
            .config
            .express
            .as_ref()
            .is_some_and(|express| express.covers(changed));
        let mut affected = Vec::new();
        let mut left_out = Vec::new();
        for &(name, family) in families {
            let in_scope = match kept.get(name) {
                // A family without a kept listing cannot be judged from what its checks
                // printed. When the express paths cover the whole change, the family's
                // declared inputs are judged instead, unless it is in the always set, whose
                // checks read repository-wide files its inputs do not name.
                None if family.adapter == Adapter::Nextest || (express_only && !family.always) => {
                    let patterns = self.declared_patterns(ctx, tree, name, family);
                    inputs::touched(&patterns, &tree.graph, changed)
                        .map_or(true, |touched| !touched.is_empty())
                }
                None => true,
                // A check named after a file the change edits always runs in the gate.
                Some((_, listing))
                    if listing
                        .checks
                        .iter()
                        .any(|c| plan::edited(&c.check, changed)) =>
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
                    inputs::touched(&patterns, &tree.graph, changed)
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
