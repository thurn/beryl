//! `data/beryl3d/parity.json`'s verbs, effects and marks, which the parity list counts as done
//! when the recording named for each is done. A name that is wrong would count an item done that
//! Beryl 3D never shows.

use std::collections::BTreeMap;

use app_client::driver::TimelineStep;
use beryl_inspect::{pages, recordings};
use beryl_protocol::Seat;
use beryl_protocol::protocol::Side;
use kit_probe::agent::Layout;
use kit_ui::input::Size;

fn parity() -> serde_json::Value {
    let text = beryl_test_files::read_to_string("data/beryl3d/parity.json")
        .expect("the parity file is readable");
    serde_json::from_str(&text).expect("the parity file is JSON")
}

/// Each item of `key` in the parity file, with the recording it names.
fn mapped(file: &serde_json::Value, key: &str) -> Vec<(String, String)> {
    let mapped = file[key]
        .as_object()
        .unwrap_or_else(|| panic!("the parity file maps {key}"));
    assert!(!mapped.is_empty(), "the parity file maps no {key}");
    mapped
        .iter()
        .map(|(item, recording)| {
            let recording = recording.as_str().expect("each item names a recording");
            (item.clone(), recording.to_owned())
        })
        .collect()
}

/// Every step of `recording`'s timeline from each seat the pack records it from, in the usual
/// desktop window.
fn steps(recording: &str, seats: &[Side]) -> Vec<TimelineStep> {
    let size = Size {
        width: 1600.0,
        height: 900.0,
    };
    seats
        .iter()
        .flat_map(|&seat| {
            pages::page(recording, seat, Layout::Desktop, size)
                .unwrap_or_else(|why| panic!("{recording}: {why}"))
                .timeline()
        })
        .collect()
}

/// Every recording the parity file names for a verb, an effect or a mark is a recording of the
/// pack, so the parity list counts each by a recording the tests play.
#[test]
fn the_parity_file_names_recordings_of_the_pack() {
    let file = parity();
    let names: Vec<String> = recordings::names()
        .expect("the recordings pack is readable")
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    for key in ["verbs", "effects", "marks"] {
        for (item, recording) in mapped(&file, key) {
            assert!(
                names.contains(&recording),
                "{key}: {item} names {recording:?}, which `just b3d list` does not list"
            );
        }
    }
}

/// The recording the parity file names for a verb plays a step of that verb, and the one it names
/// for an effect plays a step with that effect, from one seat or the other. Otherwise the parity
/// list would count the verb or effect done because an unrelated recording passes its tests.
/// Marks are a card's look while a prompt is open, not a step, so only their recording's
/// existence is checked.
#[test]
fn each_verb_and_effect_plays_in_its_recording() {
    let file = parity();
    let mut seats: BTreeMap<String, Vec<Side>> = BTreeMap::new();
    for (name, seat) in recordings::names().expect("the recordings pack is readable") {
        let side = if seat == Seat::P0 {
            Side::You
        } else {
            Side::Opponent
        };
        seats.entry(name).or_default().push(side);
    }
    let mut timelines: BTreeMap<String, Vec<TimelineStep>> = BTreeMap::new();
    let mut timeline = |recording: &str| -> Vec<TimelineStep> {
        timelines
            .entry(recording.to_owned())
            .or_insert_with(|| steps(recording, &seats[recording]))
            .clone()
    };
    for (verb, recording) in mapped(&file, "verbs") {
        assert!(
            timeline(&recording).iter().any(|s| s.verb.name() == verb),
            "verbs: {recording} plays no `{verb}` step from either seat"
        );
    }
    for (effect, recording) in mapped(&file, "effects") {
        assert!(
            (timeline(&recording).iter()).any(|s| s.effects.iter().any(|(e, ..)| *e == effect)),
            "effects: {recording} plays no step with `{effect}` from either seat"
        );
    }
}
