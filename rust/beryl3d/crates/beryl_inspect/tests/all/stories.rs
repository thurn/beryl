use beryl_inspect::cli;

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
fn test_lists_and_reopens_each_story_in_its_profile_windows() {
    let sizes = ["1440x900", "1600x900@2", "1920x1080", "2560x1080"];
    let list = command(&["test", "story/", "--list"]);
    for size in sizes {
        assert!(
            list.stdout
                .contains(&format!("b3d:story/kit_ui/button-states/you/{size}"))
        );
    }

    let phone_list = command(&["test", "story/", "--list", "--phone-only"]);
    assert!(!phone_list.stdout.contains("button-states"));
    let phone = "b3d:story/kit_ui/phone-actions/you/375x647";
    assert!(list.stdout.lines().any(|name| name == phone));
    assert!(phone_list.stdout.lines().any(|name| name == phone));
    let tree = command(&["tree", phone]);
    assert!(tree.stdout.contains("--layout phone --size 375x647"));
    assert!(tree.stdout.contains("button \"Continue\" #1"));
}
