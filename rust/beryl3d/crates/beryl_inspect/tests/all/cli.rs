//! The text commands of `just b3d` (docs/beryl-3d.md, "The agent interface"), run as the `b3d`
//! program runs them: every output can be pasted into the next command, and a wrong command
//! line exits with status 2 and names what would work.

use beryl_inspect::cli::line::Line;
use beryl_inspect::cli::{Ran, main};
use beryl_inspect::pages;
use beryl_protocol::protocol::Side;
use beryl_scene::agent::Layout;
use beryl_scene::driver::Moment;
use beryl_scene::input::Size;
use serde_json::Value;

const RECORDING: &str = "verbs-zones";
/// A recording with no fault from either seat, for `check`, which exits with 1 on a fault.
const CLEAN: &str = "pay-default";

fn run(line: &[&str]) -> Ran {
    let args: Vec<String> = line.iter().map(|&arg| arg.to_owned()).collect();
    main(&args)
}

/// Runs a command line that must succeed, and returns what it printed.
fn ok(line: &[&str]) -> String {
    let ran = run(line);
    assert_eq!(
        ran.code, 0,
        "{line:?} failed:\n{}{}",
        ran.stdout, ran.stderr
    );
    ran.stdout
}

/// Runs a command line that must be refused, and returns its message.
fn refused(line: &[&str]) -> String {
    let ran = run(line);
    assert_eq!(ran.code, 2, "{line:?} was not refused:\n{}", ran.stdout);
    assert!(ran.stdout.is_empty(), "{}", ran.stdout);
    ran.stderr
}

#[test]
fn every_tree_line_after_the_header_follows_the_grammar() {
    for at in ["0ms", "1500ms", "end"] {
        let printed = ok(&["tree", RECORDING, "--at", at, "--full"]);
        let mut lines = printed.lines();
        let command = lines.next().unwrap();
        assert!(
            command.starts_with("just b3d tree verbs-zones --seat you"),
            "{command}"
        );
        let header = lines.next().unwrap();
        assert!(header.starts_with("view "), "{header}");
        let mut elements = 0;
        for line in lines {
            match Line::parse(line) {
                Ok(Line::Element { .. }) => elements += 1,
                Ok(_) => {}
                Err(why) => panic!("{why}"),
            }
        }
        assert!(elements > 0, "{printed}");
    }
    // A cut place prints a cut line, which the grammar reads too.
    let cut = ok(&["tree", RECORDING, "--at", "1500ms"]);
    assert!(
        cut.lines()
            .skip(2)
            .any(|line| matches!(Line::parse(line), Ok(Line::Cut { .. }))),
        "{cut}"
    );
}

#[test]
fn every_moment_info_prints_is_accepted_by_at() {
    let printed = ok(&["info", RECORDING, "--full"]);
    let moments: Vec<&str> = printed
        .lines()
        .skip(2)
        .take_while(|line| !line.starts_with("Each line"))
        .map(|line| line.split_whitespace().next().unwrap())
        .collect();
    assert!(moments.iter().any(|m| m.starts_with("step:")), "{printed}");
    assert!(
        moments.iter().any(|m| m.starts_with("prompt:")),
        "{printed}"
    );
    let size = Size {
        width: 1600.0,
        height: 900.0,
    };
    let mut page = pages::page(RECORDING, Side::You, Layout::Desktop, size).unwrap();
    let end = page.moments().last().unwrap().1.0;
    for moment in &moments {
        let parsed: Moment = moment.parse().unwrap_or_else(|e| panic!("{moment}: {e}"));
        let time = page
            .time_of(&parsed)
            .unwrap_or_else(|e| panic!("{moment}: {e}"));
        assert!(time <= end, "{moment} lies past the end");
    }
    // And through the command itself, for one of each kind.
    for kind in ["start", "view:", "step:", "prompt:", "end"] {
        let moment = moments.iter().find(|m| m.starts_with(kind)).unwrap();
        ok(&["tree", RECORDING, "--at", moment]);
    }
}

#[test]
fn an_unknown_recording_exits_2_and_lists_the_nearest_names() {
    let message = refused(&["tree", "verbs-zone", "--at", "1500ms"]);
    assert!(
        message.contains("no recording is named 'verbs-zone'"),
        "{message}"
    );
    let listed: Vec<&str> = message.lines().filter(|l| l.starts_with("  ")).collect();
    assert_eq!(listed.len(), 5, "{message}");
    assert_eq!(listed[0].trim(), "verbs-zones", "{message}");
}

