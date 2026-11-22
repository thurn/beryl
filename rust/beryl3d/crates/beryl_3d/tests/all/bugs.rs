//! Bug reports: an app replaying `pay-default` with no window, driven to press Cmd+B, take the
//! whole window, describe the bug and submit it, writes a report folder into `$BERYL_BUGS` with
//! the picture of the frozen frame, its element tree, the frame log, the latest views, the
//! recording's moment in `ui.json` and the files every report holds. With local filing enabled,
//! it also hands that report's ID to the filing command.

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};

use beryl_3d::bugs::SAVED;

/// The app, stopped when dropped.
struct App(Child);

impl Drop for App {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Submits two reports after the replay starts. The last line keeps the run going while
/// they are written, since a drive script ends the run after its last action.
const SCRIPT: &str = "\
at 1000ms key Cmd+B
at 1500ms key space
at 2000ms type field \"description\" \"the pay bar never filled\"
at 2500ms press button \"Submit\"
at 5000ms key Cmd+B
at 5500ms key space
at 6000ms type field \"description\" \"another report in this run\"
at 6500ms press button \"Submit\"
at 30000ms move 10,10
";

#[test]
fn a_report_saved_from_a_replay_holds_the_picture_the_tree_and_the_logs() {
    let temp = std::env::temp_dir().join(format!("b3d-bugs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp);
    std::fs::create_dir_all(&temp).expect("a temporary folder");
    let bugs = temp.join("bugs");
    let script = temp.join("report.drive");
    std::fs::write(&script, SCRIPT).expect("the script is written");
    let mut command = Command::new(env!("CARGO_BIN_EXE_beryl3d"));
    command.env_remove("BERYL_FILE_BUGS");
    #[cfg(unix)]
    stub_filing(&mut command, &temp);
    let mut child = command
        .args([
            "--fixture",
            "pay-default",
            "--window",
            "none",
            "--for",
            "60s",
        ])
        .arg("--drive")
        .arg(&script)
        .env("BERYL_BUGS", &bugs)
        // Under cargo the report asks git for the commit; a bundled app has no checkout to ask,
        // so the test runs the app as one and the report must hold the build's commit.
        .env_remove("CARGO_MANIFEST_DIR")
        .stdout(Stdio::piped())
        .spawn()
        .expect("the app starts");
    let stdout = child.stdout.take().expect("the app's output is a pipe");
    let _app = App(child);
    // Kept open to the end: the app dies when it prints to a closed pipe.
    let mut lines = BufReader::new(stdout).lines();
    let folder = lines
        .by_ref()
        .map_while(Result::ok)
        .find_map(|line| line.strip_prefix(&format!("{SAVED} ")).map(str::to_owned))
        .expect("the app saves the report");
    let folder = Path::new(&folder);
    assert_eq!(
        folder.parent(),
        Some(bugs.as_path()),
        "{}",
        folder.display()
    );

    for file in [
        "report.md",
        "screenshot.png",
        "window.png",
        "tree.txt",
        "frames.jsonl",
        "fixture.json",
        "ui.json",
        "journal.jsonl",
    ] {
        assert!(folder.join(file).is_file(), "{file} is in the report");
    }
    let read = |file: &str| std::fs::read_to_string(folder.join(file)).expect("a text file");
    let report = read("report.md");
    assert!(report.contains("status: open\n"), "{report}");
    assert!(report.contains("the pay bar never filled"), "{report}");
    let run = report
        .lines()
        .find_map(|line| line.strip_prefix("beryl3d_run: "))
        .expect("the report records a run ID");
    assert_eq!(run.len(), 32, "{run}");
    // The commit is the one the binary was built from, not the `unknown` of a program that never
    // recorded it.
    let commit = report
        .lines()
        .find_map(|line| line.strip_prefix("commit: "))
        .expect("the report records a commit");
    assert!(!commit.starts_with("unknown"), "{report}");
    let tree = read("tree.txt");
    assert!(tree.starts_with("game"), "{tree}");
    assert!(
        !tree.contains("region \"bug report\""),
        "the picture shows nothing of the report: {tree}"
    );
    // The report records where the replay was, so `just b3d tree` can reopen it with no game.
    assert_replay_moment(&read("ui.json"));
    // The window is the app's 1600 by 900 points at the scale of a run with no window.
    let window = std::fs::read(folder.join("window.png")).expect("the picture");
    let scale = u32::from(beryl_3d::options::WINDOWLESS_SCALE);
    let size = [1600 * scale, 900 * scale].map(u32::to_be_bytes).concat();
    assert_eq!(&window[16..24], size.as_slice(), "a 1600 by 900 point PNG");
    #[cfg(unix)]
    assert_filing(&temp, folder);

    let second = lines
        .map_while(Result::ok)
        .find_map(|line| line.strip_prefix(&format!("{SAVED} ")).map(str::to_owned))
        .expect("the second report is saved");
    let second = Path::new(&second);
    let second_report =
        std::fs::read_to_string(second.join("report.md")).expect("the second report");
    assert!(second_report.contains("another report in this run"));
    assert!(second_report.contains(&format!("beryl3d_run: {run}\n")));
    #[cfg(unix)]
    assert_filing(&temp, second);
    let _ = std::fs::remove_dir_all(&temp);
}

/// Checks that opening the report froze the replay at the captured moment.
fn assert_replay_moment(saved_ui: &str) {
    let ui: serde_json::Value = serde_json::from_str(saved_ui).expect("ui.json is JSON");
    let log = &ui["log"];
    assert_eq!(log["client"], "Beryl 3D", "{ui}");
    assert_eq!(log["recording"], "pay-default", "{ui}");
    assert_eq!(log["seat"], "you", "{ui}");
    assert_eq!(
        log["ended"], false,
        "pay-default plays for longer than 1000 ms: {ui}"
    );
    let at = log["at"]
        .as_u64()
        .expect("`at` is a whole number of milliseconds");
    assert!(
        (1000..1100).contains(&at),
        "frozen when the script pressed Cmd+B at 1000 ms: {ui}"
    );
}

/// Stands in for `uv`, checking that the app's report is already on disk when filing starts.
#[cfg(unix)]
fn stub_filing(command: &mut Command, checkout: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let uv = checkout.join("uv");
    std::fs::write(
        &uv,
        r#"#!/bin/sh
set -eu
report="$BERYL_BUGS/${10}"
test -f "$report/report.md"
test -f "$report/window.png"
printf '%s\n' "$@" > "$report/filed.tmp"
mv "$report/filed.tmp" "filed-${10}"
"#,
    )
    .expect("write the filing stub");
    std::fs::set_permissions(uv, std::fs::Permissions::from_mode(0o755))
        .expect("make the stub executable");
    let paths = std::env::var_os("PATH").unwrap_or_default();
    let path = std::env::join_paths(
        [checkout.to_owned()]
            .into_iter()
            .chain(std::env::split_paths(&paths)),
    )
    .expect("the stub's PATH");
    command.env("PATH", path).env("BERYL_FILE_BUGS", checkout);
}

#[cfg(unix)]
fn assert_filing(checkout: &Path, report: &Path) {
    use std::time::{Duration, Instant};

    let report_id = report
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .expect("a UTF-8 report ID");
    let marker = checkout.join(format!("filed-{report_id}"));
    // Filing happens on a background thread. Wait for its output, bounding a missing invocation.
    let deadline = Instant::now() + Duration::from_secs(10);
    while !marker.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let invocation = std::fs::read_to_string(marker).expect("the app asks to file the report");
    assert_eq!(
        invocation,
        format!("run\n--quiet\n--no-sync\n--project\ntools\nby\nbugs\nfile\n--\n{report_id}\n")
    );
}
