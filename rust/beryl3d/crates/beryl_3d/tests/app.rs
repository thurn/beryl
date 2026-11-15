//! The app run as an agent runs it: a fixture replayed with no window for three seconds exits
//! with status 0, prints the frame log's summary and names a log that reads back. And the
//! options a release build lacks are not options without `tools`.

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
}
