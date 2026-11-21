//! Tests that a regression handed to a CLI slot resumes the transcript of the worker whose
//! ticket caused it, while that ticket landed within 24 hours, and that a resume the CLI refuses
//! is followed by one fresh start of the same ticket.

use ignite_core::cli::{Action, TurnEnd};
use ignite_core::fakes::ScriptedTurn;

use super::*;

const REGRESSION_PROMPT: &str = "R.1 is a regression caused by your last ticket: x. Run \
                                 `ignite start R.1 --reservation c0de` and fix it.";
const FRESH_PROMPT: &str = "Run `ignite start R.1 --reservation c0de` and follow what it prints.";

/// Lands T.1 from slot 1, whose turn named the transcript `t-1`, so the slot is free again.
fn land_t1(rig: &Rig) -> SessionId {
    rig.push(clean("landed"));
    let session = rig.begin();
    rig.host.holders.lock().unwrap().clear();
    rig.then(Action::Free);
    rig.daemon.step().unwrap();
    session
}

/// Begins the regression R.1, caused by T.1, on slot 1 with the regression's own model and
/// effort.
fn begin_regression(rig: &Rig) -> SessionId {
    begin_regression_with(rig, "c0de")
}

/// [`begin_regression`] under the reservation code `code`.
fn begin_regression_with(rig: &Rig, code: &str) -> SessionId {
    let resume = Resume {
        culprit: ticket(),
        prompt: REGRESSION_PROMPT.to_owned(),
    };
    let reservation = Reservation {
        provider: None,
        code: code.to_owned(),
        ..reservation("R.1", 1)
    };
    rig.daemon
        .begin(&reservation, "m-1", Some("low"), Some(&resume), false)
        .unwrap()
}

fn has_resume(command: &CommandSpec) -> bool {
    command.args.iter().any(|arg| arg == "--resume")
}

#[test]
fn a_regression_resumes_the_culprits_transcript_within_24_hours() {
    let rig = Rig::new();
    let culprit = land_t1(&rig);
    rig.clock.advance(23.0 * 3600.0);
    let session = begin_regression(&rig);
    assert_ne!(session, culprit);
    let starts = rig.turns.starts();
    assert_eq!(starts.len(), 2);
    let (started, turn, command) = &starts[1];
    assert_eq!((started.as_str(), *turn), (session.as_str(), 1));
    assert!(command.args.ends_with(&[
        "--resume".to_owned(),
        "t-1".to_owned(),
        REGRESSION_PROMPT.to_owned()
    ]));
    assert!(
        command
            .env
            .contains(&("IGNITE_SESSION".to_owned(), session.to_string()))
    );
    let row = rig.row(&session);
    assert_eq!(row.effort.as_deref(), Some("low"));
    assert_eq!(row.ticket.as_deref(), Some("R.1"));
    // The record is used once: a second regression of the same culprit starts fresh.
    let other = begin_regression_with(&rig, "f00d");
    let starts = rig.turns.starts();
    assert_eq!(starts[2].0, other.as_str());
    assert!(!has_resume(&starts[2].2));
}

#[test]
fn a_regression_whose_culprit_landed_over_24_hours_ago_starts_fresh() {
    let rig = Rig::new();
    land_t1(&rig);
    rig.clock.advance(25.0 * 3600.0);
    begin_regression(&rig);
    let starts = rig.turns.starts();
    assert_eq!(starts.len(), 2);
    assert!(!has_resume(&starts[1].2));
    assert_eq!(
        starts[1].2.args.last().map(String::as_str),
        Some(FRESH_PROMPT)
    );
}

#[test]
fn a_refused_resume_is_followed_by_one_fresh_start() {
    let rig = Rig::new();
    land_t1(&rig);
    // The CLI exits before its first line, as it does for a transcript it cannot find.
    rig.push(ScriptedTurn {
        lines: Vec::new(),
        exit: Some(1),
        hung: false,
    });
    rig.keep_running(2);
    let session = begin_regression(&rig);
    rig.daemon.step().unwrap();
    let starts = rig.turns.starts();
    assert_eq!(starts.len(), 3);
    assert!(has_resume(&starts[1].2));
    let (started, turn, command) = &starts[2];
    assert_eq!((started.as_str(), *turn), (session.as_str(), 2));
    assert!(!has_resume(command));
    assert_eq!(command.args.last().map(String::as_str), Some(FRESH_PROMPT));
    // The refused turn reached neither `decide` nor a hold: only T.1's end was decided.
    assert_eq!(rig.asked().len(), 1);
    assert!(lock(&rig.effects.holds).is_empty());
    let row = rig.row(&session);
    assert_eq!(row.fallback_prompt, None);
    // The fresh turn runs on; nothing starts again.
    rig.daemon.step().unwrap();
    assert_eq!(rig.turns.starts().len(), 3);
    assert!(matches!(
        lock(&rig.effects.ends).as_slice(),
        [TurnEnd::Clean, TurnEnd::Hold { .. }]
    ));
}
