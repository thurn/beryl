use std::path::{Path, PathBuf};

use rusqlite::Connection;

use ignite_core::fakes::FakeGit;
use ignite_core::ids::Commit;
use ignite_core::inputs::{CrateGraph, Package};

use crate::hub;
use crate::import::verdicts::tests::add_verdict_ticket;
use crate::import::{ImportReport, RunLogSource, Sources, run};

const S1: &str = "a1a1a1a1";
const S2: &str = "b2b2b2b2";
const C3: &str = "c3c3c3c3";
const D4: &str = "d4d4d4d4";

fn line(time: &str, place: &str, commit: &str, check: &str, outcome: &str, extra: &str) -> String {
    let lane = check.split(':').next().unwrap();
    format!(
        r#"{{"time": "{time}", "host": "h1", "commit": "{commit}", "script": "check", "place": "{place}", "lane": "{lane}", "check": "{check}", "outcome": "{outcome}"{extra}}}"#
    )
}

/// A run log of two staging commits and two commits of one branch, with every kind of line the
/// importer leaves out.
fn write_log(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ignite-runlog-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let first = [
        line(
            "2026-11-10T00:00:00+00:00",
            "staging",
            S1,
            "rust:x::y",
            "pass",
            r#", "cpu_seconds": 1.0"#,
        ),
        line(
            "2026-11-10T00:00:01+00:00",
            "staging",
            S1,
            "rust:z",
            "pass",
            "",
        ),
        line(
            "2026-11-10T01:00:00+00:00",
            "staging",
            S2,
            "rust:x::y",
            "pass",
            r#", "cpu_seconds": 2.0"#,
        ),
    ];
    let second = [
        line(
            "2026-11-10T02:00:00Z",
            "topic",
            C3,
            "rust:x::y",
            "fail",
            r#", "cpu_seconds": 3.0"#,
        ),
        line(
            "2026-11-10T03:00:00-01:00",
            "topic",
            D4,
            "rust:x::y",
            "pass",
            r#", "cpu_seconds": 4.0"#,
        ),
        // Left out: a skip, an incomplete run, a bisection rerun and an exploration run.
        line("2026-11-10T04:00:00Z", "topic", D4, "rust:z", "skip", ""),
        line(
            "2026-11-10T04:00:00Z",
            "topic",
            D4,
            "rust:z",
            "fail",
            r#", "timeout": "hang""#,
        ),
        line("2026-11-10T04:00:00Z", "topic", D4, "rust:z", "fail", "")
            .replace(r#""script": "check""#, r#""script": "bisect""#),
        line(
            "2026-11-10T04:00:00Z",
            "topic",
            D4,
            "fuzz:seed/1",
            "fail",
            "",
        ),
        "{not json".to_owned(),
    ];
    std::fs::write(dir.join("2026-11-10T00.jsonl"), first.join("\n") + "\n").unwrap();
    std::fs::write(dir.join("2026-11-10T02.jsonl"), second.join("\n") + "\n").unwrap();
    std::fs::write(dir.join("notes.txt"), "not a log file").unwrap();
    dir
}

fn git() -> FakeGit {
    let git = FakeGit::default();
    let commit = |c: &str| Commit::new(c).unwrap();
    git.set_changed(&commit(S1), &commit(S2), &["ws/crates/core/src/lib.rs"]);
    git.set_changed(&commit(S2), &commit(C3), &["web/src/a.ts"]);
    git.set_changed(
        &commit(S2),
        &commit(D4),
        &["web/src/a.ts", "ws/crates/core/src/x.rs"],
    );
    git
}

fn graph() -> CrateGraph {
    CrateGraph {
        packages: vec![Package {
            name: "core".to_owned(),
            dir: "ws/crates/core".to_owned(),
            deps: Vec::new(),
            workspace: "ws".to_owned(),
        }],
    }
}

fn add_verdicts(hub: &Connection) {
    add_verdict_ticket(
        hub,
        "ci1",
        "rust:x::y",
        &[
            &format!(
                r#"{{"time":"2026-11-10T05:00:00+00:00","commit":"{C3}","lane":"rust","verdict":"catch"}}"#
            ),
            // No run of this commit is in the log.
            r#"{"time":"2026-10-01T00:00:00+00:00","commit":"ffff0000","lane":"rust","verdict":"stale"}"#,
            &format!(
                r#"{{"time":"2026-11-10T05:00:00+00:00","commit":"{C3}","lane":"rust","verdict":"miss"}}"#
            ),
        ],
    );
    add_verdict_ticket(
        hub,
        "ci2",
        "rust:z",
        &[&format!(
            r#"{{"time":"2026-11-10T06:00:00-08:00","host":"h2","commit":"{D4}","lane":"rust","verdict":"flaky","fixed":"tree eeee0000"}}"#
        )],
    );
    add_verdict_ticket(
        hub,
        "ci3",
        "fuzz:seed/1",
        &[&format!(
            r#"{{"time":"2026-11-10T05:00:00+00:00","commit":"{D4}","lane":"fuzz","verdict":"catch"}}"#
        )],
    );
}

fn import(hub: &mut Connection, folder: &Path, git: &FakeGit, dry_run: bool) -> ImportReport {
    let sources = Sources {
        run_log: Some(RunLogSource {
            folder: folder.to_owned(),
            git,
            graph: graph(),
            staging: "staging".to_owned(),
            exploration: vec!["fuzz".to_owned()],
        }),
        ..Sources::default()
    };
    run(hub, &sources, dry_run).unwrap().remove(0)
}

fn area_counts(hub: &Connection, check: &str) -> Vec<(String, u32, u32)> {
    let reply = hub::checks::stats(hub, check.split(':').next().unwrap()).unwrap();
    let stats = reply
        .checks
        .into_iter()
        .find(|c| c.check.as_str() == check)
        .unwrap();
    stats
        .areas
        .into_iter()
        .map(|a| (a.area, a.runs, a.failures))
        .collect()
}

#[test]
fn runs_count_in_the_areas_of_their_change_and_failures_come_from_verdicts() {
    let folder = write_log("areas");
    let git = git();
    let mut hub = hub::open_in_memory().unwrap();
    add_verdicts(&hub);
    let report = import(&mut hub, &folder, &git, false);

    // S1 has nothing before it, so its run counts in the unknown area.
    assert_eq!(
        area_counts(&hub, "rust:x::y"),
        vec![
            (String::new(), 1, 0),
            ("core".to_owned(), 2, 0),
            ("web/src/".to_owned(), 2, 1),
        ]
    );
    let lines = report.get("lines").unwrap();
    assert_eq!((lines.read, lines.written, lines.skipped), (10, 0, 5));
    assert_eq!(report.problems.len(), 1, "{report}");
    let verdicts = report.get("verdicts").unwrap();
    assert_eq!(
        (verdicts.read, verdicts.written, verdicts.skipped),
        (5, 0, 3)
    );

    let (costs, median, unit): (String, f64, String) = hub
        .query_row(
            "SELECT costs, median, unit FROM check_costs WHERE check_name = 'rust:x::y'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        (costs.as_str(), median, unit.as_str()),
        ("[1.0,2.0,3.0,4.0]", 2.5, "cpu_s")
    );

    let flaky: (String, String, String, String, String) = hub
        .query_row("SELECT * FROM flaky", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .unwrap();
    assert_eq!(
        flaky,
        (
            "rust:z".to_owned(),
            "eeee0000".to_owned(),
            D4.to_owned(),
            "h2".to_owned(),
            "2026-11-10T14:00:00.000Z".to_owned()
        )
    );

    let exploration: i64 = hub
        .query_row(
            "SELECT count(*) FROM checks WHERE family = 'fuzz'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(exploration, 0);
    let group: String = hub
        .query_row(
            "SELECT check_group FROM checks WHERE name = 'rust:z'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(group, "rust");
}

#[test]
fn a_second_import_writes_nothing_and_a_dry_run_writes_nothing() {
    let folder = write_log("twice");
    let git = git();
    let mut hub = hub::open_in_memory().unwrap();
    add_verdicts(&hub);
    let dry = import(&mut hub, &folder, &git, true);
    assert!(dry.wrote());
    let rows: i64 = hub
        .query_row(
            "SELECT (SELECT count(*) FROM check_stats) + (SELECT count(*) FROM checks)
                  + (SELECT count(*) FROM flaky) + (SELECT count(*) FROM check_costs)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(rows, 0);

    let first = import(&mut hub, &folder, &git, false);
    let second = import(&mut hub, &folder, &git, false);
    assert!(first.wrote());
    assert!(!second.wrote(), "{second}");
    assert_eq!(
        first.get("check stats").unwrap().read,
        second.get("check stats").unwrap().skipped
    );
}

#[test]
fn a_change_git_cannot_resolve_counts_in_the_unknown_area() {
    let folder = write_log("unknown");
    let mut hub = hub::open_in_memory().unwrap();
    let report = import(&mut hub, &folder, &FakeGit::default(), false);
    // The fake answers no paths for every pair, so every change is empty.
    let changes = report.get("changes").unwrap();
    assert_eq!((changes.read, changes.skipped), (4, 4));
    assert_eq!(area_counts(&hub, "rust:x::y"), vec![(String::new(), 4, 0)]);
}
