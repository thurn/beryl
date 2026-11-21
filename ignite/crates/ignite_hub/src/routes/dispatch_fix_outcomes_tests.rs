use ignite_core::protocol::hub::{
    CheckFix, FixOutcome, FixSource, FlakyReport, FoundBy, RegressionReport,
};
use ignite_core::protocol::tickets::{BatchRequest, Operation};
use ignite_store::hub::{self, batch, checks, findings, fix_outcomes};

use super::tests::{at, claim, file, landed, post, post_with_test_paths, register, start};
use super::{Connection, Request, Timestamp, json};
use crate::upkeep::{filing::UPKEEP, flaky, regressions};

fn report(name: &str) -> RegressionReport {
    RegressionReport {
        check: format!("unit:{name}").parse().unwrap(),
        first_bad: "badd".parse().unwrap(),
        last_good: "600d".parse().unwrap(),
        caused_by: None,
        output: "failed".to_owned(),
        suspects: Vec::new(),
        platform: None,
        search: None,
        found_by: FoundBy::TipVerification,
        detected_at: None,
    }
}

fn history(conn: &mut Connection, check: &str, now: Timestamp) -> Vec<CheckFix> {
    let request = Request {
        method: "GET".to_owned(),
        path: "/checks/fix-outcomes".to_owned(),
        query: vec![("check".to_owned(), check.to_owned())],
        writer: None,
        body: Vec::new(),
        headers: Vec::new(),
    };
    let reply = crate::routes::checks::handle(conn, &request, now).unwrap();
    assert_eq!(reply.status, 200);
    serde_json::from_slice(&reply.body).unwrap()
}

#[test]
fn closing_regressions_records_every_check_and_each_close_once() {
    let mut conn = hub::open_in_memory().unwrap();
    register(&mut conn);
    let config = ["**/tests/**".to_owned(), "**/*_tests.rs".to_owned()];
    for (index, (files, outcome)) in [
        (
            vec!["src/main.rs", "tests/regression.rs"],
            FixOutcome::Product,
        ),
        (
            vec!["tests/regression.rs", "src/read_tests.rs"],
            FixOutcome::TestOnly,
        ),
        (vec![], FixOutcome::None),
    ]
    .into_iter()
    .enumerate()
    {
        let a = report(&format!("a{index}"));
        let b = report(&format!("b{index}"));
        let (ticket, _) = regressions::file(&mut conn, UPKEEP, &a, at(1.0)).unwrap();
        let (grouped, _) = regressions::file(&mut conn, UPKEEP, &b, at(1.0)).unwrap();
        assert_eq!(ticket, grouped);
        assert_eq!(
            post(&mut conn, "/claims", &claim(&ticket, "a"), at(2.0)).0,
            200
        );
        let mut body = landed(&ticket, "a", "fixed");
        body["work"]["files"] = json!(files);
        if files.is_empty() {
            body["work"]["commits"] = json!([]);
        }
        assert_eq!(
            post_with_test_paths(&mut conn, "/landed", &body, at(3.0), &config).0,
            200
        );
        assert_eq!(
            post_with_test_paths(&mut conn, "/landed", &body, at(4.0), &config).0,
            200
        );
        for check in [&a.check, &b.check] {
            let fixes = history(&mut conn, check.as_str(), at(4.0));
            assert_eq!(
                fixes,
                vec![CheckFix {
                    check: check.clone(),
                    ticket: ticket.parse().unwrap(),
                    at: at(3.0),
                    outcome,
                    source: FixSource::Regression,
                    first_bad: Some(a.first_bad.clone()),
                    fixing_commit: (!files.is_empty()).then(|| "3a9e4c1".parse().unwrap()),
                }]
            );
        }

        if outcome == FixOutcome::Product {
            let (reopened, change) = regressions::file(&mut conn, UPKEEP, &a, at(5.0)).unwrap();
            assert_eq!(ticket, reopened);
            assert_eq!(change, regressions::Outcome::Reopened);
            assert_eq!(
                post(&mut conn, "/claims", &claim(&ticket, "b"), at(6.0)).0,
                200
            );
            let close = json!({"ticket": ticket, "machine": "m-1", "session": "b", "reason": "already fixed"});
            assert_eq!(post(&mut conn, "/close", &close, at(7.0)).0, 200);
            let fixes = history(&mut conn, a.check.as_str(), at(8.0));
            assert_eq!(fixes.len(), 2);
            assert_eq!(fixes[0].outcome, FixOutcome::None);
            assert_eq!(fixes[0].fixing_commit, None);
            assert_eq!(fixes[1].outcome, FixOutcome::Product);
            let cutoff = at(8.0).plus_seconds(fix_outcomes::HISTORY_DAYS * 86_400.0);
            assert!(history(&mut conn, a.check.as_str(), cutoff).is_empty());
        }
    }
}

#[test]
fn closing_a_flaky_ticket_by_batch_records_each_grouped_check_once() {
    let mut conn = hub::open_in_memory().unwrap();
    for check in ["unit:a", "unit:b"] {
        checks::register(&conn, check).unwrap();
        for commit in ["aaaa", "bbbb"] {
            let report = FlakyReport {
                check: check.parse().unwrap(),
                hash: "a".repeat(64).parse().unwrap(),
                commit: commit.parse().unwrap(),
                machine: "m-1".parse().unwrap(),
                always: true,
                command: String::new(),
                output: "flaked".to_owned(),
            };
            checks::record_flaky(&conn, &report, start()).unwrap();
        }
    }
    let outcome = flaky::switch_off_repeated(&mut conn, at(1.0)).unwrap();
    assert_eq!(outcome.filed.len(), 2);
    let ticket = &outcome.filed[0].ticket;
    assert_eq!(ticket, &outcome.filed[1].ticket);
    let request = BatchRequest {
        preconditions: Vec::new(),
        ops: vec![Operation::Close {
            id: ticket.clone(),
            reason: "cleared".into(),
            include_ready: false,
        }],
    };
    batch::apply(&mut conn, &request, "operator", "m-1", at(2.0)).unwrap();
    let fixes = fix_outcomes::between(&conn, start(), at(3.0), None).unwrap();
    assert_eq!(fixes.len(), 2);
    for (fix, check) in fixes.iter().zip(["unit:a", "unit:b"]) {
        assert_eq!(fix.check.as_str(), check);
        assert_eq!(fix.ticket.as_str(), ticket);
        assert_eq!(fix.source, FixSource::Flaky);
        assert_eq!(fix.outcome, FixOutcome::None);
    }
}

#[test]
fn only_product_landings_credit_exploration() {
    let mut conn = hub::open_in_memory().unwrap();
    register(&mut conn);
    let ids = file(
        &mut conn,
        &[
            ("first", &["found-by:fuzz"], &[]),
            ("second", &["found-by:fuzz"], &[]),
        ],
    );
    let config = ["**/tests/**".to_owned()];
    for (id, file) in ids.iter().zip(["tests/regression.rs", "src/main.rs"]) {
        assert_eq!(
            post(&mut conn, "/claims", &claim(id.as_str(), "a"), at(1.0)).0,
            200
        );
        let mut body = landed(id.as_str(), "a", "fixed");
        body["work"]["files"] = json!([file]);
        assert_eq!(
            post_with_test_paths(&mut conn, "/landed", &body, at(2.0), &config).0,
            200
        );
    }
    let days = findings::exploration_since(&conn, &start().day()).unwrap();
    assert_eq!(days.len(), 1);
    assert_eq!(days[0].family, "fuzz");
    assert_eq!(days[0].bugs, 1);
}
