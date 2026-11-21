//! `just b3d test` and the interface test runner behind it (docs/beryl-3d.md, "Test speed").

use app_client::driver::Page;
use beryl_inspect::cli::{Options, Ran, Spec, main};
use beryl_inspect::faults::PATH_VARIABLE;
use beryl_inspect::pages::{self, PHONE_SAFE_AREA, Windows};
use beryl_inspect::runner::{self, Limits, Outcome, Test};
use beryl_protocol::protocol::Side;
use kit_probe::agent::Layout;
use kit_ui::input::{Insets, Size};
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
    let judged = rest[rest.len() - 3];
    assert!(
        judged.starts_with("judged,") && judged.contains("on-screen "),
        "{judged}"
    );
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
    let tests = runner::tests("verbs-zones/", Windows::Desktop).expect("the pack opens");
    let names: Vec<String> = tests.iter().map(Test::name).collect();
    assert_eq!(
        names,
        [
            "b3d:verbs-zones/you/1440x900",
            "b3d:verbs-zones/you/1600x900@2",
            "b3d:verbs-zones/you/1920x1080",
            "b3d:verbs-zones/you/2560x1080",
            "b3d:verbs-zones/opponent/1440x900",
            "b3d:verbs-zones/opponent/1600x900@2",
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
    assert!(tests.iter().all(|test| test.layout() == Layout::Desktop));
}

/// The whole matrix is the four desktop windows, one of them at the Retina scale, and the three
/// phone windows, from each seat a recording is recorded for; the default run plays only the desktop ones and `--phone-only`
/// only the phone ones.
#[test]
fn the_window_matrix_has_seven_sizes_from_each_recorded_seat() {
    let names = |windows| -> Vec<String> {
        let tests = runner::tests("pay-default/", windows).expect("the pack opens");
        tests.iter().map(Test::name).collect()
    };
    let sizes = [
        "1440x900",
        "1600x900@2",
        "1920x1080",
        "2560x1080",
        "393x759",
        "375x647",
        "430x932",
    ];
    let every: Vec<String> = ["you", "opponent"]
        .iter()
        .flat_map(|seat| sizes.map(|size| format!("b3d:pay-default/{seat}/{size}")))
        .collect();
    assert_eq!(names(Windows::All), every);
    let phone = |name: &String| sizes[4..].iter().any(|size| name.ends_with(size));
    let desktop: Vec<String> = every.iter().filter(|n| !phone(n)).cloned().collect();
    let phones: Vec<String> = every.iter().filter(|n| phone(n)).cloned().collect();
    assert_eq!(names(Windows::Desktop), desktop);
    assert_eq!(names(Windows::Phone), phones);
}

/// A phone test plays in the phone layout with the fixed phone safe area, and `tree` and
/// `check`, given the test's name, open the same layout and insets: the client draws what the
/// test checked, and a fault is reported against the same safe area.
#[test]
fn a_phone_tests_name_opens_the_same_layout_and_insets_in_test_tree_and_check() {
    let name = "b3d:pay-default/you/375x647";
    let size = Size {
        width: 375.0,
        height: 647.0,
    };
    let test = Test {
        recording: "pay-default".to_owned(),
        seat: Side::You,
        size,
        scale: 1.0,
    };
    assert_eq!(test.name(), name);
    assert_eq!(test.layout(), Layout::Phone);
    let insets = pages::safe_area(test.layout());
    assert_eq!(insets, PHONE_SAFE_AREA);
    assert!(insets.top > 0.0 && insets.bottom > 0.0, "{insets:?}");
    for command in ["tree", "check"] {
        let spec = Spec {
            recording: true,
            ..Spec::default()
        };
        let options = Options::parse(command, &[name.to_owned()], spec).expect("a test's name");
        assert_eq!(
            (options.name(), options.seat, options.layout, options.size),
            ("pay-default", Side::You, Layout::Phone, size)
        );
    }
    // The insets reach the client: the test's page lays out differently from the same page with
    // none, and `tree` prints the test's page.
    let tested = test.page().expect("the page opens").tree();
    let recording = pages::recording("pay-default", Side::You).expect("the recording");
    let bare = Page::new(recording.into(), Side::You, Layout::Phone, size)
        .expect("the page opens")
        .tree();
    assert_ne!(tested, bare, "the safe area changed nothing on the phone");
    let inset = Page::new(
        pages::recording("pay-default", Side::You)
            .expect("the recording")
            .into(),
        Side::You,
        Layout::Phone,
        size,
    )
    .expect("the page opens")
    .with_safe_area(PHONE_SAFE_AREA)
    .tree();
    assert_eq!(tested, inset);
    let ran = run(&["tree", name]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    let command = ran.stdout.lines().next().unwrap_or_default();
    assert_eq!(
        command,
        "just b3d tree pay-default --seat you --layout phone --size 375x647"
    );
    // A desktop test's name still opens the desktop layout with no safe area.
    let options = Options::parse(
        "check",
        &["b3d:pay-default/you/1440x900".to_owned()],
        Spec {
            recording: true,
            ..Spec::default()
        },
    )
    .expect("a test's name");
    assert_eq!(options.layout, Layout::Desktop);
    assert_eq!(pages::safe_area(options.layout), Insets::default());
}

/// `--phone-only` plays only the phone windows, names each fault by the phone test that
/// reproduces it, and never writes the list of allowed faults.
#[test]
fn phone_only_plays_the_phone_windows_and_never_updates_the_list() {
    let ran = run(&["test", "pay-default/", "--phone-only", "--json"]);
    assert!(ran.code <= 1, "{}{}", ran.stdout, ran.stderr);
    let value: Value = serde_json::from_str(&ran.stdout).expect("JSON");
    assert_eq!(value["tests"], 6, "{value}");
    assert_eq!(
        value["command"],
        "just b3d test pay-default/ --phone-only --json"
    );
    for fault in value["faults"].as_array().expect("faults") {
        let test = fault["test"].as_str().unwrap_or_default();
        assert!(
            ["393x759", "375x647", "430x932"]
                .iter()
                .any(|size| test.ends_with(size)),
            "{fault}"
        );
    }
    let ran = run(&["test", "pay-default/", "--phone-only", "--update"]);
    assert_eq!(ran.code, 2, "{}", ran.stdout);
    assert!(ran.stderr.contains("--phone-only"), "{}", ran.stderr);
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
            scale: 1.0,
        },
        faults: Vec::new(),
        frames,
        judged: kit_probe::agent::Judged::default(),
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
        over[0].contains("median test of a recorded fixture evaluates 30 frames"),
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

#[test]
fn the_median_frame_limit_leaves_out_card_spec_moments() {
    // Card scenarios add spec moments without running Beryl 3D's lanes, so they must not move
    // the median that every Beryl 3D push is held to.
    let limits = Limits {
        median_frames: 20,
        most_frames: 500,
    };
    let fixtures = [outcome("a", 10), outcome("b", 20), outcome("c", 20)];
    let moments = (0..10).map(|n| outcome(&format!("card/moment-{n}"), 400));
    let outcomes: Vec<Outcome> = fixtures.into_iter().chain(moments).collect();
    assert_eq!(runner::fixture_median(&outcomes), Some(20));
    assert!(limits.over(&outcomes, true).is_empty());
}

/// The verdict the run log gets for `name` when `b3d test` runs it against its entry in the
/// list of allowed faults with `allowed` `rule` faults besides (none when `allowed` is `None`),
/// and the exit status.
fn logged_verdict(name: &str, rule: &str, allowed: Option<usize>, label: &str) -> (String, i32) {
    let dir = std::env::temp_dir().join(format!("beryl-results-{}-{label}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a directory");
    let list = dir.join("test-faults.json");
    let listed: Value = serde_json::from_str(
        &beryl_test_files::read_to_string("data/beryl3d/test-faults.json")
            .expect("the list of allowed faults"),
    )
    .expect("JSON");
    let mut entry = listed["tests"][name].clone();
    if !entry.is_object() {
        entry = serde_json::json!({});
    }
    if let Some(n) = allowed {
        entry[rule] = n.into();
    }
    let tests = serde_json::json!({ name: entry });
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

/// The run log's verdict is the one the exit status gives: a test whose faults match their
/// allowance passes, and a stale count fails it. The allowance starts from the test's entry in
/// the list of allowed faults, so a fault listed there today does not decide the verdict;
/// `faults.rs` covers the new and excess counts on made-up outcomes.
#[test]
fn the_run_log_agrees_with_the_allowance_comparison() {
    let name = "b3d:verbs-piles/you/1440x900";
    assert_eq!(
        logged_verdict(name, "step-shown", None, "same"),
        ("pass".into(), 0)
    );
    assert_eq!(
        logged_verdict(name, "step-shown", Some(1), "stale"),
        ("fail".into(), 1)
    );
}

/// The desktop matrix plays a window at the Retina scale, named with `@2` after its size, and
/// `tree` and `check`, given that name, open the page at the same scale: what the client lays
/// out and the faults it is checked for are those of the desktop app on a Retina Mac.
#[test]
fn a_retina_tests_name_opens_the_same_scale_in_test_tree_and_check() {
    let tests = runner::tests("pay-default/you/", Windows::Desktop).expect("the tests");
    let retina: Vec<&Test> = tests.iter().filter(|test| test.scale > 1.0).collect();
    assert_eq!(retina.len(), pages::RETINA_WINDOWS.len(), "{tests:?}");
    let test = retina[0];
    assert_eq!(test.scale.to_bits(), pages::RETINA_SCALE.to_bits());
    let name = test.name();
    assert_eq!(name, "b3d:pay-default/you/1600x900@2");
    let page = test.page().expect("the page opens");
    assert_eq!(page.scale().to_bits(), 2.0_f32.to_bits());
    for command in ["tree", "check"] {
        let spec = Spec {
            recording: true,
            ..Spec::default()
        };
        let options =
            Options::parse(command, std::slice::from_ref(&name), spec).expect("a test's name");
        assert_eq!(
            (options.name(), options.size, options.scale),
            ("pay-default", test.size, 2.0)
        );
    }
}
