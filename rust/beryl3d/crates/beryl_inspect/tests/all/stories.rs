use app_client::driver::Moment;
use app_client::stories::Source;
use beryl_inspect::{cli, data, pages};
use beryl_protocol::protocol::Side;
use kit_probe::agent::{TreeOptions, tree};
use kit_probe::story::Header;
use kit_ui::input::Size;

fn command(words: &[&str]) -> cli::Ran {
    let args: Vec<String> = words.iter().map(ToString::to_string).collect();
    let output = cli::main(&args);
    assert_eq!(output.code, 0, "{}", output.stderr);
    output
}

#[test]
fn a_story_is_listed_drawn_and_checked_as_a_scene() {
    let button = "story/kit_ui/button-states";
    let list = command(&["list", "story/"]);
    assert!(list.stdout.contains(button));
    assert!(list.stdout.contains("story/kit_space/forty-rectangles"));
    assert!(list.stdout.contains("story/kit_fx/glow"));

    let tree = command(&["tree", button]);
    assert!(tree.stdout.contains("--size 570x280"));
    assert!(
        tree.stdout.contains("button \"Ready\" #1"),
        "{}",
        tree.stdout
    );
    let hovered = command(&["tree", button, "--do", "hover button #1"]);
    assert!(
        hovered.stdout.contains("160x48 interactive hovered"),
        "{}",
        hovered.stdout
    );

    let check = command(&["check", button, "--json"]);
    let checked: serde_json::Value = serde_json::from_str(&check.stdout).expect("check JSON");
    assert_eq!(checked["frames"], 1);
    assert_eq!(checked["faults"], serde_json::json!([]));
}

#[test]
fn migrated_replays_are_listed_and_draw_the_same_frames_under_both_names() {
    let list = command(&["list", "story/"]);
    for legacy in [
        "stress-255",
        "stress-hand",
        "stress-stack",
        "screen/table",
        "screen/testing",
        "screen/gallery",
    ] {
        let (crate_name, name) = match legacy {
            "screen/table" => ("game_manual", "table"),
            "screen/testing" => ("app_screens", "testing"),
            "screen/gallery" => ("app_screens", "gallery"),
            board => ("game_table", board),
        };
        let story = format!("story/{crate_name}/{name}");
        assert!(list.stdout.contains(&story));
        assert_eq!(
            tree_body(&[legacy, "--at", "end"]),
            tree_body(&[&story, "--at", "end"]),
            "{story}"
        );
    }
}

#[test]
fn test_lists_and_reopens_each_story_in_its_profile_windows() {
    let sizes = ["1440x900", "1600x900@2", "1920x1080", "2560x1080"];
    let list = command(&["test", "story/", "--list"]);
    for size in sizes {
        assert!(
            list.stdout
                .contains(&format!("b3d:story/kit_ui/button-states/you/{size}"))
        );
    }

    let compact_list = command(&["test", "story/", "--list", "--phone-only"]);
    assert!(!compact_list.stdout.contains("button-states"));
    let compact_test = "b3d:story/kit_ui/phone-actions/you/375x647";
    assert!(list.stdout.lines().any(|name| name == compact_test));
    assert!(compact_list.stdout.lines().any(|name| name == compact_test));
    let tree = command(&["tree", compact_test]);
    assert!(tree.stdout.contains("--layout phone --size 375x647"));
    assert!(tree.stdout.contains("button \"Continue\" #1"));
}

fn capture(source: &[&str], feature: Option<&str>) -> String {
    let mut args = vec!["story", "from", "--json"];
    args.extend_from_slice(source);
    if let Some(id) = feature {
        args.extend(["--feature", id]);
    }
    let written = command(&args);
    let value: serde_json::Value = serde_json::from_str(&written.stdout).unwrap();
    value["path"].as_str().unwrap().to_owned()
}

fn tree_body(args: &[&str]) -> String {
    let mut command_line = vec!["tree", "--full"];
    command_line.extend_from_slice(args);
    command(&command_line)
        .stdout
        .lines()
        .skip(2)
        .collect::<Vec<_>>()
        .join("\n")
}

