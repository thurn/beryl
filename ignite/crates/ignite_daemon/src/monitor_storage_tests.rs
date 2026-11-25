use std::fs;
use std::path::PathBuf;

use crate::monitor::tests::{FakeMachine, at, home};
use crate::monitor::{MonitorState, run_once};

use super::*;

/// Restart after 30 minutes stuck, and force a restart that hangs.
fn allowed() -> RebootPolicy {
    RebootPolicy {
        after_s: Some(1800),
        force: Some(vec!["force-restart".to_owned(), "now".to_owned()]),
    }
}

/// Runs the monitor every two minutes from `from` seconds to before `to`, returning the log of
/// what the machine was asked to do in that time.
fn runs(home: &Home, machine: &FakeMachine, from: u32, to: u32) -> Vec<String> {
    let before = machine.log().len();
    for seconds in (from..to).step_by(120) {
        run_once(home, machine, at(f64::from(seconds)));
    }
    machine.log()[before..].to_vec()
}

fn state(home: &Home) -> MonitorState {
    serde_json::from_slice(&fs::read(home.root.join("monitor.json")).unwrap()).unwrap()
}

#[test]
fn a_stuck_trees_drive_stops_kickstarts_and_asks_for_a_restart_naming_the_drive() {
    let home = home();
    let mut machine = FakeMachine::stuck(RebootPolicy::default());
    machine.running = false;
    machine.answers = false;
    let done = run_once(&home, &machine, at(0.0));
    assert_eq!(
        machine.log(),
        ["notify Ignite needs a restart", "report /trees"]
    );
    assert!(
        done.iter()
            .any(|line| line.contains("asked for a repair: the trees drive at /trees")),
        "{done:?}"
    );
    let request: serde_json::Value =
        serde_json::from_slice(&fs::read(home.root.join("justiciar-needed.json")).unwrap())
            .unwrap();
    let reason = request["reason"].as_str().unwrap();
    assert!(reason.contains("/trees"), "{reason}");
    assert!(reason.contains("needs a restart"), "{reason}");
    assert_eq!(state(&home).storage_stuck_since, Some(at(0.0)));

    // Ten hours stuck with no policy: never a kickstart and never a restart, and the request
    // is repeated hourly.
    let later = runs(&home, &machine, 120, 36_000);
    assert!(
        later
            .iter()
            .all(|line| line == "notify Ignite needs a restart"),
        "{later:?}"
    );
    assert_eq!(later.len(), 9);
}

#[test]
fn the_hub_hears_of_a_stuck_drive_once_and_again_after_it_was_unreachable() {
    let home = home();
    let machine = FakeMachine::stuck(RebootPolicy::default());
    machine.hub_takes_reports.set(false);
    let done = run_once(&home, &machine, at(0.0));
    assert!(
        done.iter().any(|line| line.contains("cannot tell the hub")),
        "{done:?}"
    );
    machine.hub_takes_reports.set(true);
    assert_eq!(runs(&home, &machine, 120, 600), ["report /trees"]);
}

#[test]
fn a_drive_stuck_past_the_limit_restarts_the_machine_and_then_forces_it_once() {
    let home = home();
    let machine = FakeMachine::stuck(allowed());
    machine.booted.set(Some(at(-86_400.0)));
    runs(&home, &machine, 0, 1800);
    assert!(!machine.log().contains(&"restart".to_owned()));
    // Stuck since 0 s: the run at 1800 s restarts, and the machine is still up 10 minutes later.
    assert_eq!(runs(&home, &machine, 1800, 1920), ["restart"]);
    assert!(runs(&home, &machine, 1920, 2400).is_empty());
    assert_eq!(runs(&home, &machine, 2400, 2520), ["run force-restart now"]);
    // Neither again within six hours.
    let later = runs(&home, &machine, 2520, 1800 + 21_600);
    assert!(
        later
            .iter()
            .all(|line| line == "notify Ignite needs a restart"),
        "{later:?}"
    );
    assert_eq!(
        runs(&home, &machine, 1800 + 21_600, 1920 + 21_600),
        ["restart"]
    );
}

