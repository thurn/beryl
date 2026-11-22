//! Tuning values must parse and report the key at fault when they do not.

#[path = "../../../../test_profiles.rs"]
mod test_profiles;

use test_profiles::SHIPPED;

use kit_base::data::{Card, Data, profiles};
use serde_json::Value;

const VERBS: &str = include_str!("../../../../../../data/beryl3d/verbs.json");
const EFFECTS: [(&str, &str); 14] = [
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
    (
        "settle",
        include_str!("../../../../../../data/beryl3d/effects/settle.json"),
    ),
    (
        "dim",
        include_str!("../../../../../../data/beryl3d/effects/dim.json"),
    ),
    (
        "blanch",
        include_str!("../../../../../../data/beryl3d/effects/blanch.json"),
    ),
    (
        "cover",
        include_str!("../../../../../../data/beryl3d/effects/cover.json"),
    ),
    (
        "impact",
        include_str!("../../../../../../data/beryl3d/effects/impact.json"),
    ),
    (
        "shake",
        include_str!("../../../../../../data/beryl3d/effects/shake.json"),
    ),
    (
        "trail",
        include_str!("../../../../../../data/beryl3d/effects/trail.json"),
    ),
    (
        "flash",
        include_str!("../../../../../../data/beryl3d/effects/flash.json"),
    ),
    (
        "beam",
        include_str!("../../../../../../data/beryl3d/effects/beam.json"),
    ),
    (
        "pulse",
        include_str!("../../../../../../data/beryl3d/effects/pulse.json"),
    ),
    (
        "gain",
        include_str!("../../../../../../data/beryl3d/effects/gain.json"),
    ),
];
const AUDIT: &str = include_str!("../../../../../../data/beryl3d/audit.json");
const CARD: &str = include_str!("../../../../../../data/beryl3d/card.json");
const BADGES: &str = include_str!("../../../../../../data/beryl3d/badges.json");

fn card() -> Card {
    Card::parse(CARD).expect("repository card")
}

fn parse(changed: Option<(&str, &str)>) -> Result<Data, String> {
    parse_with_effects(changed, EFFECTS)
}

