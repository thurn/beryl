//! The repository files a `nextest` family's tests open at run time (see "Observed inputs" in
//! `docs/ignite.md`).
//!
//! What a test's build reads is observed from Cargo's dep-info files (see `observed`). A file
//! the test opens while it runs leaves no such trace, so the test opens it through a helper of
//! its workspace, which appends the path to a file in the directory that [`READS_ENV`] names,
//! one line per read: `<binary id>\t<test name>\t<path>`, the path relative to the checkout's
//! root. cargo-nextest runs each test in a process of its own and tells it its binary and its
//! name (`NEXTEST_BINARY_ID`, `NEXTEST_TEST_NAME`), which together name the check. The helper
//! names its file after its process, so tests running at once never write to the same file.
//!
//! After a run, Ignite merges the files into one read list per test that reported, an empty
//! list for a test that opened nothing through the helper. The lists are kept like a listing,
//! locally and at the hub, one entry per package under the family's name, the package and the
//! package's coarse hash: the hash of its `crate-closure`, the family's commands and the
//! toolchain. Code that could change which files a test opens is in the coarse hash, so a list
//! holds for every tree with the same coarse hash, until a file the list names says otherwise.
//!
//! A check's input hash takes the files of its read list, found in two steps: the coarse hash
//! finds the list of the package's last run, and the input hash covers those files with
//! everything else the check reads. A check of a recording family with no list is never looked
//! up in the result cache: it runs once, and its result is stored under the hash that covers
//! the files it was seen to open. A list that changes between runs, as when a data file names
//! other files to open, is replaced by the newest run's list, and that run's result is stored
//! under the hash its new list gives.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::PoisonError;
use std::sync::atomic::Ordering;

use ignite_core::ids::{CheckName, Oid};
use ignite_core::inputs;
use ignite_core::manifest::{Adapter, Family, InputPattern};
use ignite_core::protocol::Outcome;
use ignite_core::protocol::hub::ListingKey;

use super::{Checks, Final, Keyed, Listed, OpenTree, Reported, observed};
use crate::server::Context;

/// The variable that names the directory a run's tests record their reads in. The helpers
/// record nothing when it is unset.
pub use ignite_core::test_files::READS_ENV;

/// One package's read lists at one coarse hash: each check's paths, sorted, by check name.
pub(crate) type Lists = BTreeMap<String, Vec<String>>;

/// New read lists to keep: each check's name and paths, by the key of its package's lists.
type NewLists = HashMap<ListingKey, Vec<(String, Vec<String>)>>;

/// A final outcome to key again: its index, its inputs other than run-time reads, the paths its
/// new list adds and the list itself.
type Rekeying = (usize, Vec<(String, Oid)>, Vec<String>, Vec<String>);

/// Each package's list key, and its kept lists if there are any, by package name.
pub(crate) type PackageLists = HashMap<String, (ListingKey, Option<Lists>)>;

/// What a check's input hash knows of the files its test opens at run time.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum RunReads {
    /// The family does not record run-time reads; the hash covers its declared inputs.
    #[default]
    NotRecorded,
    /// No list is kept for the check, so its hash cannot be trusted and it is not looked up.
    /// `key` is where its package's lists are kept, when that is known.
    Unknown { key: Option<ListingKey> },
    /// The check's list is kept under `key`; `added` holds the paths it adds to the hash, the
    /// tracked ones not already among the check's other inputs.
    Known { key: ListingKey, added: Vec<String> },
}

impl RunReads {
    /// Where the check's package's lists are kept.
    pub fn key(&self) -> Option<&ListingKey> {
        match self {
            RunReads::NotRecorded => None,
            RunReads::Unknown { key } => key.as_ref(),
            RunReads::Known { key, .. } => Some(key),
        }
    }

    /// Whether the check's hash covers everything it reads, so a cached result may answer it.
    pub fn may_hit(&self) -> bool {
        !matches!(self, RunReads::Unknown { .. })
    }
}

/// Whether a family's tests record what they open at run time: every `nextest` family.
pub(crate) fn records(family: &Family) -> bool {
    family.adapter == Adapter::Nextest
}

/// The key a package's lists are kept under. A family's listings are kept apart from its read
/// lists, and the listing store keeps a few entries per name, so each package has a name.
fn list_key(family: &str, package: &str, coarse: ignite_core::ids::InputHash) -> ListingKey {
    ListingKey {
        family: format!("{family}#reads:{package}"),
        hash: coarse,
    }
}

