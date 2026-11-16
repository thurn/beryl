//! `just b3d test` and the interface test runner behind it (docs/beryl-3d.md, "Test speed").

use beryl_inspect::cli::{Ran, main};
use beryl_inspect::faults::{Listed, PATH_VARIABLE};
use beryl_inspect::runner::{self, Limits, Outcome, Test};
use beryl_protocol::protocol::Side;
use beryl_scene::input::Size;
use serde_json::Value;

fn run(line: &[&str]) -> Ran {
    let args: Vec<String> = line.iter().map(|&arg| arg.to_owned()).collect();
    main(&args)
}

#[test]
fn one_test_runs_and_closes_with_one_line() {
    let ran = run(&["test", "b3d:verbs-zones/you/1600x900"]);
    assert_eq!(ran.code, 0, "{}{}", ran.stdout, ran.stderr);
    let mut lines = ran.stdout.lines();
    assert_eq!(
        lines.next(),
        Some("just b3d test b3d:verbs-zones/you/1600x900")
    );
    let rest: Vec<&str> = lines.collect();
    let closing = rest[rest.len() - 2];
    assert!(closing.starts_with("1 test, 0 failed"), "{closing}");
    assert!(
        rest[rest.len() - 1].contains("a listed fault"),
        "{}",
        rest[rest.len() - 1]
    );

    let ran = run(&["test", "verbs-zones/you/1600x900", "--json"]);
    let value: Value = serde_json::from_str(&ran.stdout).expect("JSON");
    assert_eq!(value["tests"], 1);
    assert!(value["frames"].as_u64().unwrap_or(0) > 0, "{value}");
}

/// Every test of a recording is in the window matrix from each seat, and each test's name,
/// given to `tree`, shows that test's recording from its seat in its window.
#[test]
fn a_tests_name_opens_its_recording_in_tree() {
    let tests = runner::tests("verbs-zones/").expect("the pack opens");
    let names: Vec<String> = tests.iter().map(Test::name).collect();
    assert_eq!(
        names,
        [
            "b3d:verbs-zones/you/1440x900",
            "b3d:verbs-zones/you/1600x900",
            "b3d:verbs-zones/you/1920x1080",
            "b3d:verbs-zones/you/2560x1080",
            "b3d:verbs-zones/opponent/1440x900",
            "b3d:verbs-zones/opponent/1600x900",
            "b3d:verbs-zones/opponent/1920x1080",
            "b3d:verbs-zones/opponent/2560x1080",
        ]
    );
    let ran = run(&["tree", "b3d:verbs-zones/opponent/2560x1080", "--at", "0ms"]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let command = ran.stdout.lines().next().unwrap_or_default();
    assert!(
        command.starts_with("just b3d tree verbs-zones --seat opponent --size 2560x1080"),
        "{command}"
    );
}

#[test]
fn a_prefix_no_test_has_exits_2_and_names_the_nearest_recordings() {
    let ran = run(&["test", "b3d:verbs-zonez"]);
    assert_eq!(ran.code, 2, "{}", ran.stdout);
    assert!(ran.stderr.contains("verbs-zones"), "{}", ran.stderr);
    assert!(
        ran.stderr
            .contains("b3d:<recording>/<seat>/<width>x<height>"),
        "{}",
        ran.stderr
    );
}

#[test]
fn a_panic_in_a_test_is_its_error() {
    let caught = runner::caught(|| -> u32 { panic!("an object has no slot") });
    assert_eq!(
        caught,
        Err("the client panicked: an object has no slot".to_owned())
    );
    let each = runner::each(&[1, 2, 3], |&n| runner::caught(|| 10 / (n - 2)));
    assert_eq!(each[0], Ok(-10));
    assert!(each[1].is_err());
    assert_eq!(each[2], Ok(10));
}

fn outcome(recording: &str, frames: usize) -> Outcome {
    Outcome {
        test: Test {
            recording: recording.to_owned(),
            seat: Side::You,
            size: Size {
                width: 1600.0,
                height: 900.0,
            },
        },
        faults: Vec::new(),
        frames,
        judged: beryl_scene::agent::Judged::default(),
        error: None,
        seconds: 0.0,
    }
}

#[test]
fn the_frame_limits_name_what_is_over_them() {
    let limits = Limits {
        median_frames: 20,
        most_frames: 50,
    };
    let outcomes = [outcome("a", 10), outcome("b", 30), outcome("c", 60)];
    let over = limits.over(&outcomes, true);
    assert_eq!(over.len(), 2, "{over:?}");
    assert!(
        over[0].contains("median test evaluates 30 frames"),
        "{over:?}"
    );
    assert!(
        over[1].starts_with("b3d:c/you/1600x900 evaluates 60 frames"),
        "{over:?}"
    );
    // A prefix's tests are held to the most, not the median.
    assert_eq!(limits.over(&outcomes, false).len(), 1);
    assert!(limits.over(&outcomes[..1], true).is_empty());
}

/// The verdict the run log gets for `name` when `b3d test` runs it against an allowance of
/// `allowed` `rule` faults (none when `allowed` is `None`), and the exit status.
fn logged_verdict(name: &str, rule: &str, allowed: Option<usize>, label: &str) -> (String, i32) {
    let dir = std::env::temp_dir().join(format!("beryl-results-{}-{label}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a directory");
    let list = dir.join("test-faults.json");
    let tests = allowed.map_or_else(
        || serde_json::json!({}),
        |n| serde_json::json!({ name: { rule: n } }),
    );
    std::fs::write(&list, serde_json::json!({ "tests": tests }).to_string()).expect("a list");
    let results = dir.join("results.jsonl");
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_b3d"))
        .args(["test", name])
        .env(PATH_VARIABLE, &list)
        .env("BERYL_CHECK_RESULTS", &results)
        .output()
        .expect("b3d runs")
        .status
        .code()
        .unwrap_or(-1);
    let text = std::fs::read_to_string(&results).expect("the run log");
    let _ = std::fs::remove_dir_all(&dir);
    let lines: Vec<Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).expect("JSON"))
        .collect();
    assert_eq!(lines.len(), 1, "{text}");
    assert_eq!(lines[0]["check"], name);
    (
        lines[0]["outcome"].as_str().unwrap_or_default().to_owned(),
        status,
    )
}

/// A test with faults the real list allows, and how many of one rule.
fn a_test_with_listed_faults() -> (String, String, usize) {
    let listed = Listed::read().expect("the list");
    let (name, counts) = listed
        .tests
        .iter()
        .find(|(name, counts)| {
            name.contains("/you/") && counts.len() == 1 && counts.values().all(|&n| n > 1)
        })
        .expect("a listed test");
    let (rule, &n) = counts.iter().next().expect("a rule");
    (name.clone(), rule.clone(), n)
}

/// The run log's verdict is the one the exit status gives: faults that match their allowance
/// pass, and a new, excess or stale count fails the test that has it.
#[test]
fn the_run_log_agrees_with_the_allowance_comparison() {
    let (name, rule, n) = a_test_with_listed_faults();
    assert_eq!(
        logged_verdict(&name, &rule, Some(n), "same"),
        ("pass".into(), 0)
    );
    assert_eq!(
        logged_verdict(&name, &rule, None, "new"),
        ("fail".into(), 1)
    );
    assert_eq!(
        logged_verdict(&name, &rule, Some(n - 1), "excess"),
        ("fail".into(), 1)
    );
    assert_eq!(
        logged_verdict(&name, &rule, Some(n + 1), "stale"),
        ("fail".into(), 1)
    );
}