fn parse_with_effects(
    changed: Option<(&str, &str)>,
    effects: impl IntoIterator<Item = (&'static str, &'static str)>,
) -> Result<Data, String> {
    Data::parse(
        |name| {
            if let Some((file, text)) = changed
                && name == file
            {
                return Ok(text.to_owned());
            }
            let text = match name {
                "card.json" => CARD,
                "badges.json" => BADGES,
                "verbs.json" => VERBS,
                "audit.json" => AUDIT,
                "flat.json" => include_str!("../../../../../../data/beryl3d/flat.json"),
                "hud.json" => include_str!("../../../../../../data/beryl3d/hud.json"),
                "catch-up.json" => include_str!("../../../../../../data/beryl3d/catch-up.json"),
                "frame-rate.json" => include_str!("../../../../../../data/beryl3d/frame-rate.json"),
                "slide.json" => include_str!("../../../../../../data/beryl3d/slide.json"),
                "tier-probe.json" => include_str!("../../../../../../data/beryl3d/tier-probe.json"),
                "notices.json" => include_str!("../../../../../../data/beryl3d/notices.json"),
                "revealed.json" => include_str!("../../../../../../data/beryl3d/revealed.json"),
                "shown.json" => include_str!("../../../../../../data/beryl3d/shown.json"),
                "gallery.json" => include_str!("../../../../../../data/beryl3d/gallery.json"),
                "lighting.json" => include_str!("../../../../../../data/beryl3d/lighting.json"),
                "light.json" => include_str!("../../../../../../data/beryl3d/light.json"),
                "own.json" => include_str!("../../../../../../data/beryl3d/own.json"),
                "pointer.json" => include_str!("../../../../../../data/beryl3d/pointer.json"),
                _ => return Err(format!("{name}: missing test tuning")),
            };
            Ok(text.to_owned())
        },
        &SHIPPED,
        effects,
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
    let (file, text) = SHIPPED[1];
    let profile: Value = serde_json::from_str(text).unwrap();

    for surplus in [false, true] {
        let mut changed = profile.clone();
        let positions = changed["gallery"]["panels_at"].as_array_mut().unwrap();
        if surplus {
            positions.push(positions[0].clone());
        } else {
            positions.remove(0);
        }
        let why = profiles::Profiles::parse(&[SHIPPED[0], (file, &changed.to_string())], card())
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
fn flat_tuning_errors_name_the_file_and_nested_setting() {
    let mut flat: serde_json::Value =
        serde_json::from_str(include_str!("../../../../../../data/beryl3d/flat.json")).unwrap();
    flat["manual"]["selection"]["radius"] = serde_json::json!(-1);
    let error = parse(Some(("flat.json", &flat.to_string()))).unwrap_err();
    assert!(
        error.starts_with("flat.json: manual.selection.radius:"),
        "{error}"
    );
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
fn every_shipped_effect_is_required() {
    for (missing, _) in EFFECTS {
        let effects = EFFECTS.into_iter().filter(|&(name, _)| name != missing);
        let error = parse_with_effects(None, effects)
            .expect_err("missing shipped effects must fail tuning validation");
        assert!(
            error.starts_with(&format!("effects/{missing}.json:")),
            "{error}"
        );
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
fn a_parameterless_surface_rejects_an_unknown_key() {
    use kit_base::data::effect::Effects;

    let text = include_str!("../../../../../../data/beryl3d/effects/blanch.json");
    let mut blanch: Value = serde_json::from_str(text).unwrap();
    blanch["layers"][0]["surface"]["unused_key"] = true.into();
    let why = Effects::parse([("blanch", blanch.to_string().as_str())]).unwrap_err();
    assert!(why.contains("effects/blanch.json: layers[0]"), "{why}");
    assert!(why.contains("unknown field `unused_key`"), "{why}");
}

#[test]
fn reversed_ramps_name_the_layer_parameter() {
    use kit_base::data::effect::Effects;
    use serde_json::json;

    let reversed = json!({"from": 0, "by": 1, "start": 0.8, "end": 0.7});
    let ordinary = json!({"from": 0, "by": 1});
    for (curve, suffix) in [
        (reversed.clone(), ""),
        (json!({"cycles": 2, "amplitude": reversed}), ".amplitude"),
        (json!({"rise": reversed, "fall": ordinary}), ".rise"),
        (json!({"rise": ordinary, "fall": reversed}), ".fall"),
    ] {
        let mut glow: Value = serde_json::from_str(EFFECTS[0].1).unwrap();
        glow["layers"][0]["quad"]["record"]["scale"] = curve;
        let error = Effects::parse([("glow", glow.to_string().as_str())]).unwrap_err();
        assert!(
            error.starts_with(&format!(
                "effects/glow.json: layers[0].quad.record.scale{suffix}:"
            )),
            "{error}"
        );
        assert!(error.contains("start") && error.contains("end"), "{error}");
    }
}

#[test]
fn the_shipped_cover_ramp_can_change_instantly() {
    use kit_base::data::effect::Primitive;

    let data = parse(None).expect("repository tuning");
    let Primitive::Surface { amount, .. } = &data.effects.get("cover").unwrap().layers[1].primitive
    else {
        panic!("cover's previous face should be a surface effect");
    };
    assert!((amount.at(0.49) - 1.0).abs() < f32::EPSILON);
    assert!(amount.at(0.5).abs() < f32::EPSILON);
}

#[test]
fn invalid_camera_geometry_names_the_file_and_field() {
    use serde_json::json;

    let (file, text) = SHIPPED[0];
    for (field, value) in [
        ("pitch_degrees", json!(90)),
        ("fov_degrees", json!(0)),
        ("target", json!([0, 0, 1e39])),
        ("required", json!([[1, 0], [0, 1]])),
    ] {
        let mut profile: Value = serde_json::from_str(text).unwrap();
        profile["table"]["camera"][field] = value;
        let error = profiles::Profiles::parse(&[(file, &profile.to_string()), SHIPPED[1]], card())
            .unwrap_err();
        assert!(
            error.starts_with(&format!("{file}: table.camera.{field}:")),
            "{error}"
        );
    }
}

#[test]
fn table_profiles_reject_unknown_geometry_fields() {
    use kit_base::data::profiles::Profiles;

    for (index, key, section) in [
        (0, "beneath_step", "table"),
        (0, "deck_backs", "table.tray_tuning.put_back"),
        (1, "bias_degrees", "table.anchored.hand"),
        (1, "spare", "table.anchored.stack"),
        (1, "narrow_step", "table.inspector"),
    ] {
        let mut files = SHIPPED;
        let (file, text) = files[index];
        let invalid = text.replacen(&format!("\"{key}\""), "\"unknown_measurement\"", 1);
        files[index] = (file, &invalid);
        let why = Profiles::parse(&files, card()).unwrap_err();
        assert!(why.contains(&format!("{file}: {section}")), "{why}");
        assert!(why.contains("unknown field `unknown_measurement`"), "{why}");
    }
}

#[test]
fn a_profile_rejects_an_unknown_named_slot() {
    let (file, text) = SHIPPED[0];
    let mut profile: Value = serde_json::from_str(text).unwrap();
    profile["slots"]["unused.slot"] = profile["slots"]["screen.column"].clone();
    let why =
        profiles::Profiles::parse(&[(file, &profile.to_string()), SHIPPED[1]], card()).unwrap_err();
    assert!(
        why.starts_with(file) && why.contains("slots.unused.slot: unknown field `unused.slot`"),
        "{why}"
    );
}

/// A card's size and the badges' sizes are read at run time, so a supplied size reaches every
/// camera the profiles fit, and the badges' tuning is the file's.
#[test]
fn card_and_badge_sizes_come_from_their_files() {
    let wide = CARD.replacen("\"width\": 63", "\"width\": 70", 1);
    let data = parse(Some(("card.json", &wide))).expect("a wider card");
    assert!((data.card.width - 70.0).abs() < f32::EPSILON);
    for profile in data.profiles.iter() {
        assert_eq!(profile.table.camera.card, data.card);
    }
    let larger = BADGES.replacen("\"numeral_points\": 17", "\"numeral_points\": 19", 1);
    let data = parse(Some(("badges.json", &larger))).expect("larger numerals");
    assert!((data.badges.numeral_points - 19.0).abs() < f32::EPSILON);
}

/// The card and badge files name the key at fault: an unknown one, or a size out of range.
#[test]
fn card_and_badge_files_reject_bad_keys() {
    let unknown = CARD.replacen("\"layer\"", "\"thickness\"", 1);
    let why = parse(Some(("card.json", &unknown))).unwrap_err();
    assert!(
        why.contains("card.json") && why.contains("thickness"),
        "{why}"
    );
    let flat = CARD.replacen("\"height\": 88", "\"height\": 0", 1);
    let why = parse(Some(("card.json", &flat))).unwrap_err();
    assert!(why.starts_with("card.json: height"), "{why}");
    let unknown = BADGES.replacen("\"min_width\"", "\"least_width\"", 1);
    let why = parse(Some(("badges.json", &unknown))).unwrap_err();
    assert!(
        why.contains("badges.json: plaque") && why.contains("least_width"),
        "{why}"
    );
}
