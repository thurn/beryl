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

/// The refusal of `words`, which name no check, with why each cannot (`reasons`, empty when the
/// listings alone show it) and how checks are named.
fn no_match(all: &[(&str, &Family)], words: &[&str], reasons: &[String]) -> Reply {
    let names: Vec<&str> = all.iter().map(|(n, _)| *n).collect();
    let why = if reasons.is_empty() {
        String::new()
    } else {
        format!(
            "no listing on this machine holds it, and {}; `<family>:<name>` lists that family \
             afresh\n",
            reasons.join(", ")
        )
    };
    Reply::text(
        Exit::Usage,
        format!(
            "no check matches {}\n{why}checks are named <family>:<check> (families: {}); to run \
             one Rust test, use `cargo nextest run -p <crate> <name>` in the tree",
            words.join(", "),
            names.join(", ")
        ),
    )
}

/// Why `word`, which no listing holds, cannot name a check added since the listings were made,
/// or `None` when it may. Such a check is most often a test the branch has just written, and its
/// name is then in a file of the tree. Two kinds of word are known not to be one:
///
/// - A word whose text before its first `:` is shaped like a family name but names no family,
///   such as `rust-test:cards`. Families are renamed (`b3d-test` became `client-test`),
///   and the old names stay in agents' habits.
/// - A Rust or Python test name, or a `::` path to one, whose last part no file of the tree
///   holds (see [`ignite_git::tracked::mentions`]).
///
/// Any other word, such as a recording's name, may be made from data, so the families are
/// listed to find out. On 2026-11-25 `ignite test rust-test:<crate>` listed every family for
/// 449 s, and `the_plain_card_back_is_brown`, which no file held, for 1040 s, before each said
/// that no check matches.
fn absent(tree: &OpenTree, word: &str) -> Option<String> {
    if let Some((prefix, rest)) = word.split_once(':')
        && !rest.starts_with(':')
    {
        let shaped = prefix.starts_with(|c: char| c.is_ascii_lowercase())
            && prefix
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        return shaped.then(|| format!("no family is named {prefix}"));
    }
    let path = word.strip_suffix("::").unwrap_or(word);
    let identifier = |part: &str| {
        part.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    };
    if !path.split("::").all(identifier) {
        return None;
    }
    let last = path.rsplit("::").next()?;
    match ignite_git::tracked::mentions(&tree.path, last) {
        Ok(false) => Some(format!("no file in the tree holds {last}")),
        Ok(true) | Err(_) => None,
    }
}

/// The families `ignite test` lists, and what it left out.
#[derive(Debug, Default)]
pub(crate) struct TestFamilies<'a> {
    /// The families to list, sorted.
    pub families: Vec<&'a str>,
    /// The packages to list some `nextest` families for (see [`Checks::test_scope`]).
    pub scopes: BTreeMap<String, Vec<String>>,
    /// Families whose listing needs a product build, listed only when some name still matches
    /// no check once `families` are listed. Each is moved to `skipped` otherwise.
    pub deferred: Vec<&'a str>,
    /// The first word no listing held, which made `deferred` necessary.
    pub new_word: Option<String>,
    /// Each family left out because its listing needs a build and every name it matched was
    /// also matched by a family that is cheap to list, with the first such name.
    pub skipped: Vec<(&'a str, String)>,
}

impl TestFamilies<'_> {
    /// Moves every deferred family to `skipped`, once the families listed first matched every
    /// name.
    pub(crate) fn skip_deferred(&mut self) {
        let deferred = std::mem::take(&mut self.deferred);
        let word = self.new_word.clone().unwrap_or_default();
        self.skipped
            .extend(deferred.into_iter().map(|name| (name, word.clone())));
        self.skipped.sort_unstable();
    }

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
/// ([`Checks::keep_names`]). A family never listed on this machine is listed. When no family
/// could hold a bare word, the reply says how checks are named.
///
/// A word that matches no listing may name a check added since, most often a test the branch
/// has just written, so every family without a kept listing for the tree's inputs is listed
/// too. Those of them whose listing needs a product build are `deferred`: the caller lists
/// them only when some word still matches nothing after the cheap families are listed. A new
/// test in one engine crate, `ignite test a_turn_past_small_does_not_narrow`, used to list all
/// 23 families: 1241 s, of which 888 s waited for the 3D client to build and 637 s for the
/// scenario dump, to run two tests that the Rust family's listing of the changed packages
/// found. When every family has a listing here and each word no listing holds cannot name a
/// check added since ([`absent`]), the words are refused before anything is listed.
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
    // Families without a kept listing for the tree's inputs, each with whether it is cheap.
    let mut not_current = Vec::new();
    // Families never listed here whose listing needs a build.
    let mut unknown_costly = Vec::new();
    // Whether some family has never been listed here, so no listing can say what it holds.
    let mut unlisted = false;
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
            not_current.push((name, is_cheap));
            let reading = Stopwatch::start(ctx);
            let latest = checks.latest_listing(name);
            run.slow(
                ctx,
                format!("reading the newest listing of {name}"),
                reading,
            );
            let Some(latest) = latest else {
                unlisted = true;
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
            .filter(|(_, word)| {
                listing
                    .checks
                    .iter()
                    .any(|l| matches(word, l.check.as_str()))
            })
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
    let mut deferred = Vec::new();
    let new_word = found
        .iter()
        .position(|&was| !was)
        .map(|index| bare[index].to_owned());
    if new_word.is_some() && !unlisted {
        let unheld: Vec<&str> = bare
            .iter()
            .zip(&found)
            .filter(|(_, was)| !**was)
            .map(|(word, _)| *word)
            .collect();
        let reading = Stopwatch::start(ctx);
        let reasons: Option<Vec<String>> = unheld.iter().map(|w| absent(tree, w)).collect();
        run.slow(ctx, "searching the tree for the names", reading);
        if let Some(reasons) = reasons {
            return Err(no_match(&all, &unheld, &reasons));
        }
    }
    if new_word.is_some() {
        // A word no listing holds may name a check added since to any family. The families
        // that are cheap to list are listed first; one whose listing needs a build waits for
        // them, unless a word only it matched needs it anyway.
        for (name, is_cheap) in not_current {
            if is_cheap {
                families.push(name);
            } else {
                deferred.push(name);
            }
        }
        deferred.extend(unknown_costly);
        for (name, hits) in costly {
            if hits.iter().all(|&index| found_cheap[index]) {
                deferred.push(name);
            } else {
                families.push(name);
            }
        }
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
    if families.is_empty() && deferred.is_empty() {
        return Err(no_match(&all, &bare, &[]));
    }
    families.sort_unstable();
    families.dedup();
    // A family a word names by its prefix, or one a word needs anyway, is listed first.
    deferred.sort_unstable();
    deferred.dedup();
    deferred.retain(|name| !families.contains(name));
    skipped.sort_unstable();
    Ok(TestFamilies {
        families,
        scopes,
        deferred,
        new_word,
        skipped,
    })
}
