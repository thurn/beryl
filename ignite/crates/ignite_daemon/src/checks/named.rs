//! Which families `ignite test <names>` lists, found from the listings this machine keeps
//! before anything is built (see "ignite test" in `docs/ignite.md`).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use ignite_core::manifest::Family;
use ignite_core::protocol::client::{Exit, Reply};

use super::time_lines::Stopwatch;
use super::verbs::{changed_paths, matches};
use super::{CheckRun, Checks, OpenTree};
use crate::server::Context;

/// The families `ignite test` lists, and what it left out.
#[derive(Debug, Default)]
pub(crate) struct TestFamilies<'a> {
    /// The families to list, sorted.
    pub families: Vec<&'a str>,
    /// The packages to list some `nextest` families for (see [`Checks::test_scope`]).
    pub scopes: BTreeMap<String, Vec<String>>,
    /// Each family left out because its listing needs a build and every name it matched was
    /// also matched by a family that is cheap to list, with the first such name.
    pub skipped: Vec<(&'a str, String)>,
}

impl TestFamilies<'_> {
    /// The `skipped` line, which names the families left out and how to run their checks.
    pub(crate) fn skipped_line(&self) -> Option<(String, String)> {
        let (first, word) = self.skipped.first()?;
        let names: Vec<&str> = self.skipped.iter().map(|(name, _)| *name).collect();
        Some((
            "skipped".to_owned(),
            format!(
                "{} also hold matching checks, but listing them needs a build; \
                 `ignite test {first}:{word}` runs them",
                names.join(", ")
            ),
        ))
    }
}

/// Whether listing `family` for the tree costs no product build of its own: its listing is kept
/// for the tree's inputs, its `list` needs no product, or it is a `nextest` family that can be
/// listed for some of its packages only.
fn cheap(family: &Family, kept: bool) -> bool {
    let needs = family.list_needs.as_ref().unwrap_or(&family.needs);
    kept || needs.is_empty() || Checks::lists_packages(family)
}