fn round_trip(source: &[&str]) {
    let path = capture(source, None);
    let expected = tree_body(source);
    let actual = tree_body(&[&path]);
    std::fs::remove_file(path).unwrap();
    // The capture uses the same driver and protocol ids, so it preserves even the ids.
    assert!(
        actual == expected,
        "tree changed for {source:?}\nexpected:\n{expected}\nactual:\n{actual}"
    );
}

#[test]
fn a_written_story_replays_a_moment_of_every_fixture() {
    let manifest: serde_json::Value =
        serde_json::from_slice(&beryl_test_files::read("ui/fixtures.json").unwrap()).unwrap();
    let fixtures = manifest["fixtures"].as_array().unwrap();
    assert!(
        fixtures
            .iter()
            .any(|fixture| fixture["name"] == "verbs-zones")
    );
    assert!(
        fixtures
            .iter()
            .any(|fixture| fixture["name"] == "pay-default")
    );
    let recordings = beryl_inspect::recordings::names().unwrap();
    for fixture in fixtures {
        let name = fixture["name"].as_str().unwrap();
        let (_, seat) = recordings
            .iter()
            .find(|(recording, _)| recording == name)
            .unwrap_or_else(|| panic!("no recording for fixture {name}"));
        let seat = cli::options::seat_word(cli::options::side_of(*seat));
        round_trip(&[name, "--at", "end", "--seat", seat]);
    }
}

#[test]
fn a_written_story_preserves_motion_and_the_selected_seat() {
    round_trip(&[
        "verbs-zones",
        "--at",
        "500ms",
        "--seat",
        "opponent",
        "--no-catch-up",
    ]);
    round_trip(&["stress-stack", "--at", "end"]);
    round_trip(&["verbs-zones", "--at", "end", "--profile", "wide-compact"]);
}

#[test]
fn screen_captures_replay_their_saved_story_catalog() {
    let window = Size {
        width: 1600.0,
        height: 900.0,
    };
    let header = Header {
        size_class: data::checkout()
            .unwrap()
            .profiles
            .for_size(window)
            .size_class,
        window,
    };
    let saved = Source::owned(
        "story/kit_ui/captured-catalog-example".to_owned(),
        "story element 600x400 desktop\nbutton \"Saved\" #1 24,24 160x48 interactive | widget=button\n"
            .to_owned(),
        Some("captured-controls".to_owned()),
    );
    for screen in ["screen/gallery", "screen/testing"] {
        let source = [screen, "--at", "end"];
        let path = capture(&source, None);
        let checkout_tree = tree_body(&source);
        assert_eq!(tree_body(&[&path]), checkout_tree);
        assert!(!checkout_tree.contains(saved.name.as_ref()));

        // Give the driver a catalog that differs from the inspector's checkout.
        for sources in [vec![saved.clone()], Vec::new()] {
            let mut page = pages::page(screen, Side::You, header.size_class, header.window)
                .unwrap()
                .with_stories(sources.clone().into());
            page.seek(&"end".parse::<Moment>().unwrap()).unwrap();
            let expected = tree(
                page.frame(),
                &TreeOptions {
                    full: true,
                    ..TreeOptions::default()
                },
            )
            .unwrap();
            assert_eq!(expected.contains(saved.name.as_ref()), !sources.is_empty());
            assert_ne!(expected.trim_end(), checkout_tree);

            let text = page.capture().write(header, None, None).unwrap();
            std::fs::write(&path, text).unwrap();
            assert_eq!(tree_body(&[&path]), expected.trim_end(), "{screen}");
        }
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn a_settings_feature_story_draws_the_sheet_on_its_own() {
    let path = capture(&["screen/settings", "--at", "end"], Some("settings"));
    let source = std::fs::read_to_string(&path).unwrap();
    assert!(source.contains("\nfeature settings\n"));
    assert!(source.contains("\nscreen start\n"));
    let tree = tree_body(&[&path]);
    std::fs::remove_file(path).unwrap();
    assert!(tree.contains("\"settings panel\""));
    assert!(tree.contains("\"Mute\""));
    assert!(!tree.contains("\"Quick Seer\""));
}

#[test]
fn an_authored_feature_story_uses_its_state_and_controls() {
    let tree = tree_body(&["story/game_probe/settings-confirmation"]);
    assert!(tree.contains("\"Confirm Concede\""));
    assert!(tree.contains("\"Unmute\""));
}
