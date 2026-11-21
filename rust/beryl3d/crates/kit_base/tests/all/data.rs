//! Tuning values must parse and report the key at fault when they do not.

use kit_base::data::{Data, profiles};
use serde_json::Value;

const VERBS: &str = include_str!("../../../../../../data/beryl3d/verbs.json");
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
const AUDIT: &str = include_str!("../../../../../../data/beryl3d/audit.json");

fn parse(changed: Option<(&str, &str)>) -> Result<Data, String> {
    Data::parse(
        |name| {
            if let Some((file, text)) = changed
                && name == file
            {
                return Ok(text.to_owned());
            }
            let text = match name {
                "verbs.json" => VERBS,
                "audit.json" => AUDIT,
                "catch-up.json" => include_str!("../../../../../../data/beryl3d/catch-up.json"),
                "frame-rate.json" => include_str!("../../../../../../data/beryl3d/frame-rate.json"),
                "slide.json" => include_str!("../../../../../../data/beryl3d/slide.json"),
                "tier-probe.json" => include_str!("../../../../../../data/beryl3d/tier-probe.json"),
                "notices.json" => include_str!("../../../../../../data/beryl3d/notices.json"),
                "revealed.json" => include_str!("../../../../../../data/beryl3d/revealed.json"),
                "shown.json" => include_str!("../../../../../../data/beryl3d/shown.json"),
                "gallery.json" => include_str!("../../../../../../data/beryl3d/gallery.json"),
                "lighting.json" => include_str!("../../../../../../data/beryl3d/lighting.json"),
                "pointer.json" => include_str!("../../../../../../data/beryl3d/pointer.json"),
                _ => return Err(format!("{name}: missing test tuning")),
            };
            Ok(text.to_owned())
        },
        &profiles::SHIPPED,
        EFFECTS,
    )
}

#[test]
fn the_repository_tuning_loads() {
    let data = parse(None).expect("repository tuning");
    assert!(data.verbs.fly.arc.is_finite());
    assert!(data.audit.mark_points > 0.0);
}

#[test]
fn reference_gallery_rejects_missing_and_surplus_panel_positions() {
    let (file, text) = profiles::SHIPPED[1];
    let profile: Value = serde_json::from_str(text).unwrap();

    for surplus in [false, true] {
        let mut changed = profile.clone();
        let positions = changed["gallery"]["panels_at"].as_array_mut().unwrap();
        if surplus {
            positions.push(positions[0].clone());
        } else {
            positions.remove(0);
        }
        let why = profiles::Profiles::parse(&[profiles::SHIPPED[0], (file, &changed.to_string())])
            .unwrap_err();
        assert!(
            why.starts_with(&format!("{file}: gallery.panels_at")),
            "{why}"
        );
        assert!(
            why.contains("invalid length") || why.contains("trailing characters"),
            "{why}"
        );
    }
}

#[test]
fn an_unknown_key_names_its_file_and_path() {
    let mut space: Value = serde_json::from_str(VERBS).unwrap();
    let fly = space["verbs"]["fly"].as_object_mut().unwrap();
    let arc = fly.remove("arc").unwrap();
    fly.insert("arx".to_owned(), arc);
    let why = parse(Some(("verbs.json", &space.to_string()))).unwrap_err();
    assert!(why.contains("verbs.json: verbs.fly.arx"), "{why}");
    assert!(why.contains("unknown field"), "{why}");
}

#[test]
fn an_unused_audit_key_fails_tuning_validation() {
    let mut audit: Value = serde_json::from_str(AUDIT).unwrap();
    audit["retired_threshold"] = 1.into();
    let why = parse(Some(("audit.json", &audit.to_string()))).unwrap_err();
    assert!(why.contains("audit.json: retired_threshold"), "{why}");
    assert!(why.contains("unknown field"), "{why}");
}

#[test]
fn a_missing_key_names_its_file_and_path() {
    let mut audit: Value = serde_json::from_str(AUDIT).unwrap();
    audit.as_object_mut().unwrap().remove("seen");
    let why = parse(Some(("audit.json", &audit.to_string()))).unwrap_err();
    assert!(why.contains("audit.json: seen"), "{why}");
    assert!(why.contains("missing field"), "{why}");
}

#[test]
fn an_out_of_range_value_names_its_file_and_path() {
    let mut space: Value = serde_json::from_str(VERBS).unwrap();
    space["verbs"]["fly"]["arc"] = (-1).into();
    let why = parse(Some(("verbs.json", &space.to_string()))).unwrap_err();
    assert!(why.contains("verbs.json: verbs.fly.arc"), "{why}");
    assert!(why.contains("nonnegative"), "{why}");
}

#[test]
fn effect_length_sources_match_the_shared_conventions() {
    use kit_base::data::effect::Length;

    let data = parse(None).expect("repository tuning");
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

#[test]
fn table_profiles_reject_unknown_geometry_fields() {
    use kit_base::data::profiles::{Profiles, SHIPPED};

    for (index, key, section) in [
        (0, "beneath_step", "table"),
        (0, "deck_backs", "table.tray_tuning.put_back"),
        (1, "vanishing_y", "table.anchored.web.perspective"),
        (1, "bias_degrees", "table.anchored.hand"),
        (1, "spare", "table.anchored.stack"),
        (1, "narrow_step", "table.inspector"),
    ] {
        let mut files = SHIPPED;
        let (file, text) = files[index];
        let invalid = text.replacen(&format!("\"{key}\""), "\"unknown_measurement\"", 1);
        files[index] = (file, &invalid);
        let why = Profiles::parse(&files).unwrap_err();
        assert!(why.contains(&format!("{file}: {section}")), "{why}");
        assert!(why.contains("unknown field `unknown_measurement`"), "{why}");
    }
}
