//! Provenance follows the placement and style selected by the same client as `tree`.

use beryl_inspect::{cli, pages};
use beryl_protocol::protocol::{Side, Status};
use serde_json::Value;

fn run(args: &[&str]) -> cli::Ran {
    cli::main(&args.iter().map(ToString::to_string).collect::<Vec<_>>())
}

fn explain(args: &[&str]) -> Value {
    let output = run(&[["explain"].as_slice(), args, &["--json"]].concat());
    assert_eq!(output.code, 0, "{}", output.stderr);
    serde_json::from_str(&output.stdout).expect("explanation as JSON")
}

#[test]
fn a_crowded_battlefield_row_reports_overlap_and_its_region() {
    let value = explain(&["story/game_table/stress-255", "--at", "start", "card #1"]);
    let place = &value["place"];
    assert_eq!(place["container"], "battlefield.yours.creatures");
    assert_eq!(place["policy"], "row");
    assert_eq!(place["region"]["key"], "table.creatures");
    assert!(
        place["stages"]
            .as_array()
            .unwrap()
            .contains(&"overlapped".into()),
        "{value}"
    );
    assert!(!place["limits"].as_array().unwrap().is_empty(), "{value}");
}

#[test]
fn a_two_row_hand_displaces_the_docks_preferred_slot() {
    let payment = pages::recording("pay-default", Side::You).unwrap();
    let decision = payment
        .views()
        .unwrap()
        .into_iter()
        .find_map(|update| match update.view.status {
            Status::Deciding { decision }
                if decision
                    .prompt
                    .buttons
                    .iter()
                    .any(|button| button.label == "Auto Pay") =>
            {
                Some(decision)
            }
            _ => None,
        })
        .expect("recorded payment decision");
    let source = format!(
        "story table 393x759 phone\ncard \"Hand {{n}}\" hand.yours x8\nprompt {}\n",
        serde_json::to_string(&decision).unwrap()
    );
    let path = std::env::temp_dir().join(format!(
        "b3d-explain-raised-dock-{}.story",
        std::process::id()
    ));
    std::fs::write(&path, source).unwrap();
    let result = run(&[
        "explain",
        path.to_str().unwrap(),
        "--at",
        "end",
        "region \"dock\"",
        "--json",
    ]);
    std::fs::remove_file(path).unwrap();
    assert_eq!(result.code, 0, "{}", result.stderr);
    let value: Value = serde_json::from_str(&result.stdout).unwrap();
    let place = &value["place"];
    let attempts = place["attempts"].as_array().unwrap();
    assert_eq!(place["profile"], "phone");
    assert_eq!(attempts[0]["slot"], "prompt.dock", "{value}");
    assert_eq!(attempts[0]["selected"], false, "{value}");
    assert!(
        attempts[0]["blocked_by"]
            .as_array()
            .unwrap()
            .iter()
            .any(|blocker| blocker["group"] == "viewer hand"),
        "{value}"
    );
    assert!(
        attempts
            .iter()
            .skip(1)
            .any(|attempt| attempt["selected"] == true),
        "{value}"
    );
}

#[test]
fn a_payment_button_reports_widget_defaults_and_the_dock_override() {
    let args = &["pay-default", "--at", "prompt:2", "button \"Auto Pay\""];
    let value = explain(args);
    assert_eq!(value["place"]["layout"]["policy"], "flexbox");
    assert_eq!(value["place"]["slot"], "prompt.bar");
    assert_eq!(
        value["owner"]["directory"],
        "rust/beryl3d/crates/game_hud/src/prompts/bar"
    );
    let look = value["look"].as_array().unwrap();
    let style = |property| {
        look.iter()
            .find(|style| style["property"] == property)
            .unwrap_or_else(|| panic!("missing {property} in {value}"))
    };
    for (property, expected) in [
        ("background", "Some([0, 0, 0, 0])"),
        ("text_align", "Center"),
        ("fit", "true"),
    ] {
        let row = style(property);
        assert_eq!(row["value"], expected);
        assert_eq!(row["source"]["kind"], "widget");
        assert_eq!(row["source"]["key"], "kit_ui::widgets::button::button");
        assert_eq!(
            row["source"]["file"],
            "rust/beryl3d/crates/kit_ui/src/widgets/button.rs"
        );
    }
    let minimum = style("min_width");
    assert_eq!(minimum["value"], "Some(0.0)");
    assert_eq!(minimum["source"]["kind"], "view");
    assert_eq!(
        minimum["source"]["key"],
        "game_hud::prompts::bar::view::bar"
    );
    assert_eq!(
        minimum["source"]["file"],
        "rust/beryl3d/crates/game_hud/src/prompts/bar/view.rs"
    );
    let color = style("color");
    assert_eq!(color["source"]["key"], "flat.color.PRIMARY_INK");
    assert_eq!(color["source"]["file"], "data/design-tokens.json");
    let animation = style("layout_slide");
    assert_eq!(animation["source"]["file"], "data/beryl3d/flat.json");
    assert_eq!(animation["source"]["key"], "transition_ms");
    assert!(look.iter().all(|style| style["source"]["file"].is_string()));
    let text = run(&[["explain"].as_slice(), args].concat());
    assert_eq!(text.code, 0, "{}", text.stderr);
    assert!(text.stdout.starts_with("just b3d explain pay-default "));
    assert!(text.stdout.contains("kit_ui::widgets::button::button"));
    assert!(text.stdout.contains("PRIMARY_INK"));
}

#[test]
fn a_widget_property_reset_to_the_base_style_still_reports_the_override() {
    let mut button = kit_ui::widgets::button("Cancel");
    button.style.fit = false;
    let look = game_probe::explain::styles(&button, "view", "view.rs", None);
    let fit = look
        .iter()
        .find(|style| style["property"] == "fit")
        .unwrap();
    assert_eq!(fit["value"], "false");
    assert_eq!(fit["source"]["kind"], "view");
    assert_eq!(fit["source"]["file"], "view.rs");
}

#[test]
fn a_missing_locator_reuses_the_trees_nearest_names() {
    let output = run(&[
        "explain",
        "pay-default",
        "--at",
        "prompt:2",
        "button \"Auto Pya\"",
    ]);
    assert_eq!(output.code, 2, "{}", output.stdout);
    assert!(output.stderr.contains("nearest names"));
    assert!(output.stderr.contains("button \"Auto Pay\""));
}

#[test]
fn a_moving_card_reports_the_step_that_supplied_its_track() {
    let value = explain(&["pay-default", "--at", "step:2.0.0", "card #1"]);
    assert_eq!(value["motion"], serde_json::json!(["step:2.0.0 (fly)"]));
}
