use std::fs;
use std::path::PathBuf;

use crate::monitor::run_once;
use crate::monitor::tests::{FakeMachine, at, home};

use super::*;

fn claude() -> JusticiarApp {
    JusticiarApp {
        name: "claude".to_owned(),
        program: CliProgram {
            program: "claude".to_owned(),
            args: vec!["--dangerously-skip-permissions".to_owned()],
            dialect: None,
        },
        model: "claude-opus".to_owned(),
        checkout: PathBuf::from("/repo"),
    }
}

/// A run in which the daemon is down, the repair was asked for and the machine runs Claude in
/// CLI mode.
fn down() -> Seen {
    Seen {
        answered: false,
        needed: true,
        turn_running: false,
        app: Some("claude".to_owned()),
        transcript: None,
    }
}

#[test]
fn a_turn_starts_only_when_the_repair_was_asked_for_and_the_daemon_does_not_answer() {
    let cases = [
        (down(), true),
        (
            Seen {
                needed: false,
                ..down()
            },
            false,
        ),
        (
            Seen {
                answered: true,
                ..down()
            },
            false,
        ),
        (
            Seen {
                app: None,
                ..down()
            },
            false,
        ),
    ];
    for (seen, starts) in cases {
        let mut incident = Incident::default();
        let step = decide(&mut incident, &seen, at(0.0));
        assert_eq!(
            step == Some(Step::Start { turn: 1 }),
            starts,
            "{seen:?}: {step:?}"
        );
        if !starts {
            assert_eq!(incident, Incident::default(), "{seen:?}");
        }
    }
}

#[test]
fn no_second_turn_starts_while_the_lock_is_held() {
    let mut incident = Incident::default();
    assert_eq!(
        decide(&mut incident, &down(), at(0.0)),
        Some(Step::Start { turn: 1 })
    );
    let held = Seen {
        turn_running: true,
        ..down()
    };
    for run in 1..10 {
        assert_eq!(
            decide(&mut incident, &held, at(f64::from(run) * 120.0)),
            None
        );
    }
    assert_eq!(incident.turns, 1);
}

#[test]
fn a_free_lock_resumes_the_transcript_and_six_turns_end_the_tries() {
    let mut incident = Incident::default();
    let mut steps = Vec::new();
    for run in 0..10 {
        let seen = Seen {
            transcript: (run > 0).then(|| format!("t-{run}")),
            ..down()
        };
        steps.push(decide(&mut incident, &seen, at(f64::from(run) * 120.0)));
    }
    assert_eq!(steps[0], Some(Step::Start { turn: 1 }));
    assert_eq!(
        steps[1],
        Some(Step::Resume {
            turn: 2,
            transcript: "t-1".to_owned()
        })
    );
    assert_eq!(
        steps[5],
        Some(Step::Resume {
            turn: 6,
            transcript: "t-5".to_owned()
        })
    );
    assert_eq!(steps[6], Some(Step::Exhausted));
    assert_eq!(steps[7..], [None, None, None]);
    assert_eq!(incident.turns, MAX_TURNS);
}

#[test]
fn two_answered_runs_in_a_row_end_the_incident_and_one_does_not() {
    let mut incident = Incident::default();
    decide(&mut incident, &down(), at(0.0));
    let answered = Seen {
        answered: true,
        needed: false,
        ..down()
    };
    assert_eq!(decide(&mut incident, &answered, at(120.0)), None);
    // Down again: the count of answered runs starts over, and so does nothing else.
    let held = Seen {
        turn_running: true,
        ..down()
    };
    assert_eq!(decide(&mut incident, &held, at(240.0)), None);
    assert_eq!(decide(&mut incident, &answered, at(360.0)), None);
    assert_eq!(incident.turns, 1);
    assert_eq!(decide(&mut incident, &answered, at(480.0)), Some(Step::End));
    assert_eq!(incident, Incident::default());
}

#[test]
fn a_machine_with_no_cli_app_starts_nothing_and_writes_nothing() {
    let home = home();
    let machine = FakeMachine::new(false, false);
    for run in 0..12 {
        run_once(&home, &machine, at(f64::from(run) * 120.0));
    }
    assert!(home.root.join("justiciar-needed.json").exists());
    assert!(machine.turns.borrow().is_empty());
    assert!(!home.root.join("monitor").exists());
}

