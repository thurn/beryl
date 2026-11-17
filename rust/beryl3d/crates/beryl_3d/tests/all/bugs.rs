//! Bug reports: an app replaying `pay-default` with no window, driven to press Cmd+B, take the
//! whole window, describe the bug and submit it, writes a report folder into `$BERYL_BUGS` with
//! the picture of the frozen frame, its element tree, the frame log, the latest views, the
//! recording's moment in `ui.json` and the files every report holds.

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

/// Opens the report once the replay is under way, takes the whole window, types and submits.
/// The last line only keeps the run going while the report is written, since a drive script
/// ends the run after its last action.
const SCRIPT: &str = "\
at 1000ms key Cmd+B
at 1500ms key space
at 2000ms type field \"description\" \"the pay bar never filled\"
at 2500ms press button \"Submit\"
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
    let mut child = Command::new(env!("CARGO_BIN_EXE_beryl3d"))
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
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
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
    let tree = read("tree.txt");
    assert!(tree.starts_with("game"), "{tree}");
    assert!(
        !tree.contains("region \"bug report\""),
        "the picture shows nothing of the report: {tree}"
    );
    // The report records where the replay was, so `just b3d tree` can reopen it with no game.
    let ui: serde_json::Value = serde_json::from_str(&read("ui.json")).expect("ui.json is JSON");
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
    // The window is the app's 1600 by 900 points at one pixel to a point.
    let window = std::fs::read(folder.join("window.png")).expect("the picture");
    assert_eq!(
        &window[16..24],
        &[0, 0, 6, 64, 0, 0, 3, 132],
        "a 1600 by 900 PNG"
    );
    let _ = std::fs::remove_dir_all(&temp);
}
