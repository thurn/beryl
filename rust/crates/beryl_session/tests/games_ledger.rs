use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use beryl_session::bugs::{self, BugReport, Images, PageInfo};
use beryl_session::protocol::{OutcomeView, Side};
use beryl_session::{CandidateIndex, GameUpdate, Seat, Sessions, StartGame, Status, StopsSetting};
use serde_json::{Value, json};

fn next_decision(views: &Receiver<GameUpdate>) -> GameUpdate {
    loop {
        let update = views
            .recv_timeout(Duration::from_secs(120))
            .expect("the session appears hung: it emitted no view within 120 s");
        if matches!(
            update.view.status,
            Status::Deciding { .. } | Status::Over { .. }
        ) {
            return update;
        }
    }
}

fn report() -> BugReport {
    BugReport {
        description: "Test report".to_owned(),
        ai: false,
        snapshot: None,
        region: None,
        page: PageInfo {
            url: "/#/game".to_owned(),
            width: 1440.0,
            height: 900.0,
            scale: 1.0,
        },
        ui_log: json!([]),
        updates: Vec::new(),
        spec: None,
        spec_viewer: false,
    }
}

#[test]
fn completed_games_are_appended_and_counted_in_reports() {
    let directory = tempfile::tempdir().unwrap();
    let ledger = directory.path().join("games.jsonl");
    let reports = directory.path().join("bugs");
    let (send, views) = channel();
    let sessions =
        Sessions::new(None, move |update| send.send(update).unwrap()).with_ledger(ledger.clone());
    let mut start = StartGame {
        seed: 11,
        decks: [
            vec![(7, "Island".to_owned())],
            vec![(8, "Island".to_owned())],
        ],
        human: Seat::P0,
        opponent: "random".to_owned(),
        stops: StopsSetting::SMART,
    };
    sessions.start_game(&start, None).unwrap();
    let finished = loop {
        let update = next_decision(&views);
        match update.view.status {
            Status::Deciding { decision } => {
                sessions.choose(decision.id, CandidateIndex(0)).unwrap();
            }
            Status::Over { .. } => break update.view,
            _ => unreachable!(),
        }
    };
    let result = match finished.status {
        Status::Over {
            outcome: OutcomeView::Win {
                winner: Side::You, ..
            },
        } => "win",
        Status::Over {
            outcome:
                OutcomeView::Win {
                    winner: Side::Opponent,
                    ..
                },
        } => "loss",
        other => panic!("expected a game ending by drawing from an empty library: {other:?}"),
    };
    let entries = || {
        std::fs::read_to_string(&ledger)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>()
    };
    let first = entries();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0]["result"], result);
    assert_eq!(first[0]["agent"], "random");
    assert_eq!(first[0]["turns"], finished.turn.number);
    assert!(
        first[0]["commit"]
            .as_str()
            .is_some_and(|commit| !commit.is_empty())
    );
    assert_eq!(first[0]["date"].as_str().unwrap().len(), 10);

    let concede = |start: &StartGame| {
        let session = sessions.start_game(start, None).unwrap();
        assert!(matches!(
            next_decision(&views).view.status,
            Status::Deciding { .. }
        ));
        sessions.concede(session).unwrap();
        assert!(matches!(
            next_decision(&views).view.status,
            Status::Over { .. }
        ));
    };
    concede(&start);
    concede(&start);
    let played = entries();
    assert_eq!(played.len(), 3);
    assert_eq!(played[1]["result"], "concession");
    assert_eq!(played[2]["result"], "concession");

    let report = report();
    let write = || {
        let saved = bugs::write(&reports, &report, &Images::default(), &sessions).unwrap();
        std::fs::read_to_string(std::path::Path::new(&saved.path).join("report.md")).unwrap()
    };
    let first = write();
    assert!(first.contains("games_against_agent: 3\n"));
    assert!(first.contains("games_since_last_report: 3\n"));
    start.opponent = "greedy".to_owned();
    concede(&start);
    let second = write();
    assert!(second.contains("games_against_agent: 1\n"));
    assert!(second.contains("games_since_last_report: 1\n"));
}
