use ignite_core::protocol::hub::{
    CheckFix, FixOutcome, FixSource, FlakyReport, FoundBy, RegressionReport,
};
use ignite_core::protocol::tickets::{BatchRequest, Operation};
use ignite_store::hub::{self, batch, checks, findings, fix_outcomes};

use super::tests::{at, claim, file, landed, post, post_with_test_paths, register, start};
use super::{Connection, Request, Timestamp, json};
use crate::upkeep::{Upkeep, filing::UPKEEP, regressions};

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

fn history(conn: &mut Connection, query: (&str, &str), now: Timestamp) -> Vec<CheckFix> {
    let request = Request {
        method: "GET".to_owned(),
        path: "/checks/fix-outcomes".to_owned(),
        query: vec![(query.0.to_owned(), query.1.to_owned())],
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
            let fixes = history(&mut conn, ("check", check.as_str()), at(4.0));
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
            let fixes = history(&mut conn, ("check", a.check.as_str()), at(8.0));
            assert_eq!(fixes.len(), 2);
            assert_eq!(fixes[0].outcome, FixOutcome::None);
            assert_eq!(fixes[0].fixing_commit, None);
            assert_eq!(fixes[1].outcome, FixOutcome::Product);
            let cutoff = at(8.0).plus_seconds(fix_outcomes::HISTORY_DAYS * 86_400.0);
            assert!(history(&mut conn, ("check", a.check.as_str()), cutoff).is_empty());
        }
    }
    let now = at(8.0).plus_seconds(fix_outcomes::HISTORY_DAYS * 86_400.0);
    let products = history(&mut conn, ("outcome", "product"), now);
    let checks: Vec<_> = products.iter().map(|fix| fix.check.as_str()).collect();
    assert_eq!(checks, ["unit:a0", "unit:b0"]);
    assert!(
        products
            .iter()
            .all(|fix| fix.outcome == FixOutcome::Product)
    );
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
                command: format!("m-1:{commit}"),
                output: "flaked".to_owned(),
            };
            checks::record_flaky(&conn, &report, start()).unwrap();
        }
    }
    let outcome = Upkeep::new(None).unwrap().pass(&mut conn, at(1.0)).unwrap();
    assert_eq!(outcome.tickets_filed.len(), 2);
    let ticket = &outcome.tickets_filed[0];
    assert_eq!(ticket, &outcome.tickets_filed[1]);
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
fn an_ordinary_batch_records_outcomes_only_on_the_repair_ticket() {
    let mut conn = hub::open_in_memory().unwrap();
    register(&mut conn);
    for check in ["unit:fixed", "unit:unresolved"] {
        let report = FlakyReport {
            check: check.parse().unwrap(),
            hash: "a".repeat(64).parse().unwrap(),
            commit: "aaaa".parse().unwrap(),
            machine: "m-1".parse().unwrap(),
            always: false,
            command: "m-1:run-1".to_owned(),
            output: "flaked".to_owned(),
        };
        checks::register(&conn, check).unwrap();
        checks::record_flaky(&conn, &report, start()).unwrap();
    }
    let pass = Upkeep::new(None).unwrap().pass(&mut conn, at(1.0)).unwrap();
    let [triage] = pass.tickets_filed.try_into().expect("one pending batch");
    let request = serde_json::from_value(json!({"ops": [
        {"op":"create","fields":{"title":"Fix the isolated cause","labels":["flaky"],
                                "metadata":{"ignite_checks":["unit:fixed"]}}},
        {"op":"close","id":triage,"reason":"Filed one repair; other evidence unresolved"}
    ]}))
    .unwrap();
    let batch::BatchOutcome::Applied(reply) =
        batch::apply(&mut conn, &request, "test", "m-1", at(2.0)).unwrap()
    else {
        panic!("triage did not close");
    };
    assert!(
        fix_outcomes::between(&conn, start(), at(3.0), None)
            .unwrap()
            .is_empty()
    );
    let repair = reply.results[0].id.as_str();
    assert_eq!(
        post(&mut conn, "/claims", &claim(repair, "a"), at(3.0)).0,
        200
    );
    assert_eq!(
        post(&mut conn, "/landed", &landed(repair, "a", "fixed"), at(4.0)).0,
        200
    );
    let [fix]: [CheckFix; 1] = fix_outcomes::between(&conn, start(), at(5.0), None)
        .unwrap()
        .try_into()
        .expect("one repaired check");
    assert_eq!(fix.check.as_str(), "unit:fixed");
    assert_eq!(fix.ticket.as_str(), repair);
    assert_eq!(fix.outcome, FixOutcome::Product);
    assert_eq!(
        ignite_store::hub::check_state::flaky_ticket(&conn, "unit:unresolved").unwrap(),
        Some(triage)
    );
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

#[test]
fn a_test_only_close_follows_up_each_registered_check_and_leaves_protected_checks_on() {
    use ignite_core::config::Rules;
    use ignite_core::protocol::hub::CheckState;
    use ignite_store::hub::{check_state, tickets};

    let mut conn = hub::open_in_memory().unwrap();
    register(&mut conn);
    let names = ["ordinary", "protected", "unknown"];
    for name in &names[..2] {
        checks::register(&conn, &format!("unit:{name}")).unwrap();
    }
    let mut regression_ticket = None;
    for name in names {
        let mut report = report(name);
        report.output = format!("{name} failed with a brittle assertion");
        let (ticket, _) = regressions::file(&mut conn, UPKEEP, &report, at(1.0)).unwrap();
        assert_eq!(regression_ticket.get_or_insert(ticket.clone()), &ticket);
    }
    let regression_ticket = regression_ticket.unwrap();
    assert_eq!(
        post(
            &mut conn,
            "/claims",
            &claim(&regression_ticket, "a"),
            at(2.0)
        )
        .0,
        200
    );
    let mut body = landed(&regression_ticket, "a", "fixed");
    body["work"]["files"] = json!(["tests/regression.rs"]);
    assert_eq!(
        post_with_test_paths(
            &mut conn,
            "/landed",
            &body,
            at(3.0),
            &["tests/**".to_owned()]
        )
        .0,
        200
    );

    let mut upkeep = Upkeep::new(None).unwrap().with_rules(Rules {
        protected: vec!["unit:protected".to_owned()],
        ..Rules::default()
    });
    let result = upkeep.pass(&mut conn, at(4.0)).unwrap();
    assert_eq!(result.checks_switched_off, ["unit:ordinary"]);
    assert_eq!(result.tickets_filed.len(), 2);
    let remedy = &result.tickets_filed[0];
    assert_eq!(&result.tickets_filed[1], remedy);
    let ordinary = tickets::load(&conn, remedy).unwrap().unwrap();
    assert!(
        ordinary
            .body
            .contains("ordinary failed with a brittle assertion")
    );
    assert!(ordinary.body.contains("Fixing commit 3a9e4c1"));
    assert!(
        ordinary
            .body
            .contains("Delete unit:ordinary or redesign it")
    );
    let comments = tickets::comments(&conn, remedy).unwrap();
    assert_eq!(comments.len(), 1);
    assert!(
        comments[0]
            .text
            .contains("protected failed with a brittle assertion")
    );
    assert!(
        comments[0]
            .text
            .contains("It stays on because it is protected.")
    );
    assert_eq!(
        check_state::state(&conn, "unit:protected").unwrap(),
        Some(CheckState::On)
    );
    assert_eq!(check_state::state(&conn, "unit:unknown").unwrap(), None);
    assert_eq!(
        check_state::flaky_ticket(&conn, "unit:ordinary").unwrap(),
        Some(remedy.clone())
    );

    checks::set_state(&conn, "unit:ordinary", CheckState::On).unwrap();
    let result = upkeep.pass(&mut conn, at(5.0)).unwrap();
    assert!(result.tickets_filed.is_empty());
    assert!(result.checks_switched_off.is_empty());
    assert_eq!(
        check_state::state(&conn, "unit:ordinary").unwrap(),
        Some(CheckState::On)
    );
}

#[test]
fn the_culprit_tickets_own_test_update_and_scenario_changes_do_not_switch_checks_off() {
    use ignite_core::protocol::hub::CheckState;
    use ignite_store::hub::check_state;

    let mut conn = hub::open_in_memory().unwrap();
    register(&mut conn);
    for (name, path) in [
        ("own-update", "tests/regression.rs"),
        ("scenario", "src/rules/scenarios.rs"),
    ] {
        let mut report = report(name);
        checks::register(&conn, report.check.as_str()).unwrap();
        let (ticket, _) = regressions::file(&mut conn, UPKEEP, &report, at(1.0)).unwrap();
        if name == "own-update" {
            report.caused_by = Some(ticket.parse().unwrap());
            regressions::file(&mut conn, UPKEEP, &report, at(1.0)).unwrap();
        }
        assert_eq!(
            post(&mut conn, "/claims", &claim(&ticket, "a"), at(2.0)).0,
            200
        );
        let mut body = landed(&ticket, "a", "fixed");
        body["work"]["files"] = json!([path]);
        assert_eq!(
            post_with_test_paths(
                &mut conn,
                "/landed",
                &body,
                at(3.0),
                &["tests/**".to_owned()]
            )
            .0,
            200
        );
        let result = Upkeep::new(None).unwrap().pass(&mut conn, at(4.0)).unwrap();
        assert!(result.tickets_filed.is_empty());
        assert!(result.checks_switched_off.is_empty());
        assert_eq!(
            check_state::state(&conn, report.check.as_str()).unwrap(),
            Some(CheckState::On)
        );
    }
}