#[test]
fn a_machine_that_restarted_is_not_forced_and_waits_six_hours_to_restart_again() {
    let home = home();
    let machine = FakeMachine::stuck(allowed());
    machine.booted.set(Some(at(-86_400.0)));
    assert_eq!(
        runs(&home, &machine, 0, 1920),
        ["notify Ignite needs a restart", "report /trees", "restart"]
    );
    // It restarted at 2000 s, and the drive is stuck again. The monitor remembers the restart.
    machine.booted.set(Some(at(2000.0)));
    let later = runs(&home, &machine, 2040, 1800 + 21_600);
    assert!(
        later
            .iter()
            .all(|line| line == "notify Ignite needs a restart"),
        "{later:?}"
    );
}

#[test]
fn without_a_force_command_only_the_ordinary_restart_runs() {
    let home = home();
    let machine = FakeMachine::stuck(RebootPolicy {
        force: None,
        ..allowed()
    });
    let log = runs(&home, &machine, 0, 7200);
    assert_eq!(log.iter().filter(|line| *line == "restart").count(), 1);
    assert!(!log.iter().any(|line| line.starts_with("run ")), "{log:?}");
}

#[test]
fn a_drive_that_answers_again_lets_the_daemon_be_kickstarted_and_keeps_the_last_restart() {
    let home = home();
    let mut machine = FakeMachine::stuck(allowed());
    machine.running = false;
    machine.answers = false;
    assert!(runs(&home, &machine, 0, 1920).contains(&"restart".to_owned()));
    assert!(!machine.log().contains(&"kickstart".to_owned()));

    machine.storage_answers.set(true);
    assert_eq!(runs(&home, &machine, 2040, 2160), ["kickstart"]);
    let after = state(&home);
    assert_eq!(after.storage_stuck_since, None);
    assert_eq!(after.storage_reboot_at, Some(at(1800.0)));

    // Stuck again an hour later: a fresh incident is asked for and reported, but the restart
    // waits out six hours from the last one.
    machine.storage_answers.set(false);
    let again = runs(&home, &machine, 5640, 5760 + 1800);
    assert_eq!(
        again,
        ["notify Ignite needs a restart", "report /trees"],
        "{again:?}"
    );
}

#[test]
fn the_headless_justiciar_is_told_the_drive_is_stuck() {
    let home = home();
    let mut machine = FakeMachine::stuck(RebootPolicy::default());
    machine.running = false;
    machine.answers = false;
    machine.cli_app = Some(crate::monitor::justiciar::JusticiarApp {
        name: "claude".to_owned(),
        program: ignite_core::config::CliProgram {
            program: "claude".to_owned(),
            args: Vec::new(),
            dialect: None,
        },
        model: "claude-opus".to_owned(),
        checkout: PathBuf::from("/repo"),
    });
    run_once(&home, &machine, at(0.0));
    let turns = machine.turns.borrow();
    let prompt = turns[0].command.last().unwrap();
    assert!(
        prompt.contains("because the trees drive at /trees has not answered"),
        "{prompt}"
    );
}

#[test]
fn the_roots_the_daemon_records_are_what_the_monitor_reads() {
    let home = home();
    assert!(roots(&home).is_empty());
    let recorded = vec![PathBuf::from("/b/trees"), PathBuf::from("/a/trees")];
    record_roots(&home, [recorded.clone(), recorded].concat()).unwrap();
    assert_eq!(
        roots(&home),
        [PathBuf::from("/a/trees"), PathBuf::from("/b/trees")]
    );
}

#[test]
fn the_hub_report_names_the_machine_and_the_drive() {
    let stuck = StuckStorage {
        root: PathBuf::from("E:\\trees"),
        since: at(0.0),
    };
    let report = hub_report(&stuck, "cygnus");
    assert_eq!(report.host, "cygnus");
    assert_eq!(report.root, "E:\\trees");
    assert_eq!(report.since, at(0.0));
}
