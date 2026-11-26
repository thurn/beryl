//! Bug reports: an app replaying `pay-default` with no window, driven to press Cmd+B, take the
//! whole window, describe the bug and submit it, writes a report folder into `$BERYL_BUGS` with
//! the picture of the frozen frame, its element tree, the frame log, the latest views, the
//! recording's moment in `ui.json` and the files every report holds. With local filing enabled,
//! it also hands that report's ID to the filing command.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use beryl_client::bugs::SAVED;
use beryl_session::bugs::filing::FINISHED;

/// The app, stopped when dropped.
struct App(Child);

impl Drop for App {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Submits two reports after the replay starts. Only the first Cmd+B has a time, because the
/// frozen moment is checked. Each `still` waits for the report to finish capturing, or for the
/// save and the "Saved" notice to end, so that a slow machine cannot press Cmd+B while the first
/// report is still open, which does nothing. The last line keeps the run going while they are
/// saved and filed. It bounds a missing save or filing outcome by closing stdout.
const SCRIPT: &str = "\
at 1000ms key Cmd+B
still
key space
still
type field \"description\" \"the pay bar never filled\"
press button \"Submit\"
still
key Cmd+B
still
key space
still
type field \"description\" \"another report in this run\"
press button \"Submit\"
at 60000ms move 10,10
";

#[test]
fn a_report_saved_from_a_replay_holds_the_picture_the_tree_and_the_logs() {
    let temp = std::env::temp_dir().join(format!("client bugs {}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp);
    std::fs::create_dir_all(&temp).expect("a temporary folder");
    let bugs = temp.join("bugs");
    let script = temp.join("report.drive");
    std::fs::write(&script, SCRIPT).expect("the script is written");
    let mut command = Command::new(env!("CARGO_BIN_EXE_beryl"));
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
    let app = App(child);
    let mut output = BufReader::new(stdout);
    let [first, second] = saved_reports(&mut output);
    for folder in [&first, &second] {
        assert_report_files(&bugs, folder);
        assert_filing(&temp, folder);
    }

    let read = |file: &str| std::fs::read_to_string(first.join(file)).expect("a text file");
    let report = read("report.md");
    assert!(report.contains("status: open\n"), "{report}");
    assert!(report.contains("the pay bar never filled"), "{report}");
    let run = report
        .lines()
        .find_map(|line| line.strip_prefix("client_run: "))
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
    // The report records where the replay was, so `just client tree` can reopen it with no game.
    assert_replay_moment(&read("ui.json"));
    // The window is the app's 1600 by 900 points at the scale of a run with no window.
    let window = std::fs::read(first.join("window.png")).expect("the picture");
    let scale = u32::from(beryl_client::options::WINDOWLESS_SCALE);
    let size = [1600 * scale, 900 * scale].map(u32::to_be_bytes).concat();
    assert_eq!(&window[16..24], size.as_slice(), "a 1600 by 900 point PNG");
    let second_report =
        std::fs::read_to_string(second.join("report.md")).expect("the second report");
    assert!(second_report.contains("another report in this run"));
    assert!(second_report.contains(&format!("client_run: {run}\n")));
    // Keep stdout open until the app stops.
    drop(app);
    let _ = std::fs::remove_dir_all(&temp);
}

/// Collects saves and filing outcomes together because either thread can print first.
fn saved_reports(output: &mut impl BufRead) -> [PathBuf; 2] {
    let saved = format!("{SAVED} ");
    let finished = format!("{FINISHED} ");
    let mut reports = Vec::new();
    let mut filed = Vec::new();

    for line in output.lines() {
        let line = line.expect("read the app's output");
        if let Some(folder) = line.strip_prefix(&saved) {
            reports.push(PathBuf::from(folder));
        }
        if let Some(outcome) = line.strip_prefix(&finished) {
            let (id, status) = outcome
                .split_once(' ')
                .expect("report ID and filing outcome");
            assert_eq!(status, "ok", "filing report {id}");
            filed.push(id.to_owned());
        }
        if reports.len() == 2 && filed.len() == 2 {
            break;
        }
    }

    let reports: [PathBuf; 2] = reports.try_into().expect("the app saves two reports");
    let mut expected = reports.each_ref().map(|folder| report_id(folder));
    expected.sort_unstable();
    filed.sort_unstable();
    assert_eq!(filed, expected, "one filing outcome per saved report");
    reports
}

fn assert_report_files(root: &Path, folder: &Path) {
    assert_eq!(folder.parent(), Some(root), "{}", folder.display());
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
        assert!(
            folder.join(file).is_file(),
            "{file} is in {}",
            folder.display()
        );
    }
}

/// Checks that opening the report froze the replay at the captured moment.
fn assert_replay_moment(saved_ui: &str) {
    let ui: serde_json::Value = serde_json::from_str(saved_ui).expect("ui.json is JSON");
    let log = &ui["log"];
    assert_eq!(log["client"], "Beryl", "{ui}");
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
fn stub_filing(command: &mut Command, checkout: &Path) {
    write_filing_stub(checkout);
    // Keep an installed uv.exe out of the app's search path so filing reaches the stub.
    command
        .env("PATH", checkout)
        .env("BERYL_FILE_BUGS", checkout);
}

#[cfg(unix)]
fn write_filing_stub(checkout: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let uv = checkout.join("uv");
    std::fs::write(
        &uv,
        r#"#!/bin/sh
set -eu
report="$BERYL_BUGS/${10}"
for file in report.md window.png; do
    if [ ! -f "$report/$file" ]; then
        printf 'report file missing when filing started: %s\n' "$report/$file" >&2
        exit 1
    fi
done
printf '%s\n' "$@" > "filed-${10}"
"#,
    )
    .expect("write the filing stub");
    std::fs::set_permissions(uv, std::fs::Permissions::from_mode(0o755))
        .expect("make the stub executable");
}

#[cfg(windows)]
fn write_filing_stub(checkout: &Path) {
    let script = r#"@echo off
setlocal EnableExtensions DisableDelayedExpansion
shift /1
set "report_id=%~9"
for %%f in (report.md window.png) do (
    if not exist "%BERYL_BUGS%\%report_id%\%%f" (
        >&2 echo Missing report file %%f when filing started
        exit /b 1
    )
)
> "filed-%report_id%" (
    for %%a in (%*) do echo %%a
)
exit /b %errorlevel%
"#;
    std::fs::write(checkout.join("uv.cmd"), script.replace('\n', "\r\n"))
        .expect("write the filing stub");
}

fn report_id(report: &Path) -> &str {
    report
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .expect("a UTF-8 report ID")
}

fn assert_filing(checkout: &Path, report: &Path) {
    let report_id = report_id(report);
    let marker = checkout.join(format!("filed-{report_id}"));
    let invocation = std::fs::read_to_string(marker).expect("the app asks to file the report");
    assert_eq!(
        invocation.lines().collect::<Vec<_>>(),
        [
            "run",
            "--quiet",
            "--no-sync",
            "--project",
            "tools",
            "by",
            "bugs",
            "file",
            "--",
            report_id,
        ]
    );
}
