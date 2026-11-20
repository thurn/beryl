//! The rules and their filing, over seeded records in an in-memory hub database.

use rusqlite::{Connection, params};
use serde_json::json;

use ignite_core::config;
use ignite_core::time::Timestamp;
use ignite_store::hub::{self, findings};

use super::filing::{self, Evaluation};
use super::{backlog, efficiency};

const DAY: f64 = 86_400.0;

fn now() -> Timestamp {
    "2026-11-17T12:00:00.000Z".parse().unwrap()
}

fn ago(days: f64) -> String {
    now().plus_seconds(-days * DAY).to_string()
}

/// A record and its ID; `ticket` and `family` are optional.
struct Run<'a> {
    class: &'a str,
    verb: &'a str,
    family: Option<&'a str>,
    ticket: Option<&'a str>,
    wall_s: f64,
    queued_s: f64,
    cpu_s: f64,
    instructions: Option<f64>,
    days_ago: f64,
}

impl Default for Run<'_> {
    fn default() -> Self {
        Self {
            class: "agent",
            verb: "test",
            family: None,
            ticket: None,
            wall_s: 10.0,
            queued_s: 0.0,
            cpu_s: 10.0,
            instructions: Some(1e9),
            days_ago: 1.0,
        }
    }
}

fn record(conn: &Connection, run: &Run) -> i64 {
    conn.execute(
        "INSERT INTO records (machine, ticket, verb, class, family, queued_s, wall_s, cpu_s,
                              instructions, peak_mb, ignite, at)
         VALUES ('m-1', ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 100, 'abc', ?9)",
        params![
            run.ticket,
            run.verb,
            run.class,
            run.family,
            run.queued_s,
            run.wall_s,
            run.cpu_s,
            run.instructions,
            ago(run.days_ago)
        ],
    )
    .unwrap();
    conn.last_insert_rowid()
}

/// `count` rebuilds of `name` by agents, each costing `cpu_s`.
fn rebuilds(conn: &Connection, name: &str, count: u32, cpu_s: f64) {
    for _ in 0..count {
        let id = record(conn, &Run::default());
        conn.execute(
            "INSERT INTO compile_units (record, crate_name, mode, instructions, cpu_s, rebuilt)
             VALUES (?1, ?2, 'test', 1.1e12, ?3, 1)",
            params![id, name, cpu_s],
        )
        .unwrap();
    }
}

/// A run of `check` at the tip, `days_ago`, with its outcome and cost.
fn tip_result(conn: &Connection, check: &str, days_ago: f64, outcome: &str, cpu_s: f64) {
    let id = record(
        conn,
        &Run {
            class: "tip",
            cpu_s,
            days_ago,
            ..Run::default()
        },
    );
    conn.execute(
        "INSERT INTO results (check_name, hash, outcome, machine, record, at, last_lookup)
         VALUES (?1, ?2, ?3, 'm-1', ?4, ?5, ?5)",
        params![check, format!("h{id}"), outcome, id, ago(days_ago)],
    )
    .unwrap();
}

fn evaluate(conn: &mut Connection, at: Timestamp) -> Evaluation {
    let config = config::Rules::default();
    filing::evaluate(conn, &super::all(&config), &config, at).unwrap()
}

fn ticket_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM tickets", [], |row| row.get(0))
        .unwrap()
}