#[test]
fn wrong_command_lines_exit_2_and_name_what_would_work() {
    let past = refused(&["tree", RECORDING, "--at", "99s"]);
    assert!(
        past.contains("past the end") && past.contains("just b3d info"),
        "{past}"
    );
    let two = refused(&[
        "tree",
        RECORDING,
        "--at",
        "1500ms",
        "--do",
        "hover card \"Plains\"",
    ]);
    assert!(
        two.contains("elements match") && two.contains("Narrow it with an #id"),
        "{two}"
    );
    let command = refused(&["trea", RECORDING]);
    assert!(command.contains("the nearest are tree"), "{command}");
    let duration = refused(&["tree", RECORDING, "--at", "15x0ms"]);
    assert!(duration.contains("as in 850ms or 1.25s"), "{duration}");
    let option = refused(&["tree", RECORDING, "--whithin", "hand.yours"]);
    assert!(option.contains("--within"), "{option}");
}

#[test]
fn a_diff_of_a_moment_with_itself_prints_nothing() {
    let printed = ok(&["diff", RECORDING, "--from", "1500ms", "--to", "1500ms"]);
    assert_eq!(printed.lines().count(), 1, "{printed}");
    let moved = ok(&["diff", RECORDING, "--from", "800ms", "--to", "1500ms"]);
    assert!(moved.lines().count() > 1, "{moved}");
    for line in moved.lines().skip(1) {
        let element = line.rsplit_once("  (").unwrap().0;
        assert!(
            matches!(Line::parse(element), Ok(Line::Element { .. })),
            "{line}"
        );
    }
}

#[test]
fn records_to_a_later_moment_print_one_line_a_frame_60_to_the_second() {
    let printed = ok(&[
        "tree",
        RECORDING,
        "--at",
        "1400ms",
        "--to",
        "1450ms",
        "--records",
    ]);
    let frames: Vec<Value> = (printed.lines())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let times: Vec<f64> = (frames.iter())
        .map(|frame| frame["t_ms"].as_f64().unwrap())
        .collect();
    assert_eq!(times, [1400.0, 1416.667, 1433.333, 1450.0], "{printed}");
    for (at, frame) in frames.iter().enumerate() {
        assert_eq!(frame["frame"], at, "{frame}");
        assert!(!frame["elements"].as_array().unwrap().is_empty(), "{frame}");
    }
    let back = refused(&[
        "tree",
        RECORDING,
        "--at",
        "1450ms",
        "--to",
        "1400ms",
        "--records",
    ]);
    assert!(back.contains("before --at"), "{back}");
    let bare = refused(&["tree", RECORDING, "--at", "1400ms", "--to", "1450ms"]);
    assert!(bare.contains("give --records"), "{bare}");
}

#[test]
fn json_output_parses() {
    for line in [
        &["list", RECORDING][..],
        &["info", RECORDING],
        &["tree", RECORDING, "--at", "1500ms"],
        &["diff", RECORDING, "--from", "800ms", "--to", "1500ms"],
        &["check", CLEAN],
    ] {
        let mut line = line.to_vec();
        line.push("--json");
        let printed = ok(&line);
        let value: Value =
            serde_json::from_str(&printed).unwrap_or_else(|e| panic!("{line:?}: {e}\n{printed}"));
        assert!(
            value["command"].as_str().unwrap().contains("--json"),
            "{value}"
        );
    }
}

#[test]
fn hovering_a_tree_line_changes_that_elements_states() {
    let printed = ok(&["tree", RECORDING, "--at", "prompt:2", "--role", "button"]);
    let button = printed
        .lines()
        .find(|line| line.trim_start().starts_with("button "))
        .unwrap()
        .trim();
    assert!(!button.contains("hovered"), "{button}");
    let hover = format!("hover {button}");
    let after = ok(&[
        "tree", RECORDING, "--at", "prompt:2", "--role", "button", "--do", &hover,
    ]);
    let locator = button.split(" #").next().unwrap();
    let hovered = after
        .lines()
        .find(|line| line.trim_start().starts_with(locator))
        .unwrap();
    assert!(hovered.contains(" hovered"), "{after}");
    // The printed command carries the action, so it prints the same tree again.
    assert!(
        after
            .lines()
            .next()
            .unwrap()
            .contains(" --do 'hover button "),
        "{after}"
    );
}

#[test]
fn check_runs_every_tick_and_closes_with_one_line() {
    let printed = ok(&["check", CLEAN]);
    let last = printed.lines().last().unwrap();
    assert!(
        last.starts_with("1 test, ") && last.contains(" faults in "),
        "{printed}"
    );
    let refused = refused(&["check", RECORDING, "--do", "still"]);
    assert!(refused.contains("tree or diff"), "{refused}");
}

#[test]
fn every_time_info_prints_is_accepted_by_at() {
    // Times are printed rounded down, so even the last, the recording's end, lies within it.
    let printed = ok(&["info", RECORDING, "--full"]);
    let times: Vec<&str> = printed
        .lines()
        .skip(2)
        .take_while(|line| !line.starts_with("Each line"))
        .map(|line| line.split_whitespace().nth(1).unwrap())
        .collect();
    let end = printed
        .lines()
        .nth(1)
        .and_then(|summary| summary.rsplit_once("ends at "))
        .unwrap()
        .1;
    assert_eq!(times.last(), Some(&end), "{printed}");
    ok(&["tree", RECORDING, "--at", end]);
}

