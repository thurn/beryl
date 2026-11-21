//! The app run as an agent runs it: a fixture replayed with no window for three seconds exits
//! with status 0, prints the frame log's summary and names a log that reads back. And the
//! options a release build lacks are not options without `tools`, and read as given with it;
//! `--replay` names a file that exists.

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use beryl_3d::frame_log;
use beryl_3d::options::Options;
use kit_base::data::profiles;

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
fn a_paused_windowless_app_logs_one_reload_for_a_touched_data_file() {
    let directory = std::env::temp_dir().join(format!("b3d-app-watch-{}", std::process::id()));
    let source = Path::new("data/beryl3d");
    let effects = source.join("effects");
    let effect_names = beryl_test_files::read_dir(&effects)
        .unwrap()
        .map(|entry| Path::new("effects").join(entry.unwrap().file_name()));
    for name in beryl_host::TUNING_FILES
        .into_iter()
        .chain(profiles::FILES)
        .map(std::path::PathBuf::from)
        .chain(effect_names)
    {
        let target = directory.join(&name);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, beryl_test_files::read(source.join(name)).unwrap()).unwrap();
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_beryl3d"))
        .args([
            "--fixture",
            "verbs-zones",
            "--window",
            "none",
            "--for",
            "3000",
            "--watch",
            "--pause-at",
            "0ms",
            "--tier",
            "baseline",
        ])
        .env("BERYL3D_DATA", &directory)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the app starts");
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut text = String::new();
    loop {
        let start = text.len();
        let read = stdout.read_line(&mut text).unwrap();
        if read == 0 {
            break;
        }
        if text[start..].starts_with(beryl_3d::app::FIRST_SHOWN) {
            let verbs = File::open(directory.join("verbs.json")).unwrap();
            let modified = verbs.metadata().unwrap().modified().unwrap();
            verbs
                .set_modified(modified - Duration::from_secs(1))
                .unwrap();
            break;
        }
    }
    stdout.read_to_string(&mut text).unwrap();
    let result = child.wait_with_output().unwrap();
    fs::remove_dir_all(directory).unwrap();
    assert!(
        result.status.success(),
        "{}\n{text}",
        String::from_utf8_lossy(&result.stderr)
    );
    let path = text
        .lines()
        .find_map(|line| line.strip_prefix("frame log: "))
        .unwrap();
    let log = frame_log::read(path.as_ref()).unwrap();
    fs::remove_file(path).unwrap();
    let reloads: Vec<_> = log
        .inputs
        .into_iter()
        .filter(|input| {
            matches!(
                input,
                frame_log::InputLine::DataReloaded {}
                    | frame_log::InputLine::DataReloadFailed { .. }
            )
        })
        .collect();
    assert_eq!(reloads, [frame_log::InputLine::DataReloaded {}]);
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

#[test]
fn replay_resumes_only_a_file_that_exists() {
    let parse = |words: &[&str], tools| {
        let words: Vec<String> = words.iter().map(|word| (*word).to_owned()).collect();
        Options::parse(&words, tools)
    };
    let missing = std::env::temp_dir().join("b3d-no-such-replay.json");
    let missing = missing.to_str().expect("a path");
    assert!(
        parse(&["--replay", missing], true).is_err_and(|error| error.contains("no such file")),
        "a missing replay file is a usage error"
    );
    let file = std::env::temp_dir().join(format!("b3d-replay-{}.json", std::process::id()));
    std::fs::write(&file, "{}").expect("a file");
    let path = file.to_str().expect("a path");
    let resumed = parse(&["--replay", path], true).expect("a debug build resumes a file");
    assert_eq!(resumed.replay.as_deref(), Some(file.as_path()));
    assert!(parse(&["--replay", path], false).is_err());
    assert!(parse(&["--replay", path, "--fixture", "verbs-zones"], true).is_err());
    let _ = std::fs::remove_file(&file);
}
