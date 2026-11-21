//! Tuning values must parse and report the key at fault when they do not.

use kit_base::data::{Data, profiles};
use serde_json::Value;

const SPACE: &str = include_str!("../../../../../../data/beryl3d/space.json");
const EFFECTS: [(&str, &str); 3] = [
    (
        "glow",
        include_str!("../../../../../../data/beryl3d/effects/glow.json"),
    ),
    (
        "dissolve",
        include_str!("../../../../../../data/beryl3d/effects/dissolve.json"),
    ),
    (
        "loss",
        include_str!("../../../../../../data/beryl3d/effects/loss.json"),
    ),
];
const GATES: &str = include_str!("../../../../../../data/beryl3d/gates.json");

#[test]
fn the_repository_tuning_loads() {
    let data = Data::parse(SPACE, GATES, &profiles::SHIPPED, EFFECTS).expect("repository tuning");
    assert!(data.verbs.fly.arc.is_finite());
    assert!(data.audit.mark_points > 0.0);
}

#[test]
fn an_unknown_key_names_its_file_and_path() {
    let mut space: Value = serde_json::from_str(SPACE).unwrap();
    let fly = space["verbs"]["fly"].as_object_mut().unwrap();
    let arc = fly.remove("arc").unwrap();
    fly.insert("arx".to_owned(), arc);
    let why = Data::parse(&space.to_string(), GATES, &profiles::SHIPPED, EFFECTS).unwrap_err();
    assert!(why.contains("space.json: verbs.fly.arx"), "{why}");
    assert!(why.contains("unknown field"), "{why}");
}

#[test]
fn a_missing_key_names_its_file_and_path() {
    let mut gates: Value = serde_json::from_str(GATES).unwrap();
    gates["audit"].as_object_mut().unwrap().remove("seen");
    let why = Data::parse(SPACE, &gates.to_string(), &profiles::SHIPPED, EFFECTS).unwrap_err();
    assert!(why.contains("gates.json: audit.seen"), "{why}");
    assert!(why.contains("missing field"), "{why}");
}

#[test]
fn an_out_of_range_value_names_its_file_and_path() {
    let mut space: Value = serde_json::from_str(SPACE).unwrap();
    space["verbs"]["fly"]["arc"] = (-1).into();
    let why = Data::parse(&space.to_string(), GATES, &profiles::SHIPPED, EFFECTS).unwrap_err();
    assert!(why.contains("space.json: verbs.fly.arc"), "{why}");
    assert!(why.contains("nonnegative"), "{why}");
}

#[test]
fn effect_length_sources_match_the_shared_conventions() {
    use kit_base::data::effect::Length;

    let data = Data::parse(SPACE, GATES, &profiles::SHIPPED, EFFECTS).expect("repository tuning");
    let conventions: Value =
        serde_json::from_str(include_str!("../../../../../../data/ui-conventions.json")).unwrap();
    let shared = conventions["effects"].as_array().unwrap();
    for (name, _) in EFFECTS {
        let entry = shared.iter().find(|entry| entry["name"] == name).unwrap();
        let length = if entry["values"]["duration"].is_null() {
            Length::Step
        } else {
            Length::Effect
        };
        assert_eq!(data.effects.get(name).unwrap().length, length, "{name}");
    }
}

#[test]
fn effect_errors_name_the_file_and_bad_layer() {
    use kit_base::data::effect::Effects;

    let mut glow: Value = serde_json::from_str(EFFECTS[0].1).unwrap();
    glow["layers"][0]["shape"] = "missing_sprite".into();
    let why = Effects::parse([("glow", glow.to_string().as_str())]).unwrap_err();
    assert!(why.contains("effects/glow.json: layers[0]"), "{why}");
    assert!(why.contains("missing_sprite"), "{why}");
    glow["layers"][0]["shape"] = "halo".into();
    glow["layers"][0]["start"] = 0.8.into();
    glow["layers"][0]["end"] = 0.7.into();
    let why = Effects::parse([("glow", glow.to_string().as_str())]).unwrap_err();
    assert!(why.contains("effects/glow.json: layers[0]"), "{why}");
}
