//! Starting a turn once against a scripted program. The supervisor, CLI turn wrapper and
//! program run as child processes of this test binary. A local connection tells the test when
//! the program is running and holds it there until the test releases it.

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ignite_core::ids::SessionId;
use ignite_os::Native;
use ignite_os::proc::Processes;
use ignite_store::local::cli_slots;
use ignite_store::local::rows::{CliSlot, CliTurn};

use super::*;
use crate::supervise::SupervisedLauncher;
use crate::test_process::{ChildGate, hold_stdin};

const ROLES: &str = "clid::start::clid_start_tests::";

/// Run by the tests as a turn's supervisor: the launcher appends the spec file's path.
#[test]
#[ignore = "run by the other tests in this module as a supervisor"]
fn supervisor_role() {
    let Some(spec) = std::env::args()
        .next_back()
        .filter(|arg| arg.ends_with("spec.json"))
    else {
        return;
    };
    // libtest has printed `test <name> ... ` without a newline, and the launcher looks for the
    // supervisor's ready line on a line of its own.
    println!();
    let _ = std::io::stdout().flush();
    std::process::exit(crate::supervise::supervise(Path::new(&spec)));
}

/// Run by the tests as `ignite internal cli-turn`: its arguments after `--` are the
/// Ignite home and then the program to run.
#[test]
#[ignore = "run by the other tests in this module as ignite internal cli-turn"]
fn cli_turn_role() {
    if std::env::var_os(TURN_VARIABLE).is_none() {
        return;
    }
    let args: Vec<String> = std::env::args().collect();
    let Some(at) = args.iter().position(|arg| arg == "--") else {
        return;
    };
    let home = PathBuf::from(&args[at + 1]);
    std::process::exit(cli_turn(&home, &args[at + 2..]));
}

/// Run as the turn's program, with a count file and an optional gate address.
#[test]
#[ignore = "run by the other tests in this module as the CLI program"]
fn program_role() {
    let mut args = std::env::args().skip_while(|arg| arg != "--").skip(1);
    let Some(count) = args.next() else {
        return;
    };
    let mut count = OpenOptions::new()
        .create(true)
        .append(true)
        .open(count)
        .unwrap();
    writeln!(count, "ran").unwrap();
    if let Some(address) = args.next() {
        ChildGate::wait(&address);
    }
    println!("\n{{\"type\":\"result\"}}");
    std::process::exit(0);
}

fn role(name: &str) -> Vec<String> {
    vec![
        std::env::current_exe().unwrap().display().to_string(),
        "--exact".to_owned(),
        format!("{ROLES}{name}"),
        "--ignored".to_owned(),
        "--nocapture".to_owned(),
    ]
}

