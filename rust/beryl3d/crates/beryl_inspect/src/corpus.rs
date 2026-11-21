//! The checked-in set of recordings used by default in the interface tests.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::PathBuf;

use beryl_protocol::Seat;
use game_probe::signature::{Measure, Signature};
use serde_json::Value;

use crate::pages::Windows;
use crate::{pack, recordings};

/// The first recording of a signature and the number of recording names that share it.
pub(crate) struct Representative<'a> {
    pub name: &'a str,
    pub count: usize,
}

fn path() -> PathBuf {
    recordings::root().join("data/beryl3d/corpus.txt")
}

/// The sorted recording names in the checked-in corpus, checked against the pack.
pub(crate) fn read() -> Result<BTreeSet<String>, String> {
    let path = path();
    let text = fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let available = recordings::entries()?;
    let mut names: BTreeSet<_> = text.lines().map(str::to_owned).collect();
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

fn fixtures() -> Result<BTreeSet<String>, String> {
    let path = recordings::root().join("ui/fixtures.json");
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

/// Groups pack entries by the ordered pair of signatures for their recorded seats. The pack
/// is sorted by name and seat, so insertion chooses the first name in each group.
pub(crate) fn representatives() -> Result<Vec<Representative<'static>>, String> {
    let windows = Windows::All.windows();
    let mut measure = Measure::new(crate::data::checkout_ref()?, &windows);
    let mut groups = HashMap::<Vec<(Seat, Signature)>, Representative<'_>>::new();
    for entries in recordings::entries()?.chunk_by(|a, b| a.name == b.name) {
        let name = entries[0].name;
        let mut signatures = Vec::new();
        for entry in entries {
            let signature = pack::messages(entry)
                .and_then(|messages| measure.recording(&messages))
                .map_err(|error| format!("{name} seat {}: {error}", entry.seat))?;
            signatures.push((entry.seat(), signature));
        }
        let representative = groups
            .entry(signatures)
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