/// What a check's hash knows of its run-time reads, from its package's lists. `base` holds the
/// check's other input paths.
pub(crate) fn for_check(
    lists: &PackageLists,
    listed: &Listed,
    base: &[String],
    tracked: &[String],
) -> RunReads {
    let Some((key, found)) = listed.package.as_ref().and_then(|p| lists.get(p)) else {
        return RunReads::Unknown { key: None };
    };
    match found.as_ref().and_then(|l| l.get(listed.check.as_str())) {
        None => RunReads::Unknown {
            key: Some(key.clone()),
        },
        Some(paths) => RunReads::Known {
            key: key.clone(),
            added: added(paths, base.iter().map(String::as_str), tracked),
        },
    }
}

/// The paths of `read` that are tracked (a file, or a directory holding one) and not in
/// `base`. A file the test read that is gone names no path, which changes the hash.
fn added<'a>(
    read: &[String],
    base: impl Iterator<Item = &'a str>,
    tracked: &[String],
) -> Vec<String> {
    let known = observed::known_paths(tracked);
    let base: HashSet<&str> = base.collect();
    read.iter()
        .filter(|p| known.contains(p.as_str()) && !base.contains(p.as_str()))
        .cloned()
        .collect()
}

/// The paths the files in `dir` record for each test of `family`, by check name.
pub(crate) fn collect(dir: &Path, family: &str) -> HashMap<CheckName, BTreeSet<String>> {
    let mut found: HashMap<CheckName, BTreeSet<String>> = HashMap::new();
    let files = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok);
    for file in files {
        let Ok(recorded) = fs::read_to_string(file.path()) else {
            continue;
        };
        for line in recorded.lines() {
            let mut fields = line.splitn(3, '\t');
            let (Some(binary), Some(test), Some(path)) =
                (fields.next(), fields.next(), fields.next())
            else {
                continue;
            };
            let Ok(check) = CheckName::new(format!("{family}:{binary}::{test}")) else {
                continue;
            };
            found
                .entry(check)
                .or_default()
                .insert(path.replace('\\', "/"));
        }
    }
    found
}

/// Whether a run reported the check's test as having run to its end.
fn ran(reported: &[Reported], check: &CheckName) -> bool {
    reported.iter().any(|r| {
        r.check == *check && matches!(r.outcome, Outcome::Pass | Outcome::Fail | Outcome::Flaky)
    })
}

impl Checks {
    /// Each package's list key and kept lists, from one reading of the tree and one request.
    /// When the tree cannot be read, no package has one, and no check is looked up.
    pub(crate) fn read_lists(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        (name, family): (&str, &Family),
        packages: &BTreeSet<String>,
    ) -> PackageLists {
        let closure = [InputPattern::CrateClosure];
        let mut closures: Vec<(&String, Vec<String>)> = Vec::new();
        for package in packages {
            // A package the crate graph does not hold has no code of its own to hash; its
            // lists are kept under the family's commands and toolchain alone.
            let paths = inputs::resolve(&closure, Some(package), &tree.graph, &tree.tracked)
                .unwrap_or_default();
            closures.push((package, paths));
        }
        let every: BTreeSet<&String> = closures.iter().flat_map(|(_, p)| p).collect();
        let every: Vec<String> = every.into_iter().cloned().collect();
        let Ok(ids) = inputs::object_ids(ctx.git.as_ref(), &tree.path, &every) else {
            return PackageLists::new();
        };
        let ids: HashMap<String, Oid> = ids.into_iter().collect();
        let keys: Vec<(String, ListingKey)> = closures
            .into_iter()
            .map(|(package, paths)| {
                let own: Vec<(String, Oid)> = paths
                    .into_iter()
                    .filter_map(|p| ids.get(&p).cloned().map(|oid| (p, oid)))
                    .collect();
                let coarse = self.listing_hash(family, &own);
                (package.clone(), list_key(name, package, coarse))
            })
            .collect();
        let asked: Vec<ListingKey> = keys.iter().map(|(_, k)| k.clone()).collect();
        let answers = self.cache.listings(&asked);
        keys.into_iter()
            .zip(answers)
            .map(|((package, key), answer)| {
                let lists = answer.and_then(|text| serde_json::from_str::<Lists>(&text).ok());
                (package, (key, lists))
            })
            .collect()
    }

