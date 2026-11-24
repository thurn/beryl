//! The app run as an agent runs it: a fixture replayed with no window for three seconds exits
//! with status 0, prints the frame log's summary and names a log that reads back. And the
//! options a release build lacks are not options without `tools`, and read as given with it;
//! `--replay` names a file that exists.

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use app_client::{Client, Config, Input};
use beryl_client::frame_log;
use beryl_client::options::Options;
use beryl_inspect::data::{Snapshot, TUNING_FILES};
use kit_base::data::profiles;

#[test]
fn a_fixture_with_no_window_runs_for_its_time_and_logs_its_frames() {
    let output = Command::new(env!("CARGO_BIN_EXE_beryl"))
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
    assert!(stdout.contains(beryl_client::app::FIRST_SHOWN), "{stdout}");
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
fn a_watched_app_probes_with_checkout_tuning_and_reloads_when_paused() {
    let directory = copy_tuning("app");
    fs::write(
        directory.join("tier-probe.json"),
        r#"{"frames":1,"share":0.3333}"#,
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_beryl"))
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
            "automatic",
        ])
        .env("BERYL_CLIENT_DATA", &directory)
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
        if text[start..].starts_with(beryl_client::app::FIRST_SHOWN) {
            let verbs = OpenOptions::new()
                .write(true)
                .open(directory.join("verbs.json"))
                .unwrap();
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
    let tier = log.tier.unwrap();
    assert_eq!(tier.setting, "automatic");
    assert_eq!(tier.probe_frames, Some(1));
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

fn copy_tuning(name: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("client-watch-{name}-{}", std::process::id()));
    let source = Path::new("data/client");
    let effects = source.join("effects");
    let effect_names = beryl_test_files::read_dir(&effects)
        .unwrap()
        .map(|entry| Path::new("effects").join(entry.unwrap().file_name()));
    for name in TUNING_FILES
        .into_iter()
        .chain(profiles::FILES)
        .map(std::path::PathBuf::from)
        .chain(effect_names)
    {
        let target = directory.join(&name);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, beryl_test_files::read(source.join(name)).unwrap()).unwrap();
    }
    directory
}

#[test]
fn reload_detects_equal_length_and_timestamp_edits_and_keeps_the_last_valid_tuning() {
    let directory = copy_tuning("snapshot");
    let tuning = directory.join("tier-probe.json");
    let before = r#"{"frames":1,"share":0.3333}"#;
    let after = r#"{"frames":2,"share":0.3333}"#;
    fs::write(&tuning, before).unwrap();

    let watched = directory.clone();
    let mut watch = beryl_host::DataWatch::new(move || Snapshot::read(&watched), Snapshot::parse);
    let mut client = Client::new(watch.poll().unwrap().unwrap(), Config::LARGEST);
    assert_eq!(client.data().tier_probe.frames, 1);

    let original = fs::metadata(&tuning).unwrap();
    fs::write(&tuning, after).unwrap();
    OpenOptions::new()
        .write(true)
        .open(&tuning)
        .unwrap()
        .set_modified(original.modified().unwrap())
        .unwrap();
    let edited = fs::metadata(&tuning).unwrap();
    assert_eq!(edited.len(), original.len());
    assert_eq!(edited.modified().unwrap(), original.modified().unwrap());
    client.handle(Input::Data(watch.poll().unwrap().unwrap()));
    assert_eq!(client.data().tier_probe.frames, 2);

    let effect = directory.join("effects/beam.json");
    let effect_bytes = fs::read(&effect).unwrap();
    fs::remove_file(&effect).unwrap();
    let error = watch.poll().unwrap().unwrap_err();
    assert!(
        error.replace('\\', "/").contains("effects/beam.json"),
        "{error}"
    );
    client.handle(Input::DataError(error));
    assert!(client.data().effects.get("beam").is_some());
    assert!(watch.poll().is_none());
    fs::write(&effect, effect_bytes).unwrap();
    client.handle(Input::Data(watch.poll().unwrap().unwrap()));
    assert!(client.data().effects.get("beam").is_some());

    fs::write(&tuning, "{").unwrap();
    let error = watch.poll().unwrap().unwrap_err();
    assert!(error.contains("tier-probe.json"), "{error}");
    client.handle(Input::DataError(error));
    assert_eq!(client.data().tier_probe.frames, 2);
    assert!(watch.poll().is_none());

    fs::remove_file(&tuning).unwrap();
    let error = watch.poll().unwrap().unwrap_err();
    assert!(error.contains("tier-probe.json"), "{error}");
    client.handle(Input::DataError(error));
    assert_eq!(client.data().tier_probe.frames, 2);
    assert!(watch.poll().is_none());

    fs::write(&tuning, before).unwrap();
    client.handle(Input::Data(watch.poll().unwrap().unwrap()));
    assert_eq!(client.data().tier_probe.frames, 1);
    assert!(watch.poll().is_none());
    fs::remove_dir_all(directory).unwrap();
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
    assert_eq!(played.opponent, beryl_client::options::OPPONENT);
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
    let missing = std::env::temp_dir().join("client-no-such-replay.json");
    let missing = missing.to_str().expect("a path");
    assert!(
        parse(&["--replay", missing], true).is_err_and(|error| error.contains("no such file")),
        "a missing replay file is a usage error"
    );
    let file = std::env::temp_dir().join(format!("client-replay-{}.json", std::process::id()));
    std::fs::write(&file, "{}").expect("a file");
    let path = file.to_str().expect("a path");
    let resumed = parse(&["--replay", path], true).expect("a debug build resumes a file");
    assert_eq!(resumed.replay.as_deref(), Some(file.as_path()));
    assert!(parse(&["--replay", path], false).is_err());
    assert!(parse(&["--replay", path, "--fixture", "verbs-zones"], true).is_err());
    let _ = std::fs::remove_file(&file);
}
