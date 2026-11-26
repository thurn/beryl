//! The checked-in set of recordings used by default in the interface tests.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use game_probe::signature::Measure;
use serde_json::Value;

use crate::pages::Windows;
use crate::{pack, recordings};

/// The first recording of a signature and the number of recording names that share it.
pub(crate) struct Representative<'a> {
    pub name: &'a str,
    pub count: usize,
}

pub(crate) const PATH: &str = "data/client/corpus.txt";

fn path() -> PathBuf {
    recordings::root().join(PATH)
}

/// Reads the names of the default corpus from a checkout.
///
/// # Errors
///
/// When the list cannot be read.
pub fn names(root: &Path) -> Result<BTreeSet<String>, String> {
    let path = root.join(PATH);
    let text = fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(text.lines().map(str::to_owned).collect())
}

/// The sorted recording names in the checked-in corpus, checked against the pack.
pub(crate) fn read() -> Result<BTreeSet<String>, String> {
    let path = path();
    let available = recordings::entries()?;
    let mut names = names(&recordings::root())?;
    names.extend(fixtures()?);
    for name in &names {
        if available
            .binary_search_by(|entry| entry.name.cmp(name.as_str()))
            .is_err()
        {
            let hint = match name.split_once('/') {
                Some((card, _)) => format!("just client corpus --update {card}/"),
                None => "just client corpus --update".to_owned(),
            };
            return Err(format!(
                "{} names unknown recording {name:?}; run `{hint}`",
                path.display()
            ));
        }
    }
    Ok(names)
}

pub(crate) fn fixtures() -> Result<BTreeSet<String>, String> {
    let path = recordings::root().join(recordings::FIXTURE_DEFINITIONS);
    let text = fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let value: Value =
        serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?;
    value["fixtures"]
        .as_array()
        .ok_or_else(|| format!("{} has no fixtures array", path.display()))?
        .iter()
        .map(|fixture| {
            fixture["name"]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("{} has a fixture without a name", path.display()))
        })
        .collect()
}

/// The exact signature of a recording, with each seat's shape in seat order.
pub(crate) type Shape = Vec<(u8, Vec<u8>)>;

/// Measures each recording in the selected pack. Retaining failures by name lets a sweep
/// report other missing signatures even when one recording cannot be decoded.
type Measured = (&'static str, Result<Shape, String>);

/// Whether `name` belongs to the scope named by `prefix`. A prefix with `/` matches by string
/// prefix (`island/`, `dandn/is-cast`); a bare prefix matches the exact name or `<prefix>/...`,
/// so `island` does not match `island-sanctuary/...`.
pub(crate) fn matches_prefix(name: &str, prefix: &str) -> bool {
    if prefix.contains('/') {
        name.starts_with(prefix)
    } else {
        name == prefix || name.starts_with(&format!("{prefix}/"))
    }
}

pub(crate) fn shapes() -> Result<Vec<Measured>, String> {
    shapes_for(&[])
}

fn shapes_for(prefixes: &[String]) -> Result<Vec<Measured>, String> {
    let windows = Windows::All.windows();
    let mut measure = Measure::new(crate::data::checkout_ref()?, &windows);
    Ok(recordings::entries()?
        .chunk_by(|a, b| a.name == b.name)
        .filter(|entries| {
            prefixes.is_empty()
                || prefixes
                    .iter()
                    .any(|prefix| matches_prefix(entries[0].name, prefix))
        })
        .map(|entries| {
            let shape = entries.iter().map(|entry| {
                pack::messages(entry)
                    .and_then(|messages| measure.recording(&messages))
                    .map(|signature| (entry.seat, signature.into_bytes()))
                    .map_err(|error| format!("{} seat {}: {error}", entry.name, entry.seat))
            });
            (entries[0].name, shape.collect())
        })
        .collect())
}

/// Groups matching recordings in the selected pack by signature. When `prefixes` is empty, every
/// recording is included. The pack is sorted by name and seat, so insertion chooses the first
/// name in each group.
pub(crate) fn representatives_for(
    prefixes: &[String],
) -> Result<Vec<Representative<'static>>, String> {
    let mut groups = HashMap::<Shape, Representative<'_>>::new();
    for (name, shape) in shapes_for(prefixes)? {
        let representative = groups
            .entry(shape?)
            .or_insert(Representative { name, count: 0 });
        representative.count += 1;
    }
    let mut selected: Vec<_> = groups.into_values().collect();
    selected.sort_unstable_by_key(|group| group.name);
    Ok(selected)
}

/// Merges `groups` and matching `fixtures` into `existing` for `prefixes`, leaving all unrelated
/// entries untouched. When `prefixes` is empty, replaces the whole set with `fixtures` and
/// `groups`.
pub(crate) fn merge_for(
    existing: &BTreeSet<String>,
    fixtures: &BTreeSet<String>,
    prefixes: &[String],
    groups: &[&str],
) -> BTreeSet<String> {
    if prefixes.is_empty() {
        let mut out = fixtures.clone();
        out.extend(groups.iter().map(|name| (*name).to_owned()));
        return out;
    }
    let in_scope = |name: &str| prefixes.iter().any(|prefix| matches_prefix(name, prefix));
    let mut out: BTreeSet<String> = existing
        .iter()
        .filter(|name| !in_scope(name))
        .cloned()
        .collect();
    out.extend(fixtures.iter().filter(|name| in_scope(name)).cloned());
    out.extend(groups.iter().map(|name| (*name).to_owned()));
    out
}

/// Writes the representatives and fixtures in name order, scoped to `prefixes` when non-empty so
/// unrelated cards in `data/client/corpus.txt` stay untouched.
pub(crate) fn update_for(
    prefixes: &[String],
    groups: &[Representative<'_>],
) -> Result<usize, String> {
    let existing = if prefixes.is_empty() {
        BTreeSet::new()
    } else {
        names(&recordings::root())?
    };
    let fixture_names = fixtures()?;
    let group_names: Vec<&str> = groups.iter().map(|group| group.name).collect();
    let updated = merge_for(&existing, &fixture_names, prefixes, &group_names);
    let mut contents = updated.iter().cloned().collect::<Vec<_>>().join("\n");
    contents.push('\n');
    let path = path();
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&temporary, contents)
        .and_then(|()| fs::rename(&temporary, &path))
        .map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(updated.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn bare_prefix_matches_card_directory_without_sibling_prefix_collision() {
        assert!(matches_prefix("island/play", "island"));
        assert!(matches_prefix("island/play", "island/"));
        assert!(matches_prefix("verbs-zones", "verbs-zones"));
        assert!(!matches_prefix("island-sanctuary/upkeep", "island"));
    }

    #[test]
    fn scoped_merge_replaces_only_requested_prefixes() {
        let existing = set(&[
            "island/old-name",
            "island-sanctuary/upkeep",
            "shahrazad/subgame",
        ]);
        let fixtures = set(&["island/fixture-one", "other/fixture"]);
        let prefixes = vec!["island/".to_owned()];

        let merged = merge_for(&existing, &fixtures, &prefixes, &["island/new-name"]);

        assert_eq!(
            merged,
            set(&[
                "island/fixture-one",
                "island/new-name",
                "island-sanctuary/upkeep",
                "shahrazad/subgame",
            ])
        );
    }
}