#[test]
fn slow_crate_files_one_ticket_for_the_most_expensive_crate() {
    let mut conn = hub::open_in_memory().unwrap();
    // 30 rebuilds of 4,000 CPU seconds: 33.3 core-hours a week, saving 13.3.
    rebuilds(&conn, "crate_a", 30, 4000.0);
    rebuilds(&conn, "crate_b", 10, 1000.0);
    let first = evaluate(&mut conn, now());
    assert_eq!(first.filed.len(), 1);
    let (title, body, labels, saving): (String, String, String, String) = conn
        .query_row(
            "SELECT title, body, labels, saving FROM tickets WHERE id = ?1",
            [&first.filed[0]],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert!(title.contains("crate_a"), "{title}");
    assert!(body.starts_with("crate_a costs 33.3 core-hours a week in rebuilds (30 rebuilds"));
    assert!(body.contains("\nmeasure: "), "{body}");
    assert_eq!(labels, r#"["improvement","rule:slow-crate"]"#);
    let saving: serde_json::Value = serde_json::from_str(&saving).unwrap();
    assert_eq!(saving["subject"], "crate_a");
    assert_eq!(saving["unit"], "core-hours");

    let second = evaluate(&mut conn, now().plus_seconds(3600.0));
    assert!(second.filed.is_empty());
    assert_eq!(ticket_count(&conn), 1);
    assert_eq!(
        findings::open(&conn).unwrap()[0].ticket,
        Some(first.filed[0].clone())
    );
}

#[test]
fn a_finding_under_the_minimum_files_nothing_and_is_reported() {
    let mut conn = hub::open_in_memory().unwrap();
    // 10 core-hours a week of rebuilds saves 4, under the minimum of 10.
    rebuilds(&conn, "crate_a", 9, 4000.0);
    let evaluation = evaluate(&mut conn, now());
    assert!(evaluation.filed.is_empty());
    assert_eq!(evaluation.reported[0].subject, "crate_a");
    assert_eq!(ticket_count(&conn), 0);
    let shown = findings::open(&conn).unwrap();
    assert_eq!((shown.len(), shown[0].ticket.as_deref()), (1, None));
}

#[test]
fn a_closed_ticket_blocks_its_subject_for_30_days() {
    let mut conn = hub::open_in_memory().unwrap();
    rebuilds(&conn, "crate_a", 30, 4000.0);
    let filed = evaluate(&mut conn, now()).filed;
    findings::close_ticket(&conn, &filed[0], now()).unwrap();
    // The records are a day old at `now`; keep the crate as expensive in later weeks.
    let later = |days: f64| now().plus_seconds(days * DAY);
    for days in [29.0, 31.0] {
        for _ in 0..30 {
            let id = record(
                &conn,
                &Run {
                    days_ago: 1.0 - days,
                    ..Run::default()
                },
            );
            conn.execute(
                "INSERT INTO compile_units (record, crate_name, mode, cpu_s, rebuilt)
                 VALUES (?1, 'crate_a', 'test', 4000, 1)",
                [id],
            )
            .unwrap();
        }
    }
    assert!(evaluate(&mut conn, later(29.0)).filed.is_empty());
    assert_eq!(evaluate(&mut conn, later(31.0)).filed.len(), 1);
    assert_eq!(ticket_count(&conn), 2);
}

#[test]
fn tier_gap_is_reported_and_never_filed() {
    let mut conn = hub::open_in_memory().unwrap();
    let report = json!({"groups": [{"unit": "dollars", "findings": [
        {"rule": "tier-gap", "unit": "dollars", "summary": "Opus where Sonnet would do",
         "saving_per_week": {"dollars": 900.0}, "cost_per_week": 2000.0,
         "evidence": "efficiency findings --rule tier-gap"},
        {"rule": "wake-ups", "unit": "dollars", "summary": "idle wake-ups",
         "saving_per_week": {"dollars": 40.0}, "cost_per_week": 60.0, "evidence": "e"}]}]});
    conn.execute(
        "INSERT INTO metrics (machine, source, data, at) VALUES ('m-1', ?1, ?2, ?3)",
        params![efficiency::SOURCE, report.to_string(), ago(0.5)],
    )
    .unwrap();
    let evaluation = evaluate(&mut conn, now());
    assert_eq!(evaluation.filed.len(), 1);
    let rule: String = conn
        .query_row("SELECT saving FROM tickets", [], |row| row.get(0))
        .unwrap();
    assert!(rule.contains(r#""rule":"wake-ups""#), "{rule}");
    let tier_gap = evaluation.reported.iter().find(|f| f.rule == "tier-gap");
    assert!((tier_gap.unwrap().saving - 900.0).abs() < 1e-9);
}

#[test]
fn a_protected_check_with_a_year_of_runs_and_no_catch_files_nothing() {
    let mut conn = hub::open_in_memory().unwrap();
    for day in 0..365 {
        let days_ago = f64::from(day) + 0.5;
        // 4 runs a day of 2,000 CPU seconds: 15.6 core-hours a week each.
        for _ in 0..4 {
            tip_result(&conn, "clippy:workspace", days_ago, "pass", 2000.0);
            tip_result(&conn, "rust:kernel::tests::slow", days_ago, "pass", 2000.0);
        }
    }
    let evaluation = evaluate(&mut conn, now());
    assert_eq!(evaluation.filed.len(), 1);
    let subjects: Vec<String> = findings::open(&conn)
        .unwrap()
        .into_iter()
        .map(|finding| finding.subject)
        .collect();
    assert_eq!(subjects, vec!["rust:kernel::tests::slow"]);
}

#[test]
fn never_caught_skips_new_checks_and_checks_that_caught_something() {
    let mut conn = hub::open_in_memory().unwrap();
    for day in 0..40 {
        let days_ago = f64::from(day) + 0.5;
        for _ in 0..4 {
            tip_result(&conn, "rust:caught", days_ago, "pass", 2000.0);
            if day < 20 {
                tip_result(&conn, "rust:young", days_ago, "pass", 2000.0);
            }
        }
    }
    tip_result(&conn, "rust:caught", 3.0, "fail", 2000.0);
    let evaluation = evaluate(&mut conn, now());
    assert!(evaluation.filed.is_empty());
    assert!(evaluation.reported.is_empty());
}

/// Six `tier:` labelled tickets of each of two tiers, with the given runs of `test rust`.
fn tickets_with_runs(conn: &Connection, sonnet_runs: u32, opus_runs: u32) {
    for (tier, runs) in [("sonnet", sonnet_runs), ("opus", opus_runs)] {
        for n in 0..6 {
            let id = format!("t{tier}{n}");
            conn.execute(
                "INSERT INTO tickets (id, title, labels, created_at, created_by, updated_at,
                                      version)
                 VALUES (?1, 'x', ?2, ?3, 'a', ?3, 1)",
                params![id, format!(r#"["tier:{tier}"]"#), ago(9.0)],
            )
            .unwrap();
            for _ in 0..runs {
                record(
                    conn,
                    &Run {
                        family: Some("rust"),
                        ticket: Some(&id),
                        wall_s: 600.0,
                        ..Run::default()
                    },
                );
            }
        }
    }
}

#[test]
fn repeated_command_names_the_kind_that_runs_a_command_far_more_often() {
    let mut conn = hub::open_in_memory().unwrap();
    // Sonnet tickets run it 8 times each, opus tickets twice: 36 excess runs of 10 minutes.
    tickets_with_runs(&conn, 8, 2);
    let evaluation = evaluate(&mut conn, now());
    assert_eq!(evaluation.filed.len(), 1);
    let saving: String = conn
        .query_row(
            "SELECT saving FROM tickets WHERE saving IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let saving: serde_json::Value = serde_json::from_str(&saving).unwrap();
    assert_eq!(saving["subject"], "test rust in tier:sonnet");
    assert!((saving["amount"].as_f64().unwrap() - 6.0).abs() < 1e-9);
}

#[test]
fn repeated_command_files_nothing_when_the_kinds_run_it_alike() {
    let mut conn = hub::open_in_memory().unwrap();
    tickets_with_runs(&conn, 3, 2);
    assert!(evaluate(&mut conn, now()).filed.is_empty());
}

#[test]
fn slow_verb_names_a_verb_over_a_second_at_the_median() {
    let mut conn = hub::open_in_memory().unwrap();
    for n in 0..3000 {
        let quick = Run {
            verb: "status",
            wall_s: 0.4,
            instructions: None,
            ..Run::default()
        };
        record(&conn, &quick);
        let slow = Run {
            verb: "brief",
            wall_s: 4.0,
            queued_s: 0.5,
            instructions: None,
            days_ago: 1.0 + f64::from(n % 5),
            ..Run::default()
        };
        record(&conn, &slow);
    }
    let evaluation = evaluate(&mut conn, now());
    // 3,000 runs of 3.5 seconds, 2.5 above the limit: 2.1 agent-hours a week saved.
    assert_eq!(evaluation.filed.len(), 1);
    let title: String = conn
        .query_row("SELECT title FROM tickets", [], |row| row.get(0))
        .unwrap();
    assert_eq!(title, "Make `ignite brief` faster");
}

#[test]
fn a_landed_improvement_gets_its_measure_seven_days_later_once() {
    let mut conn = hub::open_in_memory().unwrap();
    rebuilds(&conn, "crate_a", 30, 4000.0);
    let id = evaluate(&mut conn, now()).filed.remove(0);
    conn.execute(
        "UPDATE tickets SET status = 'closed', landed_at = ?2 WHERE id = ?1",
        params![id, now().to_string()],
    )
    .unwrap();
    findings::close_ticket(&conn, &id, now()).unwrap();
    assert!(
        evaluate(&mut conn, now().plus_seconds(6.0 * DAY))
            .measured
            .is_empty()
    );
    let week = now().plus_seconds(7.0 * DAY);
    assert_eq!(evaluate(&mut conn, week).measured, vec![id.clone()]);
    assert!(
        evaluate(&mut conn, week.plus_seconds(3600.0))
            .measured
            .is_empty()
    );
    let text: String = conn
        .query_row(
            "SELECT text FROM comments WHERE ticket = ?1",
            [&id],
            |row| row.get(0),
        )
        .unwrap();
    // The week-old rebuilds have aged out, so the crate measures nothing now.
    assert_eq!(
        text,
        "Seven days after landing, `slow-crate` measures crate_a at 0.0 core-hours a week. \
         Before this ticket: 33.3 core-hours a week."
    );
}

#[test]
fn evaluation_runs_at_most_once_an_hour() {
    let mut conn = hub::open_in_memory().unwrap();
    let config = config::Rules::default();
    assert!(
        filing::evaluate_if_due(&mut conn, &config, now())
            .unwrap()
            .is_some()
    );
    let soon = now().plus_seconds(1800.0);
    assert!(
        filing::evaluate_if_due(&mut conn, &config, soon)
            .unwrap()
            .is_none()
    );
    let later = now().plus_seconds(3600.0);
    assert!(
        filing::evaluate_if_due(&mut conn, &config, later)
            .unwrap()
            .is_some()
    );
}

#[test]
fn a_failed_evaluation_is_not_stamped_so_the_next_arrival_retries() {
    let mut conn = hub::open_in_memory().unwrap();
    let config = config::Rules::default();
    conn.execute("ALTER TABLE findings RENAME TO findings_away", [])
        .unwrap();
    assert!(filing::evaluate_if_due(&mut conn, &config, now()).is_err());
    conn.execute("ALTER TABLE findings_away RENAME TO findings", [])
        .unwrap();
    // A minute later is well inside the hour, yet the evaluation runs because none succeeded.
    let soon = now().plus_seconds(60.0);
    assert!(
        filing::evaluate_if_due(&mut conn, &config, soon)
            .unwrap()
            .is_some()
    );
}

#[test]
fn backlog_ratios_take_the_largest_unit() {
    let conn = hub::open_in_memory().unwrap();
    // 100 core-hours and $1,000 last week.
    record(
        &conn,
        &Run {
            class: "batch",
            cpu_s: 360_000.0,
            ..Run::default()
        },
    );
    let dollars = json!({"table": "window_dollars",
                         "row": {"split": "class", "start": ago(2.0), "dollars": 1000.0}});
    conn.execute(
        "INSERT INTO metrics (machine, source, data, at, kind)
         VALUES ('m-1', 'm-1', ?1, ?2, 'window_dollars')",
        params![dollars.to_string(), ago(1.0)],
    )
    .unwrap();
    for (n, saving) in [
        r#"{"rule":"r","subject":"a","unit":"dollars","amount":50}"#,
        r#"{"rule":"r","subject":"b","unit":"core-hours","amount":11}"#,
    ]
    .into_iter()
    .enumerate()
    {
        conn.execute(
            "INSERT INTO tickets (id, title, saving, created_at, created_by, updated_at, version)
             VALUES (?1, 'x', ?2, ?3, 'a', ?3, 1)",
            params![format!("t{n}"), saving, ago(1.0)],
        )
        .unwrap();
    }
    let ratios = backlog::ratios(&conn, now()).unwrap();
    assert!((ratios.last_week.dollars - 1000.0).abs() < 1e-9);
    assert!((ratios.last_week.core_hours - 100.0).abs() < 1e-6);
    assert!((ratios.fraction - 0.11).abs() < 1e-9);
}

/// The query plan of `sql`, one step per line.
fn plan(conn: &rusqlite::Connection, sql: &str, args: &[&dyn rusqlite::ToSql]) -> String {
    let mut statement = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
    let steps = statement
        .query_map(args, |row| row.get::<_, String>(3))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    steps.join("\n")
}

/// The hub's two readers of `metrics` look rows up by an index, never by scanning the table,
/// which held 1.5 million rows when a scan on every evaluation locked the hub.
#[test]
fn the_metrics_readers_use_an_index() {
    let conn = hub::open_in_memory().unwrap();
    let (since, until) = (ago(14.0), ago(0.0));
    let findings = plan(
        &conn,
        efficiency::REPORTED,
        &[&efficiency::SOURCE, &since, &until],
    );
    assert!(findings.contains("INDEX metrics_source_at ("), "{findings}");
    let dollars = plan(&conn, backlog::WINDOW_DOLLARS, &[&since]);
    assert!(dollars.contains("INDEX metrics_kind_at ("), "{dollars}");
    for found in [findings, dollars] {
        assert!(!found.contains("SCAN metrics"), "{found}");
    }
}
