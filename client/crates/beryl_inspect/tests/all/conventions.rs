//! The conventions files, `data/client/conventions/<kind>.json`: how the client follows each
//! entry of `data/ui-conventions.json`, one file per section kind, for the parity list to count.

use std::collections::BTreeSet;

use beryl_inspect::recordings;

/// The section kinds of `data/ui-conventions.json` that have a conventions file.
const KINDS: [&str; 4] = ["exception", "settled", "no-equivalent", "not-seen"];

fn read(path: &str) -> serde_json::Value {
    let text = beryl_test_files::read_to_string(path)
        .unwrap_or_else(|why| panic!("{path} is not readable: {why}"));
    serde_json::from_str(&text).unwrap_or_else(|why| panic!("{path} is not JSON: {why}"))
}

/// Each conventions file's entries, by name, with the file's kind.
fn entries() -> Vec<(&'static str, String, serde_json::Value)> {
    KINDS
        .iter()
        .flat_map(|&kind| {
            let file = read(&format!("data/client/conventions/{kind}.json"));
            file["entries"]
                .as_object()
                .unwrap_or_else(|| panic!("conventions/{kind}.json has `entries`"))
                .iter()
                .map(|(name, entry)| (kind, name.clone(), entry.clone()))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// A conventions file's key that names no entry of its section kind would count a convention
/// that does not exist, or one of another kind.
#[test]
fn every_key_names_an_entry_of_its_section() {
    let conventions = read("data/ui-conventions.json");
    let listed = conventions["entries"]
        .as_array()
        .expect("ui-conventions.json lists entries");
    for (kind, name, _) in entries() {
        assert!(
            listed
                .iter()
                .any(|e| e["kind"] == kind && e["name"].as_str() == Some(&name)),
            "conventions/{kind}.json: {name:?} is no `{kind}` entry of data/ui-conventions.json"
        );
    }
}

/// Each entry is either a named recording from the pack with its `at` moment and `shows`
/// description, or an `open` entry with its reason. Tree lines are not matched at runtime
/// because intentional wording, layout, or id changes would break hardcoded substrings.
#[test]
fn every_entry_names_a_recording_or_is_open() {
    let known: BTreeSet<String> = recordings::names()
        .expect("the recordings pack is readable")
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    for (kind, name, entry) in entries() {
        let label = format!("conventions/{kind}.json: {name}");
        if let Some(recording) = entry["recording"].as_str() {
            assert!(
                known.contains(recording),
                "{label}: unknown recording {recording:?}"
            );
            assert!(
                entry["at"].as_str().is_some_and(|s| !s.is_empty()),
                "{label} names no `at`"
            );
            assert!(
                entry["shows"].as_str().is_some_and(|s| !s.is_empty()),
                "{label} names no `shows`"
            );
        } else {
            assert!(
                entry["open"].as_str().is_some_and(|why| !why.is_empty()),
                "{label} is neither {{\"recording\", \"at\", \"shows\"}} nor {{\"open\"}}"
            );
        }
    }
}
