//! Tuning values must parse and report the key at fault when they do not.

use kit_base::data::Data;
use serde_json::Value;

const SPACE: &str = include_str!("../../../../../../data/beryl3d/space.json");
const GATES: &str = include_str!("../../../../../../data/beryl3d/gates.json");

#[test]
fn the_repository_tuning_loads() {
    let data = Data::parse(SPACE, GATES).expect("repository tuning");
    assert!(data.verbs.fly.arc.is_finite());
    assert!(data.audit.target_points > 0.0);
}

#[test]
fn an_unknown_key_names_its_file_and_path() {
    let mut space: Value = serde_json::from_str(SPACE).unwrap();
    let fly = space["verbs"]["fly"].as_object_mut().unwrap();
    let arc = fly.remove("arc").unwrap();
    fly.insert("arx".to_owned(), arc);
    let why = Data::parse(&space.to_string(), GATES).unwrap_err();
    assert!(why.contains("space.json: verbs.fly.arx"), "{why}");
    assert!(why.contains("unknown field"), "{why}");
}

#[test]
fn a_missing_key_names_its_file_and_path() {
    let mut gates: Value = serde_json::from_str(GATES).unwrap();
    gates["audit"].as_object_mut().unwrap().remove("seen");
    let why = Data::parse(SPACE, &gates.to_string()).unwrap_err();
    assert!(why.contains("gates.json: audit.seen"), "{why}");
    assert!(why.contains("missing field"), "{why}");
}

#[test]
fn an_out_of_range_value_names_its_file_and_path() {
    let mut space: Value = serde_json::from_str(SPACE).unwrap();
    space["verbs"]["fly"]["arc"] = (-1).into();
    let why = Data::parse(&space.to_string(), GATES).unwrap_err();
    assert!(why.contains("space.json: verbs.fly.arc"), "{why}");
    assert!(why.contains("nonnegative"), "{why}");
}
