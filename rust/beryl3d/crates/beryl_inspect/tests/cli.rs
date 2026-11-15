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
    let printed = ok(&["info", RECORDING]);
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
fn json_output_parses() {
    for line in [
        &["list", RECORDING][..],
        &["info", RECORDING],
        &["tree", RECORDING, "--at", "1500ms"],
        &["diff", RECORDING, "--from", "800ms", "--to", "1500ms"],
        &["check", RECORDING],
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
    assert!(hovered.ends_with(" hovered"), "{after}");
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
    let printed = ok(&["check", RECORDING]);
    let last = printed.lines().last().unwrap();
    assert!(
        last.starts_with("1 test, ") && last.contains(" faults in "),
        "{printed}"
    );
    let refused = refused(&["check", RECORDING, "--do", "still"]);
    assert!(refused.contains("tree or diff"), "{refused}");
}