#[test]
fn info_names_the_card_each_moving_step_moves() {
    let printed = ok(&["info", RECORDING]);
    let named = printed
        .lines()
        .filter(|line| line.starts_with("step:") && line.contains(" card "))
        .count();
    assert!(named > 0, "{printed}");
}

#[test]
fn list_cuts_its_output_unless_full() {
    let cut = ok(&["list"]);
    assert!(cut.lines().count() <= 100, "{} lines", cut.lines().count());
    assert!(cut.contains("just b3d list --full"), "{cut}");
    let full = ok(&["list", "--full"]);
    assert!(full.lines().count() > 100, "{full}");
}

#[test]
fn a_misspelt_prefix_of_a_long_name_finds_that_name() {
    // Missing a letter of a long name's start finds that name, not the short names a few
    // letters away from the whole text.
    let names = [
        "converter-exiled",
        "table-piles",
        "winds-of-abandon/exiles-a-token-before-its-controller-searches",
    ];
    let near = beryl_inspect::cli::options::nearest("winds-of-abandn/exiles", &names);
    assert_eq!(near[0], names[2], "{near:?}");
}

#[test]
fn an_unknown_action_says_so_once() {
    let message = refused(&["tree", RECORDING, "--do", "hover:card"]);
    assert_eq!(message.matches("is not an action").count(), 1, "{message}");
    assert!(message.contains("The actions are press"), "{message}");
}

const TIMELINE_KEYS: [&str; 11] = [
    "recording",
    "seat",
    "view",
    "beat",
    "step",
    "verb",
    "case",
    "start",
    "end",
    "effects",
    "sounds",
];

/// The JSON lines `timeline` printed after its first line, the command that prints them again.
fn timeline_lines(printed: &str) -> Vec<&str> {
    printed.lines().skip(1).collect()
}

/// The session's steps print one line each, as `info` lists them. A line with `"own":true` is
/// a step the client made itself for a prompt, which `info` does not list.
#[test]
fn timeline_prints_one_line_for_each_step_info_lists() {
    let info = ok(&["info", RECORDING]);
    let steps: usize = info
        .lines()
        .nth(1)
        .and_then(|summary| summary.split(" prompts, ").nth(1))
        .and_then(|rest| rest.split(' ').next())
        .and_then(|count| count.parse().ok())
        .unwrap_or_else(|| panic!("{info}"));
    let printed = ok(&["timeline", RECORDING]);
    let session = timeline_lines(&printed)
        .into_iter()
        .filter(|line| !line.ends_with(",\"own\":true}"));
    assert_eq!(session.count(), steps, "{printed}");
}

#[test]
fn every_timeline_line_is_json_with_its_keys_in_order() {
    let printed = ok(&["timeline", RECORDING, "--seat", "opponent"]);
    assert!(
        printed.starts_with("just b3d timeline verbs-zones --seat opponent"),
        "{printed}"
    );
    for line in timeline_lines(&printed) {
        let value: Value = serde_json::from_str(line).unwrap_or_else(|e| panic!("{line}: {e}"));
        assert_eq!(value["seat"], "opponent", "{line}");
        // The line's keys are in the order given, which parsing into a map would hide.
        let mut at = 0;
        for key in TIMELINE_KEYS {
            let found = line[at..]
                .find(&format!("\"{key}\":"))
                .unwrap_or_else(|| panic!("no {key} after byte {at} of {line}"));
            at += found;
        }
        assert!(value["end"].as_f64() >= value["start"].as_f64(), "{line}");
        assert!(value["effects"].is_array(), "{line}");
        assert!(value["sounds"].is_array(), "{line}");
    }
}

#[test]
fn two_timeline_runs_print_identical_bytes() {
    assert_eq!(ok(&["timeline", RECORDING]), ok(&["timeline", RECORDING]),);
}

#[test]
fn export_writes_the_messages_find_gives_after_the_own_timings() {
    use beryl_protocol::ipc::{FixtureMessage, STAGE_TIMINGS};
    let directory = std::env::temp_dir().join(format!("b3d-export-{}", std::process::id()));
    let printed = ok(&["export", directory.to_str().unwrap()]);
    let file = directory.join(format!("{RECORDING}.you.json"));
    let read: Vec<FixtureMessage> = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    let found = beryl_inspect::recordings::find(RECORDING, beryl_protocol::Seat::P0).unwrap();
    // The web interface's timeline times the steps a prompt makes itself by this response.
    let timings = FixtureMessage::response(STAGE_TIMINGS, &beryl_protocol::script::own_timings());
    assert_eq!(read, [&[timings][..], &found].concat());
    assert!(printed.contains(" files"), "{printed}");
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn timeline_names_the_nearest_recordings_for_a_wrong_name() {
    let message = refused(&["timeline", "verbs-zone"]);
    assert!(message.contains("verbs-zones"), "{message}");
}
