//! A bug report's `fixture.json` opened as a recording (`beryl_inspect::fixture_file`): the
//! report holds no card records, so they come from the card store, and `moment` finds the frame
//! whose tree is the report's `tree.txt`, as `just client bug` draws the reported moment.

use std::path::PathBuf;

use beryl_inspect::cli::{Ran, main};
use beryl_protocol::ipc::{CARD_RECORDS, FixtureMessage};
use serde_json::{Value, json};

fn run(line: &[&str]) -> Ran {
    let args: Vec<String> = line.iter().map(|&arg| arg.to_owned()).collect();
    let ran = main(&args);
    assert_eq!(
        ran.code, 0,
        "{line:?} failed:\n{}{}",
        ran.stdout, ran.stderr
    );
    ran
}

/// A report-shaped fixture made from a recording: its views and answers, without the card
/// records, in `{"messages": [...]}`, in a folder of its own.
fn report_fixture(name: &str) -> (PathBuf, String) {
    let folder = std::env::temp_dir().join(format!("beryl-report-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let recorded =
        beryl_inspect::recordings::find("pay-default", beryl_protocol::Seat::P0).unwrap();
    let messages: Vec<&FixtureMessage> = recorded
        .iter()
        .filter(|message| {
            !matches!(message, FixtureMessage::Response { command, .. } if command == CARD_RECORDS)
        })
        .collect();
    let path = folder.join("fixture.json");
    std::fs::write(&path, json!({ "messages": messages }).to_string()).unwrap();
    let text = path.to_string_lossy().into_owned();
    (folder, text)
}

#[test]
fn a_reports_tree_finds_its_frame_and_the_story_carries_card_records() {
    let (folder, fixture) = report_fixture("moment");
    let printed = run(&["tree", &fixture, "--at", "end"]).stdout;
    // The first two lines are the command line and the position, which `tree.txt` lacks.
    let body: Vec<&str> = printed.lines().skip(2).collect();
    let tree = folder.join("tree.txt");
    std::fs::write(&tree, body.join("\n")).unwrap();

    let found = run(&[
        "moment",
        &fixture,
        "--like",
        &tree.to_string_lossy(),
        "--json",
    ])
    .stdout;
    let found: Value = serde_json::from_str(found.trim()).unwrap();
    assert_eq!(found["distance"], 0, "{found}");

    let story = folder.join("report.story");
    run(&[
        "story",
        "from",
        &fixture,
        "--at",
        "end",
        "--out",
        &story.to_string_lossy(),
    ]);
    let text = std::fs::read_to_string(&story).unwrap();
    assert!(
        text.contains(r#""command":"card_records""#),
        "the story holds the card store's records"
    );
    let _ = std::fs::remove_dir_all(folder);
}

#[test]
fn a_reports_fixture_is_its_own_players_only() {
    let (folder, fixture) = report_fixture("seat");
    let args: Vec<String> = ["tree", &fixture, "--seat", "opponent"]
        .map(str::to_owned)
        .to_vec();
    let ran = main(&args);
    assert_eq!(ran.code, 2);
    assert!(ran.stderr.contains("use --seat you"), "{}", ran.stderr);
    let _ = std::fs::remove_dir_all(folder);
}
