//! Which product an unbuilt check names in a tip round's report.

use std::collections::HashMap;
use std::path::PathBuf;

use ignite_core::ids::CheckName;
use ignite_core::protocol::Outcome;

use crate::checks::{BUILD_FAILED, CheckRun, Failure};
use crate::tip::work::tip_runs;

fn name(check: &str) -> CheckName {
    CheckName::new(check).unwrap()
}

fn unbuilt(check: &str) -> Failure {
    Failure {
        check: name(check),
        outcome: Outcome::Incomplete,
        at: None,
        message: Some(BUILD_FAILED.to_owned()),
        output: None,
        reason: None,
    }
}

/// Two families in one round needed different products, and both builds failed. Every unbuilt
/// check was reported as waiting on the first failed product, so the second family's checks
/// and its `<family>:build` record named the wrong build.
#[test]
fn unbuilt_checks_name_the_product_their_family_needs() {
    let checks = ["rs:a", "rs:b", "ui:a"];
    let run = CheckRun {
        outcomes: checks
            .iter()
            .map(|c| (name(c), Outcome::Incomplete))
            .collect(),
        failures: checks.iter().map(|c| unbuilt(c)).collect(),
        build_failures: vec![
            ("deps".to_owned(), PathBuf::from("deps.log")),
            ("node".to_owned(), PathBuf::from("node.log")),
        ],
        ..CheckRun::default()
    };
    let needs = HashMap::from([
        ("rs".to_owned(), vec!["deps".to_owned()]),
        ("ui".to_owned(), vec!["other".to_owned(), "node".to_owned()]),
    ]);
    let runs = tip_runs(&run, &needs);
    let builds: Vec<_> = runs
        .iter()
        .map(|r| (r.build.as_deref(), r.output.as_deref()))
        .collect();
    assert_eq!(
        builds,
        [
            (Some("deps"), Some("deps.log")),
            (Some("deps"), Some("deps.log")),
            (Some("node"), Some("node.log")),
        ]
    );
}

/// A check that failed without saving an output file left `blame` nothing to report, and its
/// regression ticket ended with an empty "output" section. The failure's message stands in.
#[test]
fn a_failure_without_an_output_file_reports_its_message() {
    let failed = Failure {
        check: name("rs:a"),
        outcome: Outcome::Fail,
        at: None,
        message: Some("total_instructions 46575016 over 35465750".to_owned()),
        output: None,
        reason: None,
    };
    let run = CheckRun {
        outcomes: vec![(name("rs:a"), Outcome::Fail)],
        failures: vec![failed],
        ..CheckRun::default()
    };

    let runs = tip_runs(&run, &HashMap::new());

    assert_eq!(
        runs[0].output.as_deref(),
        Some("total_instructions 46575016 over 35465750")
    );
}