struct Fixture {
    home: PathBuf,
    session: SessionId,
    count: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Fixture {
        let home = ignite_core::test_files::temp_dir()
            .join(format!("ignite-clid-start-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        fs::create_dir_all(&home).unwrap();
        Fixture {
            count: home.join("count"),
            session: SessionId::new("cli-fake-0a1b2c3d").unwrap(),
            home,
        }
    }

    /// A turn launcher as a newly started daemon builds it.
    fn launcher(&self) -> TurnLauncher {
        let supervised =
            SupervisedLauncher::with_command(role("supervisor_role"), self.home.join("runs"));
        let mut cli_turn = role("cli_turn_role");
        cli_turn.extend(["--".to_owned(), self.home.display().to_string()]);
        TurnLauncher::with_command(
            Arc::new(supervised),
            cli_turn,
            self.home.clone(),
            self.home.clone(),
        )
    }

    /// The scripted program's command line, optionally held by a gate.
    fn command(&self, gate: Option<&ChildGate>) -> Vec<String> {
        let mut command = role("program_role");
        command.extend(["--".to_owned(), self.count.display().to_string()]);
        if let Some(gate) = gate {
            command.push(gate.address().to_string());
        }
        command
    }

    /// How many times the scripted program ran.
    fn runs(&self) -> usize {
        fs::read_to_string(&self.count).map_or(0, |text| text.lines().count())
    }

    fn run_dirs(&self) -> usize {
        fs::read_dir(self.home.join("runs")).map_or(0, |entries| {
            entries.filter_map(std::result::Result::ok).count()
        })
    }

    fn database(&self) -> Connection {
        let conn = ignite_store::local::open_in_memory().unwrap();
        cli_slots::put(
            &conn,
            &CliSlot {
                app: "fake".to_owned(),
                slot: 1,
                session: Some(self.session.to_string()),
                ticket: Some("T.1".to_owned()),
                state: "running".to_owned(),
                turn: 0,
                transcript: None,
                model: Some("m-1".to_owned()),
                effort: None,
                since: None,
                reservation: None,
                next_prompt: Some("go".to_owned()),
                wait_handle: None,
                resume_at: None,
                park_reason: None,
                interrupted_in_row: 0,
                block_sent: 0,
                backed_off: 0,
                fallback_prompt: None,
                handoff_note: None,
            },
        )
        .unwrap();
        conn
    }

    fn turn(&self, n: i64) -> CliTurn {
        CliTurn {
            session: self.session.to_string(),
            turn: n,
            app: "fake".to_owned(),
            slot: 1,
            ticket: Some("T.1".to_owned()),
            prompt: "go".to_owned(),
            state: "starting".to_owned(),
            run: None,
            started_at: Some("2026-11-20T12:00:00.000Z".to_owned()),
            ended_at: None,
            exit: None,
            end_kind: None,
            transcript: None,
            model: None,
            cli_version: None,
            cost_usd: None,
            lines_applied: 0,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.home);
    }
}

#[test]
fn a_crash_after_recording_a_turn_starts_it_once() {
    let f = Fixture::new("recorded");
    let mut conn = f.database();
    // The daemon recorded turn 1 and died before starting its supervisor.
    cli_slots::record_turn_start(&mut conn, &f.turn(1), &[]).unwrap();
    assert_eq!(recover(&f.home, &f.session, 1), Recovery::NotStarted);

    let restarted = f.launcher();
    let command = f.command(None);
    let (found, started) = restarted.ensure(&f.session, 1, &command).unwrap();
    assert_eq!(found, Recovery::NotStarted);
    let result = started.expect("the turn was started").wait();
    assert_eq!(result.exit, Some(0));
    assert_eq!(f.runs(), 1);

    let (found, started) = restarted.ensure(&f.session, 1, &command).unwrap();
    assert_eq!(found, Recovery::Ended { exit: Some(0) });
    assert!(started.is_none());
    assert_eq!(f.runs(), 1);
    assert!(
        find_run(&f.home, &f.session, 1)
            .unwrap()
            .dir()
            .join("output.keep")
            .exists(),
        "a turn's log is kept as long as a judged run's"
    );
}

#[test]
fn a_crash_after_starting_a_turn_takes_its_run_over() {
    let f = Fixture::new("started");
    let gate = ChildGate::new();
    let mut conn = f.database();
    let first = f.launcher();
    let started = first
        .start(&mut conn, &f.turn(1), &[], &f.command(Some(&gate)))
        .unwrap();
    assert_eq!(
        find_run(&f.home, &f.session, 1).as_ref(),
        Some(started.run())
    );
    // The daemon dies: nothing waits for its supervisor any more.
    drop(started);
    drop(first);
    gate.wait_started();

    let restarted = f.launcher();
    let (found, again) = restarted
        .ensure(&f.session, 1, &f.command(Some(&gate)))
        .unwrap();
    assert_eq!(found, Recovery::Running);
    assert!(again.is_none());
    assert!(
        turn_running(&f.home, &f.session),
        "the CLI program holds the session's turn lock"
    );

    gate.release();
    let result = find_run(&f.home, &f.session, 1).unwrap().wait();
    assert_eq!(result.exit, Some(0));
    assert!(
        fs::read_to_string(&result.output)
            .unwrap()
            .contains("{\"type\":\"result\"}")
    );
    assert_eq!(
        recover(&f.home, &f.session, 1),
        Recovery::Ended { exit: Some(0) }
    );
    assert!(!turn_running(&f.home, &f.session));
    assert_eq!((f.runs(), f.run_dirs()), (1, 1));
}

#[test]
fn a_second_start_of_a_turn_does_not_run_the_program_twice() {
    let f = Fixture::new("twice");
    let gate = ChildGate::new();
    let mut conn = f.database();
    let launcher = f.launcher();
    let command = f.command(Some(&gate));
    let started = launcher
        .start(&mut conn, &f.turn(1), &[], &command)
        .unwrap();
    gate.wait_started();

    // The record refuses a second start, which starts no supervisor.
    let refused = launcher.start(&mut conn, &f.turn(1), &[], &command);
    assert!(matches!(refused, Err(Error::Conflict(_))), "{refused:?}");
    assert_eq!(f.run_dirs(), 1);

    // A second supervisor started for the same turn runs nothing, whether it tries to take the lock
    // before or after the original turn exits.
    let duplicate = launcher.launch(&f.session, 1, &command).unwrap();
    gate.release();
    assert_eq!(started.wait().exit, Some(0));
    let duplicate = duplicate.wait();
    assert_eq!(duplicate.exit, Some(3));
    let output = fs::read_to_string(&duplicate.output).unwrap();
    assert!(output.contains("turn 1 ran already"), "{output}");
    assert_eq!(f.runs(), 1);
    assert_eq!(
        recover(&f.home, &f.session, 1),
        Recovery::Ended { exit: Some(0) }
    );

    // The next turn runs.
    let next = launcher
        .start(&mut conn, &f.turn(2), &[], &f.command(None))
        .unwrap();
    assert_eq!(next.wait().exit, Some(0));
    assert_eq!(f.runs(), 2);
}

#[test]
fn a_free_lock_with_no_result_is_reported_interrupted() {
    let f = Fixture::new("interrupted");
    // A run as a reboot leaves it: its supervisor is gone, so its lock is free and it wrote no
    // result, while a process of its command's group still runs.
    let dir = f.home.join("runs").join("00000000000000000001-1-0");
    fs::create_dir_all(&dir).unwrap();
    let spec = serde_json::json!({
        "args": ["fake-cli"],
        "cwd": f.home,
        "env": [[TURN_VARIABLE, format!("{}:1", f.session)]],
        "queued_s": 0.0,
    });
    fs::write(dir.join("spec.json"), spec.to_string()).unwrap();
    fs::write(dir.join("lock"), "").unwrap();
    let mut orphan = Native
        .spawn_group(&mut hold_stdin(&std::env::current_exe().unwrap()))
        .unwrap();
    fs::write(dir.join("group"), orphan.id.to_string()).unwrap();

    assert_eq!(recover(&f.home, &f.session, 2), Recovery::NotStarted);
    assert_eq!(recover(&f.home, &f.session, 1), Recovery::Interrupted);
    let status = orphan.child.wait().unwrap();
    assert!(!status.success(), "the orphan was ended: {status:?}");
}
