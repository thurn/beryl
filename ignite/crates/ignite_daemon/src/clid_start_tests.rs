//! Starting a turn once, against a scripted program in place of the CLI. The supervisor and
//! `ignite internal cli-turn` are this test binary run again with one of the ignored role tests
//! below, so the real supervisor, turn lock and image replacement are what runs.
//!
//! The scripted program adds a line to a count file each time it runs, so a test can see how
//! many times a turn ran. It then waits until its gate file exists: `/dev/null` lets it finish at
//! once, and a path in the test's home holds it running until the test creates the file. Before
//! it waits, it writes a line to its third argument: `/dev/null`, or a FIFO that the test reads to
//! know the program, and so the turn lock it inherited, is running.

#![cfg(unix)]

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use ignite_core::ids::SessionId;
use ignite_os::Native;
use ignite_os::proc::Processes;
use ignite_store::local::cli_slots;
use ignite_store::local::rows::{CliSlot, CliTurn};

use super::*;
use crate::supervise::SupervisedLauncher;

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

/// Run by the tests as `ignite internal cli-turn`: its arguments after `--nocapture` are the
/// Ignite home and then the program to run.
#[test]
#[ignore = "run by the other tests in this module as ignite internal cli-turn"]
fn cli_turn_role() {
    if std::env::var_os(TURN_VARIABLE).is_none() {
        return;
    }
    let args: Vec<String> = std::env::args().collect();
    let Some(at) = args.iter().position(|arg| arg == "--nocapture") else {
        return;
    };
    let home = PathBuf::from(&args[at + 1]);
    std::process::exit(cli_turn(&home, &args[at + 2..]));
}

struct Fixture {
    home: PathBuf,
    session: SessionId,
    count: PathBuf,
    script: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Fixture {
        let home = ignite_core::test_files::temp_dir()
            .join(format!("ignite-clid-start-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        fs::create_dir_all(&home).unwrap();
        let script = home.join("fake-cli");
        fs::write(
            &script,
            "#!/bin/sh\necho ran >> \"$1\"\necho started > \"$3\"\nwhile [ ! -e \"$2\" ]; do sleep 0.05; done\n\
             echo '{\"type\":\"result\"}'\n",
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        Fixture {
            count: home.join("count"),
            script,
            session: SessionId::new("cli-fake-0a1b2c3d").unwrap(),
            home,
        }
    }

    /// A turn launcher as a newly started daemon builds it.
    fn launcher(&self) -> TurnLauncher {
        let exe = std::env::current_exe().unwrap().display().to_string();
        let role = |name: &str| {
            vec![
                exe.clone(),
                "--exact".to_owned(),
                format!("{ROLES}{name}"),
                "--ignored".to_owned(),
                "--nocapture".to_owned(),
            ]
        };
        let supervised =
            SupervisedLauncher::with_command(role("supervisor_role"), self.home.join("runs"));
        let mut cli_turn = role("cli_turn_role");
        cli_turn.push(self.home.display().to_string());
        TurnLauncher::with_command(
            Arc::new(supervised),
            cli_turn,
            self.home.clone(),
            self.home.clone(),
        )
    }

    /// The scripted program's command line, held by `gate` until it is opened.
    fn command(&self, gate: &Path) -> Vec<String> {
        self.command_telling(gate, Path::new("/dev/null"))
    }

    /// [`Fixture::command`], with the program saying it started on the FIFO `started`.
    fn command_telling(&self, gate: &Path, started: &Path) -> Vec<String> {
        vec![
            self.script.display().to_string(),
            self.count.display().to_string(),
            gate.display().to_string(),
            started.display().to_string(),
        ]
    }

    /// A FIFO for [`Fixture::command_telling`] and [`wait_started`].
    fn started_fifo(&self) -> PathBuf {
        let fifo = self.home.join("started");
        assert!(
            Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        fifo
    }

    /// A gate that holds the scripted program until [`open_gate`] is called on it.
    fn gate(&self) -> PathBuf {
        self.home.join("gate")
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

/// Waits until the scripted program writes to the FIFO `started`.
fn wait_started(started: &Path) {
    assert_eq!(fs::read_to_string(started).unwrap(), "started\n");
}

/// Lets the scripted program held by `gate` finish.
fn open_gate(gate: &Path) {
    fs::write(gate, "").unwrap();
}

#[test]
fn a_crash_after_recording_a_turn_starts_it_once() {
    let f = Fixture::new("recorded");
    let mut conn = f.database();
    // The daemon recorded turn 1 and died before starting its supervisor.
    cli_slots::record_turn_start(&mut conn, &f.turn(1), &[]).unwrap();
    assert_eq!(recover(&f.home, &f.session, 1), Recovery::NotStarted);

    let restarted = f.launcher();
    let command = f.command(Path::new("/dev/null"));
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
    let gate = f.gate();
    let mut conn = f.database();
    let first = f.launcher();
    let fifo = f.started_fifo();
    let started = first
        .start(&mut conn, &f.turn(1), &[], &f.command_telling(&gate, &fifo))
        .unwrap();
    assert_eq!(
        find_run(&f.home, &f.session, 1).as_ref(),
        Some(started.run())
    );
    // The daemon dies: nothing waits for its supervisor any more.
    drop(started);
    drop(first);
    wait_started(&fifo);

    let restarted = f.launcher();
    let (found, again) = restarted.ensure(&f.session, 1, &f.command(&gate)).unwrap();
    assert_eq!(found, Recovery::Running);
    assert!(again.is_none());
    assert!(
        turn_running(&f.home, &f.session),
        "the CLI program holds the session's turn lock"
    );

    open_gate(&gate);
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
    let gate = f.gate();
    let mut conn = f.database();
    let launcher = f.launcher();
    let fifo = f.started_fifo();
    let command = f.command_telling(&gate, &fifo);
    let started = launcher
        .start(&mut conn, &f.turn(1), &[], &command)
        .unwrap();
    wait_started(&fifo);

    // The record refuses a second start, which starts no supervisor.
    let refused = launcher.start(&mut conn, &f.turn(1), &[], &command);
    assert!(matches!(refused, Err(Error::Conflict(_))), "{refused:?}");
    assert_eq!(f.run_dirs(), 1);

    // A second supervisor started for the same turn anyway waits for the turn lock, then finds
    // the turn ran and runs nothing.
    let duplicate = launcher.launch(&f.session, 1, &command).unwrap();
    open_gate(&gate);
    assert_eq!(started.wait().exit, Some(0));
    let duplicate = duplicate.wait();
    assert_eq!(duplicate.exit, Some(3));
    let output = fs::read_to_string(&duplicate.output).unwrap();
    assert!(output.contains("waits for it"), "{output}");
    assert!(output.contains("turn 1 ran already"), "{output}");
    assert_eq!(f.runs(), 1);
    assert_eq!(
        recover(&f.home, &f.session, 1),
        Recovery::Ended { exit: Some(0) }
    );

    // The next turn runs.
    let next = launcher
        .start(
            &mut conn,
            &f.turn(2),
            &[],
            &f.command(Path::new("/dev/null")),
        )
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
        .spawn_group(Command::new("sleep").arg("600"))
        .unwrap();
    fs::write(dir.join("group"), orphan.id.to_string()).unwrap();

    assert_eq!(recover(&f.home, &f.session, 2), Recovery::NotStarted);
    assert_eq!(recover(&f.home, &f.session, 1), Recovery::Interrupted);
    let status = orphan.child.wait().unwrap();
    assert!(!status.success(), "the orphan was ended: {status:?}");
}
