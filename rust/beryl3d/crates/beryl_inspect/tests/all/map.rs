//! The client map reports the registered parts and gives a path to every entry.

use std::collections::BTreeSet;
use std::path::Path;

use beryl_inspect::cli::{Ran, main};
use beryl_protocol::script::Verb;
use game_hud::labels::GAME_LABELS;
use game_hud::navigation::{Screen, Sheet};
use game_table::table::Group;
use kit_base::data::{TUNING_FILES, effect::PrimitiveKind};
use kit_frame::frame::Role;
use kit_probe::agent::Rule;
use kit_space::arrange::PolicyKind;
use kit_ui::widgets::Widget;
use serde_json::Value;

fn run(line: &[&str]) -> Ran {
    main(
        &line
            .iter()
            .map(|part| (*part).to_owned())
            .collect::<Vec<_>>(),
    )
}

fn output(line: &[&str]) -> String {
    let ran = run(line);
    assert_eq!(ran.code, 0, "{line:?}: {}", ran.stderr);
    ran.stdout
}

fn json(line: &[&str]) -> Value {
    serde_json::from_str(&output(line)).expect("valid map JSON")
}

fn inventory(kind: &str) -> Vec<Value> {
    json(&["map", kind, "--full", "--json"])["entries"]
        .as_array()
        .unwrap()
        .clone()
}

fn names(entries: &[Value]) -> BTreeSet<&str> {
    entries
        .iter()
        .map(|entry| entry["name"].as_str().unwrap())
        .collect()
}

#[test]
fn every_map_inventory_matches_its_overview_and_prints_existing_paths() {
    let overview = json(&["map", "--json"]);
    assert_eq!(overview["command"], "just b3d map --json");
    assert_eq!(output(&["map"]).lines().next(), Some("just b3d map"));
    let kinds = overview["kinds"].as_array().unwrap();
    assert_eq!(
        names(kinds),
        BTreeSet::from([
            "crates",
            "features",
            "containers",
            "profiles",
            "verbs",
            "effects",
            "primitives",
            "policies",
            "roles",
            "widgets",
            "data",
            "checks",
            "stories",
            "commands",
        ])
    );
    let root = beryl_inspect::recordings::root();
    let path_exists = |entry: &Value| {
        let path = Path::new(entry["path"].as_str().unwrap());
        assert!(root.join(path).exists(), "missing source path in {entry}");
    };
    for kind in kinds {
        path_exists(kind);
        let name = kind["name"].as_str().unwrap();
        let entries = inventory(name);
        assert!(!entries.is_empty(), "{name}");
        assert_eq!(kind["count"], entries.len(), "{name}");
        assert_eq!(names(&entries).len(), entries.len(), "duplicates in {name}");
        entries.iter().for_each(path_exists);
        let text = output(&["map", name, "--full"]);
        assert_eq!(
            text.lines().next(),
            Some(format!("just b3d map {name} --full").as_str())
        );
        for (line, entry) in text.lines().skip(1).zip(&entries) {
            let path = entry["path"].as_str().unwrap();
            assert!(line.ends_with(path), "{line} should give {path}");
        }
        assert_eq!(text.lines().count(), entries.len() + 1, "{name}");
    }
}

#[test]
fn inventories_cover_their_owning_registries() {
    let data = beryl_inspect::data::checkout().unwrap();
    let checks = [
        ("verbs", Verb::ALL.iter().map(|verb| verb.name()).collect()),
        (
            "effects",
            data.effects
                .iter()
                .map(|effect| effect.name.as_ref())
                .collect(),
        ),
        (
            "primitives",
            PrimitiveKind::ALL.iter().map(|kind| kind.word()).collect(),
        ),
        (
            "policies",
            PolicyKind::ALL.iter().map(|kind| kind.word()).collect(),
        ),
        ("roles", GAME_LABELS.roles().map(Role::word).collect()),
        (
            "widgets",
            Widget::ALL.iter().map(|widget| widget.word()).collect(),
        ),
        ("checks", Rule::ALL.into_iter().map(Rule::word).collect()),
    ];
    for (kind, expected) in checks {
        assert_eq!(names(&inventory(kind)), expected, "{kind}");
    }
    let mapped_data = inventory("data");
    for file in TUNING_FILES {
        let entry = mapped_data
            .iter()
            .find(|entry| entry["name"] == file.name)
            .unwrap();
        assert_eq!(entry["detail"], file.reader);
    }
    for name in names(&inventory("profiles")) {
        assert_eq!(
            inventory("containers")
                .iter()
                .filter(|entry| {
                    entry["name"].as_str().unwrap().split_once(' ').unwrap().0 == name
                })
                .count(),
            Group::ALL.len() + 1,
            "{name}"
        );
    }
}

