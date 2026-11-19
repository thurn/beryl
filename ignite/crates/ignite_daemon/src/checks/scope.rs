//! Which families a change can affect, decided before anything is listed or built (see
//! "Landing" in `docs/ignite.md`).
//!
//! A family is out of scope when no changed path matches anything its checks can read: its
//! `inputs`, `package_inputs` and `digest_inputs`, the inputs of the products it needs, the
//! lock file, manifest and toolchain file of every Cargo workspace those name, the manifest
//! file that declares the family, and every input of their own that its checks printed in its
//! kept listing (with the directory of each such file, where a new check would appear). A
//! family one of whose checks is named after a file the change edits stays in scope too. A
//! family with no kept listing for the tree's inputs cannot be judged, and stays in scope to be
//! listed as before; a family whose checks carry digests has a kept listing only when it
//! declares `digest_inputs`. A family in the always set is treated the same way: it blocks
//! every landing its inputs touch, so it too is left out when the change touches none of them.
//!
//! A change that the express paths (`ignite.toml`) cover entirely also judges a family with no
//! kept listing by its declared inputs, so a fresh tree lists no unrelated family for a change
//! confined to Ignite's own files. Families in the always set are still listed.
//!
//! A `nextest` family's kept listing also names what its builds were observed to read (see
//! `observed`), which counts like an input the family declares.
//!
//! In a family with a kept listing, `crate-closure` stands for the packages its checks belong
//! to, with the packages each depends on, not for every package of every workspace. A change
//! to Ignite's own crates once kept the game's Rust test families in scope, because their
//! `crate-closure` reached every package the checkout holds.

use std::path::Path;

use ignite_core::inputs;
use ignite_core::manifest::{Family, InputPattern};

use super::{Checks, Listed, OpenTree, observed, plan};
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
                None if express_only && !family.always => {
                    let mut patterns = self.listing_patterns(family);
                    patterns.extend(self.manifest_file(ctx, name).map(InputPattern::Path));
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
