//! Cargo's package arguments for a build, and the comparison of a subset's features with the
//! whole workspace's (see the parent module's documentation).

use std::collections::{BTreeMap, BTreeSet};

use ignite_core::ids::InputHash;

use super::PACKAGES;

/// What a whole-workspace feature tree depends on: the workspace's directory, the state of its
/// Cargo files and the packages the product leaves out of it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct WholeKey {
    pub(super) dir: String,
    pub(super) cargo_state: InputHash,
    pub(super) exclude: Vec<String>,
}

/// Cargo's arguments selecting `packages`, or the whole workspace less `exclude` when none are
/// named.
pub(super) fn package_args(packages: &[String], exclude: &[String]) -> Vec<String> {
    if packages.is_empty() {
        let mut args = vec!["--workspace".to_owned()];
        args.extend(
            exclude
                .iter()
                .flat_map(|p| ["--exclude".to_owned(), p.clone()]),
        );
        return args;
    }
    packages
        .iter()
        .flat_map(|p| ["-p".to_owned(), p.clone()])
        .collect()
}

/// The build command split at whitespace, with `{packages}` replaced by Cargo's package
/// arguments. The command runs directly, not through a shell.
pub(super) fn command(build: &str, packages: &[String], exclude: &[String]) -> Vec<String> {
    build
        .split_whitespace()
        .flat_map(|word| {
            if word == PACKAGES {
                package_args(packages, exclude)
            } else {
                vec![word.to_owned()]
            }
        })
        .collect()
}

/// Each package in `cargo tree -e features --format '{p}|{f}'` output with the feature sets it
/// is built with. A package can appear with more than one set, for example once as a build
/// dependency and once as a normal one. Lines without `|` are feature nodes and are skipped.
fn feature_sets(text: &str) -> BTreeMap<&str, BTreeSet<&str>> {
    let mut sets: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for line in text.lines() {
        let Some((package, features)) = line.split_once('|') else {
            continue;
        };
        let features = features.trim_end().trim_end_matches("(*)").trim_end();
        sets.entry(package.trim()).or_default().insert(features);
    }
    sets
}

/// The names of the crates whose features in the subset's tree differ from the whole
/// workspace's, sorted.
pub(super) fn feature_differences(whole: &str, subset: &str) -> Vec<String> {
    let whole = feature_sets(whole);
    let mut differing: BTreeSet<String> = BTreeSet::new();
    for (package, sets) in feature_sets(subset) {
        if whole.get(package) != Some(&sets) {
            let name = package.split_whitespace().next().unwrap_or(package);
            differing.insert(name.to_owned());
        }
    }
    differing.into_iter().collect()
}
