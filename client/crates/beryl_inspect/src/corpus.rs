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
            return Err(format!(
                "{} names unknown recording {name:?}",
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

pub(crate) fn shapes() -> Result<Vec<Measured>, String> {
    let windows = Windows::All.windows();
    let mut measure = Measure::new(crate::data::checkout_ref()?, &windows);
    Ok(recordings::entries()?
        .chunk_by(|a, b| a.name == b.name)
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

/// Groups the selected pack by signature. The pack is sorted by name and seat, so insertion
/// chooses the first name in each group.
pub(crate) fn representatives() -> Result<Vec<Representative<'static>>, String> {
    let mut groups = HashMap::<Shape, Representative<'_>>::new();
    for (name, shape) in shapes()? {
        let representative = groups
            .entry(shape?)
            .or_insert(Representative { name, count: 0 });
        representative.count += 1;
    }
    let mut selected: Vec<_> = groups.into_values().collect();
    selected.sort_unstable_by_key(|group| group.name);
    Ok(selected)
}

/// Writes the representatives and every fixture in name order.
pub(crate) fn update(groups: &[Representative<'_>]) -> Result<usize, String> {
    let mut names = fixtures()?;
    names.extend(groups.iter().map(|group| group.name.to_owned()));
    let mut contents = names.iter().cloned().collect::<Vec<_>>().join("\n");
    contents.push('\n');
    let path = path();
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&temporary, contents)
        .and_then(|()| fs::rename(&temporary, &path))
        .map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(names.len())
}