#[test]
fn a_daemon_that_stays_down_gets_a_turn_then_a_resume_and_recovery_ends_the_incident() {
    let home = home();
    let mut machine = FakeMachine::new(false, false);
    machine.cli_app = Some(JusticiarApp {
        name: "fake".to_owned(),
        program: CliProgram {
            dialect: Some("claude".to_owned()),
            ..claude().program
        },
        ..claude()
    });
    // Failing since 0 s; the repair is asked for at 960 s, and the turn starts in that run.
    let mut started_at = None;
    for run in 0..9 {
        let done = run_once(&home, &machine, at(f64::from(run) * 120.0));
        if done
            .iter()
            .any(|line| line.contains("started the headless justiciar"))
        {
            started_at.get_or_insert(run);
        }
    }
    assert_eq!(started_at, Some(8));
    let first = machine.turns.borrow()[0].clone();
    assert_eq!(first.session, SESSION);
    assert_eq!(first.cwd, PathBuf::from("/repo"));
    assert_eq!(first.lock, home.root.join("monitor").join("justiciar.lock"));
    assert_eq!(first.log, home.root.join("monitor").join("justiciar-1.log"));
    let command = first.command.join(" ");
    assert!(
        command.starts_with(
            "claude -p --dangerously-skip-permissions --model claude-opus --effort high"
        ),
        "{command}"
    );
    assert!(!command.contains("--resume"), "{command}");
    let prompt = first.command.last().unwrap();
    assert!(prompt.contains("JUSTICIAR"), "the guide is in the prompt");
    assert!(prompt.contains("daemon log, last 50 lines"), "{prompt}");
    assert!(prompt.contains("kickstarting it did not help"), "{prompt}");

    // The turn holds the lock: no second one starts.
    run_once(&home, &machine, at(1080.0));
    assert_eq!(machine.turns.borrow().len(), 1);

    // It ended and named its transcript; the daemon is still down, so the next run resumes it.
    fs::write(
        &first.log,
        "{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"t-abc\",\"model\":\"claude-opus\"}\n",
    )
    .unwrap();
    machine.turn_lock.set(false);
    let done = run_once(&home, &machine, at(1200.0));
    assert_eq!(
        done,
        ["resumed the headless justiciar with fake, turn 2 of 6"]
    );
    let second = machine.turns.borrow()[1].clone();
    assert!(second.command.join(" ").contains("--resume t-abc"));
    assert_eq!(
        second.log,
        home.root.join("monitor").join("justiciar-2.log")
    );
    assert!(home.root.join("monitor").join("justiciar.json").exists());

    // The daemon answers in two runs in a row: the incident is over and its file is gone, while
    // the running turn is left alone.
    machine.running = true;
    machine.answers = true;
    run_once(&home, &machine, at(1320.0));
    assert!(home.root.join("monitor").join("justiciar.json").exists());
    let done = run_once(&home, &machine, at(1440.0));
    assert!(
        done.iter().any(|line| line.contains("incident is over")),
        "{done:?}"
    );
    assert!(!home.root.join("monitor").join("justiciar.json").exists());
    assert_eq!(machine.turns.borrow().len(), 2);
}

#[test]
fn a_configured_provider_can_start_a_justiciar_without_slots() {
    let text = r#"
        [repository]
        remote = "origin"
        staging = "staging"
        master = "master"
        trees = "trees"
        checks = "checks"
        [hub]
        url = "http://localhost:1"
        project = "test"
        [gate]
        threshold = 0.01
        cap = 10
        [machine.jarvis]
        ticket_trees = 4
        landing_trees = 1
        [machine.jarvis.pool.codex]
        mode = "cli"
        slots = 2
        [pool.cli.claude]
        program = "claude"
        [pool.cli.codex]
        program = "codex"
    "#;
    let config = ignite_core::config::Config::parse("ignite.toml", text).unwrap();
    let checkout = Path::new("/repo");
    let picked = first_cli_app(&config, checkout).unwrap();
    assert_eq!(picked.name, "claude");
}

#[cfg(unix)]
#[test]
fn a_turn_holds_its_lock_and_a_held_lock_refuses_a_second() {
    let dir = home().root;
    let lock = dir.join("justiciar.lock");
    let log = dir.join("justiciar-1.log");
    assert!(!lock_held(&lock));
    let command = [
        "sh".to_owned(),
        "-c".to_owned(),
        "echo out; echo err >&2".to_owned(),
    ];
    assert_eq!(run_turn(&lock, &log, &command), 0);
    let written = fs::read_to_string(&log).unwrap();
    assert!(
        written.contains("out") && written.contains("err"),
        "{written}"
    );

    let holder = fs::OpenOptions::new().write(true).open(&lock).unwrap();
    holder.lock().unwrap();
    assert!(lock_held(&lock));
    let other = dir.join("justiciar-2.log");
    assert_eq!(run_turn(&lock, &other, &command), 3);
    assert!(!other.exists());
}
