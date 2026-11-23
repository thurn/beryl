//! Checks that the motion recordings exist and play the verbs and effects they document.

use std::collections::BTreeMap;

use app_client::driver::TimelineStep;
use beryl_inspect::{pages, recordings};
use beryl_protocol::Seat;
use beryl_protocol::protocol::Side;
use kit_base::data::profiles::SizeClass;
use kit_ui::input::Size;

fn motion_recordings() -> serde_json::Value {
    let text = beryl_test_files::read_to_string("data/beryl3d/motion-recordings.json")
        .expect("the motion recording map is readable");
    serde_json::from_str(&text).expect("the motion recording map is JSON")
}

/// Each item of `key` in the motion recording map, with the recording it names.
fn mapped(file: &serde_json::Value, key: &str) -> Vec<(String, String)> {
    let mapped = file[key]
        .as_object()
        .unwrap_or_else(|| panic!("the motion recording map maps {key}"));
    assert!(!mapped.is_empty(), "the motion recording map maps no {key}");
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
            pages::page(recording, seat, SizeClass::Regular, size)
                .unwrap_or_else(|why| panic!("{recording}: {why}"))
                .timeline()
        })
        .collect()
}

/// Each verb, effect or mark names a recording in the pack.
#[test]
fn the_motion_recordings_are_in_the_pack() {
    let file = motion_recordings();
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

/// Each mapped verb and effect plays from at least one seat. Marks belong to a card's look
/// while a prompt is open, so the timeline covers only the verbs and effects.
#[test]
fn each_verb_and_effect_plays_in_its_recording() {
    let file = motion_recordings();
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