#[test]
fn feature_map_covers_every_route_and_the_directly_composed_hud() {
    let features = inventory("features");
    let names_at = |placement: &str| -> BTreeSet<&str> {
        features
            .iter()
            .filter_map(|entry| {
                let mut words = entry["detail"].as_str()?.split_whitespace();
                words.next().filter(|&kind| kind == placement)?;
                words.next()
            })
            .collect()
    };
    assert_eq!(
        names_at("screen"),
        BTreeSet::from(Screen::ALL.map(Screen::name))
    );
    assert_eq!(
        names_at("sheet"),
        BTreeSet::from(Sheet::ALL.map(Sheet::name))
    );

    let composed = [
        ("game", "screens/game.rs", "screen game"),
        ("opening", "screens/opening.rs", "game overlay"),
        ("phase-indicator", "hud/phase.rs", "compact HUD"),
        ("menu-button", "hud/phase.rs", "compact HUD"),
    ];
    let mut expected: BTreeSet<_> = app_client::registered_features()
        .map(|feature| feature.id)
        .collect();
    for (name, source, placement) in composed {
        assert!(expected.insert(name), "{name} is mounted");
        let entry = features
            .iter()
            .find(|entry| entry["name"] == name)
            .unwrap_or_else(|| panic!("feature map omits {name}"));
        assert_eq!(
            entry["path"],
            format!("rust/beryl3d/crates/game_hud/src/{source}")
        );
        assert_eq!(entry["detail"], placement);
    }
    assert_eq!(names(&features), expected);
}

#[test]
fn map_names_the_profile_differences_and_declared_shortcuts() {
    let containers = inventory("containers");
    let detail = |name: &str| -> &str {
        containers
            .iter()
            .find(|entry| entry["name"] == name)
            .unwrap()["detail"]
            .as_str()
            .unwrap()
    };
    assert_eq!(detail("desktop exile.yours"), "zone pile  pile");
    assert_eq!(
        detail("phone exile.yours"),
        "hidden beneath graveyard  pile"
    );
    assert_eq!(detail("desktop stack"), "anchored stack  cascade");
    assert_eq!(detail("phone stack"), "centred stack  cascade");
    let features = inventory("features");
    let bug_report = features
        .iter()
        .find(|entry| entry["name"] == "bug-report")
        .unwrap();
    assert!(
        bug_report["detail"]
            .as_str()
            .unwrap()
            .contains("Cmd+Shift+B")
    );
    let profile_output = output(&["map", "profiles"]);
    assert!(profile_output.contains("settings.sheet"));
    assert!(profile_output.contains("393x759@"));
    let effect_output = output(&["map", "effects"]);
    assert!(effect_output.contains("glow"));
    assert!(effect_output.contains("surface"));
}

#[test]
fn map_finds_the_prompt_tray_and_shared_effect_timing() {
    let containers = inventory("containers");
    for (name, boxes) in [("desktop tray", "side-by-side"), ("phone tray", "stacked")] {
        let tray = containers
            .iter()
            .find(|entry| entry["name"] == name)
            .unwrap();
        assert_eq!(
            tray["path"],
            "rust/beryl3d/crates/game_table/src/table/tray_layout.rs"
        );
        assert_eq!(
            tray["detail"],
            format!(
                "prompt tray with {boxes} boxes  ScreenFit (rust/beryl3d/crates/kit_space/src/arrange/screen.rs)"
            )
        );
    }
    let data = output(&["map", "data", "--full"]);
    assert!(data.lines().any(|line| {
        line.contains("Shared verb and effect timing") && line.ends_with("data/ui-conventions.json")
    }));
}

#[test]
fn map_rejects_unknown_kinds_and_documents_its_options() {
    let unknown = run(&["map", "featres"]);
    assert_eq!(unknown.code, 2);
    assert!(unknown.stdout.is_empty());
    assert!(
        unknown.stderr.contains("Nearest: features"),
        "{}",
        unknown.stderr
    );
    for name in names(json(&["map", "--json"])["kinds"].as_array().unwrap()) {
        assert!(unknown.stderr.contains(name), "{}", unknown.stderr);
    }
    let help = output(&["map", "--help"]);
    assert!(help.contains("--full"));
    assert!(help.contains("--json"));
    assert_eq!(
        help.lines()
            .filter(|line| line.starts_with("  just b3d map"))
            .count(),
        2
    );
}