/// The families whose checks `words` could name, found without building anything. A word with
/// a known `<family>:` prefix names that family. Any other word is looked up in each family's
/// kept listing for the tree's inputs or, when it has none, in the newest listing this machine
/// keeps of it. A change to a file many families read, such as one card, leaves most of them
/// without a kept listing for the new inputs, and listing them all again builds every product;
/// a check is seldom added between two listings, so the newest one answers. The newest listing
/// may be one that is never kept for its inputs, such as one whose checks carry digests
/// ([`Checks::keep_names`]). A family never listed on this machine is listed, and when a word
/// matches no listing, so is every family without a kept listing for the tree's inputs. When
/// no family could hold a bare word, the reply says how checks are named.
///
/// A family whose listing needs a product build (see [`cheap`]) is left out when every bare
/// word it matches also matches a check of a family that is cheap to list, and so is a family
/// never listed whose listing needs a build, when every bare word matched such a family. After
/// a one-line change to the card Fork, `ignite test fork` matched one Rust test and the
/// recordings of Fork in two interface families of the game's 3D client, whose listing needs
/// that client built; it spent 24 to 55 minutes listing them on a loaded machine before it ran
/// the Rust test. Those families are named on the `skipped` line, and a word with their prefix
/// runs their checks.
///
/// When every bare word matched, a `nextest` family found through its newest listing alone is
/// listed for the packages of its matching checks only, and the packages the branch changes
/// ([`Checks::test_scope`]). Listing a Rust test family builds every test binary it lists.
///
/// Steps of a second or more, such as reading a large family's newest listing, are recorded in
/// `run` for the `slow` line.
#[allow(clippy::too_many_lines)]
pub(crate) fn test_families<'a>(
    checks: &'a Checks,
    ctx: &Context,
    tree: &OpenTree,
    words: &[&str],
    run: &mut CheckRun,
) -> Result<TestFamilies<'a>, Reply> {
    let prefixed = |w: &str| {
        w.split_once(':')
            .and_then(|(f, _)| checks.families().find(|(n, _)| *n == f))
            .map(|(n, _)| n)
    };
    let mut families: Vec<&str> = words.iter().filter_map(|w| prefixed(w)).collect();
    let bare: Vec<&str> = words
        .iter()
        .copied()
        .filter(|w| prefixed(w).is_none())
        .collect();
    if bare.is_empty() {
        return Ok(TestFamilies {
            families,
            ..TestFamilies::default()
        });
    }
    let all: Vec<(&str, &Family)> = checks.families().collect();
    let reading = Stopwatch::start(ctx);
    let kept = checks.kept_listings(ctx, tree, &all);
    run.slow(ctx, "reading kept listings", reading);
    let mut found = vec![false; bare.len()];
    let mut found_cheap = vec![false; bare.len()];
    let mut not_current = Vec::new();
    // Families never listed here whose listing needs a build.
    let mut unknown_costly = Vec::new();
    // Each family whose listing needs a build, with the indexes of the words it matched.
    let mut costly: Vec<(&str, Vec<usize>)> = Vec::new();
    // The packages of the matching checks of each family found through its newest listing.
    let mut matched: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for &(name, family) in &all {
        let is_kept = kept.contains_key(name);
        let is_cheap = cheap(family, is_kept);
        let listing = if let Some((_, listing)) = kept.get(name) {
            Arc::clone(listing)
        } else {
            not_current.push(name);
            let reading = Stopwatch::start(ctx);
            let latest = checks.latest_listing(name);
            run.slow(
                ctx,
                format!("reading the newest listing of {name}"),
                reading,
            );
            let Some(latest) = latest else {
                if is_cheap {
                    families.push(name);
                } else {
                    unknown_costly.push(name);
                }
                continue;
            };
            Arc::new(latest)
        };
        let hits: Vec<usize> = bare
            .iter()
            .enumerate()
            .filter(|(_, word)| listing.checks.iter().any(|l| matches(word, l.check.as_str())))
            .map(|(index, _)| index)
            .collect();
        if hits.is_empty() {
            continue;
        }
        for &index in &hits {
            found[index] = true;
            found_cheap[index] |= is_cheap;
        }
        if !is_kept && Checks::lists_packages(family) {
            let packages = listing
                .checks
                .iter()
                .filter(|l| bare.iter().any(|w| matches(w, l.check.as_str())))
                .filter_map(|l| l.package.clone());
            matched.insert(name.to_owned(), packages.collect());
        }
        if is_cheap {
            families.push(name);
        } else {
            costly.push((name, hits));
        }
    }
    let mut skipped = Vec::new();
    if found.contains(&false) {
        // A word no listing holds may name a check added since to any family.
        families.extend(not_current);
        families.extend(unknown_costly);
        families.extend(costly.iter().map(|(name, _)| *name));
        matched.clear();
    } else {
        if found_cheap.contains(&false) {
            families.extend(unknown_costly);
        }
        for (name, hits) in costly {
            if hits.iter().all(|&index| found_cheap[index]) {
                skipped.push((name, bare[hits[0]].to_owned()));
            } else {
                families.push(name);
            }
        }
    }
    // A family a word names by its prefix is asked for whole, and is not skipped.
    skipped.retain(|(name, _)| !words.iter().any(|w| prefixed(w) == Some(*name)));
    matched.retain(|name, packages| {
        !packages.is_empty() && !words.iter().any(|w| prefixed(w) == Some(name.as_str()))
    });
    let changed = if matched.is_empty() {
        Vec::new()
    } else {
        changed_paths(checks, ctx, tree).unwrap_or_else(|_| {
            matched.clear();
            Vec::new()
        })
    };
    let scopes = matched
        .iter()
        .map(|(name, packages)| (name.clone(), Checks::test_scope(tree, packages, &changed)))
        .collect();
    if families.is_empty() {
        let names: Vec<&str> = all.iter().map(|(n, _)| *n).collect();
        return Err(Reply::text(
            Exit::Usage,
            format!(
                "no check matches {}\nchecks are named <family>:<check> (families: {}); to run \
                 one Rust test, use `cargo nextest run -p <crate> <name>` in the tree",
                bare.join(", "),
                names.join(", ")
            ),
        ));
    }
    families.sort_unstable();
    families.dedup();
    skipped.sort_unstable();
    Ok(TestFamilies {
        families,
        scopes,
        skipped,
    })
}