    /// A new directory for one run's tests to record their reads in, or `None` when it cannot
    /// be made, in which case nothing is recorded and the run's checks keep the hashes they had.
    pub(crate) fn reads_dir(&self) -> Option<PathBuf> {
        let dir = self.settings.outputs.join(format!(
            "reads-{}-{}",
            std::process::id(),
            self.next_file.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).ok().map(|()| dir)
    }

    /// Reads what the tests of one run recorded in `dir`, then deletes it. Each check of
    /// `checks` whose test ran to its end gets the list its run recorded, empty if it opened
    /// nothing through the helper, for [`Checks::rekey`] to take.
    pub(crate) fn note_run_reads(
        &self,
        dir: &Path,
        name: &str,
        checks: &[Keyed],
        reported: &[Reported],
    ) {
        let mut found = collect(dir, name);
        let _ = fs::remove_dir_all(dir);
        let mut fresh = self
            .fresh_reads
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        for keyed in checks {
            let check = &keyed.listed.check;
            if keyed.reads.key().is_some() && ran(reported, check) {
                let paths = found.remove(check).unwrap_or_default();
                fresh.insert(check.clone(), paths.into_iter().collect());
            }
        }
    }

    /// Gives each final outcome whose run recorded a read list the key that list gives, and
    /// keeps the lists. Returns the checks whose results must not be stored: those that had
    /// no list and got none, whose hash does not cover what they read.
    pub(crate) fn rekey(
        &self,
        ctx: &Context,
        tree: &OpenTree,
        family: &Family,
        finals: &mut [Final],
    ) -> HashSet<CheckName> {
        let fresh = std::mem::take(
            &mut *self
                .fresh_reads
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        );
        let mut unkeyed: HashSet<CheckName> = finals
            .iter()
            .filter(|f| !f.0.reads.may_hit())
            .map(|f| f.0.listed.check.clone())
            .collect();
        let mut plans: Vec<Rekeying> = Vec::new();
        for (at, (keyed, outcome, ..)) in finals.iter().enumerate() {
            let check = &keyed.listed.check;
            let Some(read) = fresh.get(check) else {
                continue;
            };
            if !matches!(outcome, Outcome::Pass | Outcome::Fail | Outcome::Flaky) {
                continue;
            }
            let old: &[String] = match &keyed.reads {
                RunReads::Known { added, .. } => added,
                _ => &[],
            };
            let base: Vec<(String, Oid)> = keyed
                .inputs
                .iter()
                .filter(|(path, _)| !old.contains(path))
                .cloned()
                .collect();
            let added = added(read, base.iter().map(|(p, _)| p.as_str()), &tree.tracked);
            plans.push((at, base, added, read.clone()));
        }
        if plans.is_empty() {
            return unkeyed;
        }
        let wanted: BTreeSet<&String> = plans.iter().flat_map(|(_, _, added, _)| added).collect();
        let wanted: Vec<String> = wanted.into_iter().cloned().collect();
        let Ok(ids) = inputs::object_ids(ctx.git.as_ref(), &tree.path, &wanted) else {
            return unkeyed;
        };
        let ids: HashMap<String, Oid> = ids.into_iter().collect();
        let mut lists = NewLists::new();
        for (at, mut inputs, added, read) in plans {
            let keyed = &mut finals[at].0;
            let Some(key) = keyed.reads.key().cloned() else {
                continue;
            };
            inputs.extend(
                added
                    .iter()
                    .filter_map(|p| ids.get(p).map(|oid| (p.clone(), oid.clone()))),
            );
            inputs.sort();
            let check = keyed.listed.check.clone();
            *keyed = self.keyed(
                family,
                keyed.listed.clone(),
                inputs,
                RunReads::Known {
                    key: key.clone(),
                    added,
                },
            );
            unkeyed.remove(&check);
            lists
                .entry(key)
                .or_default()
                .push((check.as_str().to_owned(), read));
        }
        self.keep_lists(lists);
        unkeyed
    }

    /// Merges new read lists into each package's kept lists, and keeps those that changed.
    /// Two machines that run different tests of one package at once may each keep only their
    /// own; a test whose list is lost runs once more.
    fn keep_lists(&self, new: NewLists) {
        let new: Vec<_> = new.into_iter().collect();
        let keys: Vec<ListingKey> = new
            .iter()
            .map(|(k, _): &(ListingKey, _)| k.clone())
            .collect();
        let kept = self.cache.listings(&keys);
        for ((key, lists), kept) in new.into_iter().zip(kept) {
            let mut merged: Lists = kept
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or_default();
            let before = merged.clone();
            merged.extend(lists);
            if merged != before
                && let Ok(text) = serde_json::to_string(&merged)
            {
                self.cache.store_listing(&key, &text);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_tests_lines_from_every_file_make_its_list() {
        let dir = ignite_core::test_files::temp_dir()
            .join(format!("ignite-run-reads-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("11-a"),
            "core\ta\tdata/x.json\ncore\ta\tdata/y.json\nnot a line\n",
        )
        .unwrap();
        fs::write(
            dir.join("12-b"),
            "core::cli\tb\tdocs\\decks\ncore\ta\tdata/x.json\n",
        )
        .unwrap();
        let found = collect(&dir, "rust");
        let paths = |check: &str| -> Vec<String> {
            found[&CheckName::new(check).unwrap()]
                .iter()
                .cloned()
                .collect()
        };
        assert_eq!(found.len(), 2);
        assert_eq!(paths("rust:core::a"), ["data/x.json", "data/y.json"]);
        assert_eq!(paths("rust:core::cli::b"), ["docs/decks"]);
        let _ = fs::remove_dir_all(&dir);
    }
}
