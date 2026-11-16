//! The app run as an agent runs it: a fixture replayed with no window for three seconds exits
//! with status 0, prints the frame log's summary and names a log that reads back. And the
//! options a release build lacks are not options without `tools`, and read as given with it.

use std::process::Command;

use beryl_3d::frame_log;
use beryl_3d::options::Options;

#[test]
fn a_fixture_with_no_window_runs_for_its_time_and_logs_its_frames() {
    let output = Command::new(env!("CARGO_BIN_EXE_beryl3d"))
        .args([
            "--fixture",
            "verbs-zones",
            "--window",
            "none",
            "--for",
            "3s",
        ])
        .output()
        .expect("the app starts");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "the app failed: {}\n{stdout}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains(beryl_3d::app::FIRST_SHOWN), "{stdout}");
    let path = stdout
        .lines()
        .find_map(|line| line.strip_prefix("frame log: "))
        .unwrap_or_else(|| panic!("no frame log named in:\n{stdout}"));
    let log = frame_log::read(path.as_ref()).expect("the log reads back");
    let summary = log.summary.expect("the run wrote its summary");
    assert!(summary.frames > 0, "no frame was drawn");
    assert_eq!(log.run.window, "none");
    assert_eq!(log.run.fixture.as_deref(), Some("verbs-zones"));
    let _ = std::fs::remove_file(path);
}

#[test]
fn a_release_build_has_no_fixture_option() {
    let arguments = |words: &[&str]| {
        words
            .iter()
            .map(|word| (*word).to_owned())
            .collect::<Vec<_>>()
    };
    let without = Options::parse(&arguments(&["--fixture", "verbs-zones"]), false);
    assert!(without.is_err_and(|error| error.contains("--fixture is not an option")));
    let paused = Options::parse(&arguments(&["--pause-at", "1500ms"]), false);
    assert!(paused.is_err());
    let with = Options::parse(
        &arguments(&["--fixture", "verbs-zones", "--for", "3s"]),
        true,
    )
    .expect("a debug build replays");
    assert_eq!(with.fixture.as_deref(), Some("verbs-zones"));
    let agent = |words: &[&str], tools| Options::parse(&arguments(words), tools);
    assert!(agent(&["--agent", "expert"], false).is_err());
    let played = agent(
        &[
            "--agent",
            "expert",
            "--seed",
            "7",
            "--search-class",
            "default",
        ],
        true,
    )
    .expect("a debug build plays a game for the viewer");
    assert_eq!(played.agent.as_deref(), Some("expert"));
    assert_eq!(played.opponent, beryl_3d::options::OPPONENT);
    assert_eq!(played.seed, 7);
    assert_eq!(played.search_class, beryl_host::Class::Default);
    let plain = agent(&["--agent", "expert"], true).expect("a game for the viewer");
    assert_eq!(
        plain.search_class,
        beryl_host::Class::Default,
        "the AI searches in the web app's class unless a run compares another"
    );
    assert!(agent(&["--search-class", "idle"], true).is_err());
    let offered = agent(&["--offer", "expert", "--offer", "random"], true)
        .expect("a debug build offers retired agents");
    assert_eq!(offered.offer, ["expert", "random"]);
    assert!(agent(&["--offer", "expert"], false).is_err());
    assert!(agent(&["--offer", "nobody"], true).is_err_and(|error| error.contains("nobody")));
}
