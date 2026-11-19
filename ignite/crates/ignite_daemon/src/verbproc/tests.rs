//! Verb processes against the fake daemon (`ignite_core::fakes::FakeHost`), an in-memory
//! database and run directories written by the test in place of supervisors.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rusqlite::Connection;

use ignite_core::config::Config;
use ignite_core::fakes::{FakeBuilds, FakeClock, FakeGit, FakeHost, FakeHub, FakeLauncher};
use ignite_core::ids::{Handle, MachineId, TreeId};
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::protocol::host::{Hello, PROTOCOL_VERSION, StepUpdate};
use ignite_core::protocol::{Class, Usage};
use ignite_core::time::Timestamp;
use ignite_core::traits::{BuildFor, BuildResult, CommandSpec, Result};
use ignite_store::local::handles as handle_store;

use super::{
    Assembled, Link, Outcome, Process, Remote, RemoteBuilds, RemoteLauncher, Timing, alive, hold,
};
use crate::handles::{Handles, lock};
use crate::server::{Context, Verb};
use crate::steps;
use crate::supervise::ResultFile;

pub(super) const T0: Timestamp = Timestamp::from_millis(1_800_000_000_000);

/// A verb named `test` that does what the test gives it.
pub(super) struct FakeVerb(pub(super) Box<dyn Fn(&Context) -> Reply + Send + Sync>);

impl Verb for FakeVerb {
    fn name(&self) -> &'static str {
        "test"
    }

    fn run(&self, ctx: &Context, _req: &Request) -> Reply {
        (self.0)(ctx)
    }
}

/// A folder of the test's own: the Ignite home, with the fake daemon's runs inside.
pub(super) struct Scratch {
    pub(super) home: PathBuf,
}

impl Scratch {
    pub(super) fn new(name: &str) -> Scratch {
        let home =
            std::env::temp_dir().join(format!("ignite-verbproc-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        fs::create_dir_all(home.join("runs")).unwrap();
        Scratch { home }
    }

    pub(super) fn runs(&self) -> PathBuf {
        self.home.join("runs")
    }

    /// Writes the result of the command the fake daemon starts for `handle`'s `counter`th
    /// request, as its supervisor would once the command ended with `exit`.
    fn finish_run(&self, handle: &Handle, counter: u64, exit: i32) {
        let dir = self.runs().join(format!("{handle}-{counter}"));
        fs::create_dir_all(&dir).unwrap();
        let result = ResultFile {
            exit: Some(exit),
            output: dir.join("output.log"),
            usage: Usage::default(),
            hung: false,
        };
        fs::write(
            dir.join("result.json"),
            serde_json::to_vec(&result).unwrap(),
        )
        .unwrap();
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.home);
    }
}

pub(super) fn timing() -> Timing {
    Timing {
        retry_for: Duration::from_secs(10),
        pause: Duration::from_millis(1),
        poll: Duration::from_millis(10),
    }
}

pub(super) fn context() -> Context {
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
        [machine.local]
        ticket_trees = 1
        landing_trees = 1
    "#;
    Context {
        config: Config::parse("ignite.toml", text).unwrap(),
        repo: PathBuf::from("/repo"),
        machine: MachineId::new("m-1").unwrap(),
        clock: Arc::new(FakeClock::new(T0)),
        hub: Arc::new(FakeHub::default()),
        git: Arc::new(FakeGit::default()),
        launcher: Arc::new(FakeLauncher::default()),
        builds: Arc::new(FakeBuilds::default()),
        session: None,
        tree: None,
    }
}

/// A database holding a running `test` handle, as the daemon records one before it starts the
/// verb process.
fn running_test() -> (Arc<Mutex<Connection>>, Handle) {
    let db = Arc::new(Mutex::new(ignite_store::local::open_in_memory().unwrap()));
    let handles = Handles::open(Arc::new(FakeClock::new(T0)), db.clone()).unwrap();
    let handle = handles.create_for("test", None, None, Some(&request()));
    (db, handle)
}

/// The request of `ignite test rust`, from the main checkout.
pub(super) fn request() -> Request {
    Request {
        verb: "test".to_owned(),
        args: vec!["rust".to_owned()],
        cwd: PathBuf::from("/repo"),
        session: None,
        wait_s: 0,
        pid: None,
    }
}

fn process(
    scratch: &Scratch,
    db: &Arc<Mutex<Connection>>,
    handle: &Handle,
    host: &Arc<FakeHost>,
) -> Process {
    host.set_runs_folder(scratch.runs());
    Process {
        handle: handle.clone(),
        home: scratch.home.clone(),
        db: db.clone(),
        host: host.clone(),
        executable: "1a2b3c4".to_owned(),
        clock: Arc::new(FakeClock::new(T0)),
        timing: timing(),
    }
}

/// The verb with the remote launcher and builds of the process.
pub(super) fn assembled(remote: &Arc<Remote>, verb: FakeVerb) -> Assembled {
    let mut ctx = context();
    ctx.launcher = Arc::new(RemoteLauncher(remote.clone()));
    ctx.builds = Arc::new(RemoteBuilds {
        remote: remote.clone(),
        has_built: Arc::new(|_: &TreeId, _: &str| true),
    });
    Assembled {
        verb: Arc::new(verb),
        ctx,
    }
}

pub(super) fn spec(args: &[&str]) -> CommandSpec {
    CommandSpec {
        args: args.iter().map(|&a| a.to_owned()).collect(),
        cwd: PathBuf::from("/repo"),
        env: Vec::new(),
        class: Class::Agent,
        cores: 2,
        resources: Vec::new(),
        grows: false,
        tree: None,
        ticket: None,
        hang_limit_s: None,
    }
}

pub(super) fn row(
    db: &Arc<Mutex<Connection>>,
    handle: &Handle,
) -> ignite_store::local::rows::Handle {
    handle_store::get(&lock(db), handle.as_str())
        .unwrap()
        .unwrap()
}

fn reply_of(outcome: Outcome) -> Reply {
    match outcome {
        Outcome::Finished(reply) => reply,
        other => panic!("the verb did not finish: {other:?}"),
    }
}

/// Waits until `done` holds, for at most ten seconds.
pub(super) fn until(done: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

/// The acceptance test: a `test` verb that runs two commands and a build runs to the end in a
/// verb process, and its reply lands in the handle's row, while the daemon drops the first
/// attempt of every call as a restart in the middle of each would. Each request still starts
/// one command or one build.
#[test]
fn a_verb_runs_to_the_end_across_dropped_calls_and_its_reply_lands_in_the_row() {
    let scratch = Scratch::new("end");
    let (db, handle) = running_test();
    scratch.finish_run(&handle, 1, 0);
    scratch.finish_run(&handle, 3, 2);
    let host = Arc::new(FakeHost::default());
    host.drop_each_once(true);
    host.wait_runs(2);
    let outcome = process(&scratch, &db, &handle, &host).run(|remote, verb, req| {
        assert_eq!(
            (verb, req.args.as_slice()),
            ("test", ["rust".to_owned()].as_slice())
        );
        Ok(assembled(
            remote,
            FakeVerb(Box::new(|ctx| {
                steps::step("listing family rust");
                let first = ctx.launcher.run(&spec(&["cargo", "test"]));
                let who = BuildFor {
                    class: Class::Agent,
                    ticket: None,
                    session: None,
                };
                let tree = TreeId::new("tree-01").unwrap();
                let built = ctx.builds.ensure_for(&tree, "rust-tests", &[], &who);
                let second = ctx.launcher.run(&spec(&["cargo", "clippy"]));
                Reply::text(
                    Exit::Success,
                    format!(
                        "{:?} {:?} {}",
                        first.exit,
                        second.exit,
                        built == BuildResult::Current
                    ),
                )
            })),
        ))
    });
    let reply = reply_of(outcome);
    assert_eq!(reply.text, "Some(0) Some(2) true");

    let stored = row(&db, &handle);
    assert_eq!(stored.state, "done");
    assert_eq!(stored.exit, Some(0));
    let kept: Reply = serde_json::from_str(stored.reply.as_deref().unwrap()).unwrap();
    assert_eq!(kept, reply);
    assert_eq!(stored.pid, Some(i64::from(std::process::id())));
    assert_eq!(stored.executable.as_deref(), Some("1a2b3c4"));
    assert_eq!(stored.protocol, Some(i64::from(PROTOCOL_VERSION)));

    let started: Vec<String> = host
        .started()
        .iter()
        .map(|(run, _)| run.request_id.to_string())
        .collect();
    assert_eq!(started, [format!("{handle}#1"), format!("{handle}#3")]);
    let builds: Vec<String> = host
        .builds()
        .iter()
        .map(|(build, _)| build.request_id.to_string())
        .collect();
    assert_eq!(builds, [format!("{handle}#2")]);
    assert_eq!(host.finished(), std::slice::from_ref(&handle));
    let verbs = host.verbs();
    assert!(verbs.contains(&"host-hello".to_owned()), "{verbs:?}");
    assert!(
        host.steps()
            .iter()
            .any(|step| step.step.as_deref() == Some("listing family rust")),
        "the step reached the daemon: {:?}",
        host.steps()
    );
    // Every call was dropped once and repeated after a hello: more hellos than the first.
    assert!(host.hellos().len() > 1);
}

/// A stop asked at the daemon reaches the verb, which unwinds; the row keeps it as `stopped`.
#[test]
fn a_stop_asked_at_the_daemon_stops_the_verb() {
    let scratch = Scratch::new("stop");
    let (db, handle) = running_test();
    let host = Arc::new(FakeHost::default());
    let asker = host.clone();
    let outcome = process(&scratch, &db, &handle, &host).run(|remote, _, _| {
        Ok(assembled(
            remote,
            FakeVerb(Box::new(move |_| {
                asker.request_stop().unwrap();
                let heard = until(steps::stop_requested);
                Reply::text(Exit::Success, format!("heard {heard}"))
            })),
        ))
    });
    let reply = reply_of(outcome);
    assert!(reply.text.starts_with("stopped"), "{}", reply.text);
    assert_eq!(row(&db, &handle).state, "stopped");
    assert_eq!(host.finished(), [handle]);
}

/// Refusing to stop asks the daemon first: a stop the daemon already took makes the refusal
/// fail at once, before the mirror has polled; a refusal the daemon accepts makes it refuse
/// `ignite stop` until the verb allows it again.
#[test]
fn refusing_to_stop_is_decided_by_the_daemon() {
    let scratch = Scratch::new("refuse");
    let (db, handle) = running_test();
    let host = Arc::new(FakeHost::default());
    let seen = host.clone();
    let outcome = process(&scratch, &db, &handle, &host).run(|remote, _, _| {
        Ok(assembled(
            remote,
            FakeVerb(Box::new(move |_| {
                assert!(steps::refuse_stop("pushing to staging"));
                assert_eq!(seen.refusing().as_deref(), Some("pushing to staging"));
                assert_eq!(seen.request_stop(), Err("pushing to staging".to_owned()));
                steps::allow_stop();
                assert_eq!(seen.refusing(), None);
                seen.request_stop().unwrap();
                let refused = steps::refuse_stop("pushing to staging");
                Reply::text(Exit::Success, format!("refused again: {refused}"))
            })),
        ))
    });
    assert!(reply_of(outcome).text.starts_with("stopped"));
    assert_eq!(row(&db, &handle).state, "stopped");
}

/// A panic in the verb leaves a failed reply in the row, and the daemon is told.
#[test]
fn a_panicking_verb_leaves_a_failed_reply() {
    let scratch = Scratch::new("panic");
    let (db, handle) = running_test();
    let host = Arc::new(FakeHost::default());
    let outcome = process(&scratch, &db, &handle, &host).run(|remote, _, _| {
        Ok(assembled(
            remote,
            FakeVerb(Box::new(|_| panic!("a bug in the verb"))),
        ))
    });
    let reply = reply_of(outcome);
    assert_eq!(reply.exit, Exit::Usage);
    assert!(reply.text.contains("panicked"), "{}", reply.text);
    let stored = row(&db, &handle);
    assert_eq!((stored.state.as_str(), stored.exit), ("done", Some(1)));
    assert_eq!(host.finished(), [handle]);
}

/// A verb that cannot be built leaves a failed reply instead of a running row.
#[test]
fn a_verb_that_cannot_be_assembled_leaves_a_failed_reply() {
    let scratch = Scratch::new("assemble");
    let (db, handle) = running_test();
    let host = Arc::new(FakeHost::default());
    let outcome = process(&scratch, &db, &handle, &host).run(|_, _, _| {
        Err(ignite_core::traits::Error::Invalid(
            "ignite.toml is broken".to_owned(),
        ))
    });
    assert!(reply_of(outcome).text.contains("ignite.toml is broken"));
    assert_eq!(row(&db, &handle).state, "done");
}

/// While one verb process holds a handle's lock, it is alive and a second one runs nothing.
#[test]
fn a_second_process_for_a_running_handle_runs_nothing() {
    let scratch = Scratch::new("twice");
    let (db, handle) = running_test();
    assert!(!alive(&scratch.home, &handle));
    let held = hold(&scratch.home, &handle).unwrap().unwrap();
    assert!(alive(&scratch.home, &handle));
    let host = Arc::new(FakeHost::default());
    let unused = |_: &Arc<Remote>, _: &str, _: &Request| -> Result<Assembled> {
        panic!("a second process must not assemble the verb")
    };
    let outcome = process(&scratch, &db, &handle, &host).run(unused);
    assert_eq!(outcome, Outcome::AlreadyRunning);
    drop(held);
    // A child that another test forks shares the lock's open file until it runs `exec`, so the
    // lock can stay held for a moment after `drop`.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while alive(&scratch.home, &handle) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(!alive(&scratch.home, &handle));
    assert_eq!(row(&db, &handle).state, "running");
}

/// The daemon's probe of a verb process ([`alive`]) takes the lock shared for a moment. A new
/// verb process that tries the lock during that moment waits for it instead of exiting as if
/// another process ran the handle, which once made the daemon run a landing again from the
/// start.
#[test]
fn a_new_process_waits_out_the_daemons_probe_of_its_lock() {
    let scratch = Scratch::new("probed");
    let (_db, handle) = running_test();
    let folder = super::folder(&scratch.home, &handle);
    fs::create_dir_all(&folder).unwrap();
    let probe = fs::File::create(folder.join("lock")).unwrap();
    probe.try_lock_shared().unwrap();
    let mut probe = Some(probe);
    let mut pauses = 0;
    let held = super::hold_trying(&scratch.home, &handle, 3, &mut || {
        pauses += 1;
        // The probe ends during the first pause.
        probe.take();
    })
    .unwrap();
    assert!(held.is_some(), "the lock is taken once the probe lets go");
    assert_eq!(pauses, 1);

    // A lock that stays held is still refused after the tries.
    let mut pauses = 0;
    let second = super::hold_trying(&scratch.home, &handle, 3, &mut || pauses += 1).unwrap();
    assert!(second.is_none());
    assert_eq!(pauses, 2);
}

/// A handle that has finished, or that the daemon no longer runs, is not run.
#[test]
fn a_handle_that_is_not_running_is_not_run() {
    let scratch = Scratch::new("done");
    let (db, handle) = running_test();
    Handles::open(Arc::new(FakeClock::new(T0)), db.clone())
        .unwrap()
        .finish(&handle, Reply::text(Exit::Success, "done"));
    let host = Arc::new(FakeHost::default());
    let unused = |_: &Arc<Remote>, _: &str, _: &Request| -> Result<Assembled> {
        panic!("a finished handle must not be run")
    };
    assert_eq!(
        process(&scratch, &db, &handle, &host).run(unused),
        Outcome::NotRunning
    );

    let (db, handle) = running_test();
    host.set_running(false);
    let outcome = process(&scratch, &db, &handle, &host).run(unused);
    assert!(reply_of(outcome).text.starts_with("stopped"));
    assert_eq!(row(&db, &handle).state, "stopped");
}

/// A daemon that stays away longer than the retry window fails the command, with a note that
/// says why, instead of blocking the verb for ever.
#[test]
fn a_daemon_away_past_the_retry_window_fails_the_command() {
    let scratch = Scratch::new("away");
    let (db, handle) = running_test();
    let host = Arc::new(FakeHost::default());
    let gone = host.clone();
    let mut process = process(&scratch, &db, &handle, &host);
    process.timing.retry_for = Duration::from_millis(50);
    let outcome = process.run(|remote, _, _| {
        Ok(assembled(
            remote,
            FakeVerb(Box::new(move |ctx| {
                gone.set_down(true);
                let result = ctx.launcher.run(&spec(&["cargo", "test"]));
                let note = fs::read_to_string(&result.output).unwrap_or_default();
                gone.set_down(false);
                Reply::text(Exit::Success, format!("{:?} {note}", result.exit))
            })),
        ))
    });
    let reply = reply_of(outcome);
    assert!(
        reply.text.starts_with("None cannot reach the daemon"),
        "{}",
        reply.text
    );
    assert!(host.started().is_empty());
    assert_eq!(row(&db, &handle).state, "done");
}

#[test]
fn a_verb_sends_its_step_again_when_the_daemon_restarted_without_failing_a_call() {
    let (_, handle) = running_test();
    let host = Arc::new(FakeHost::default());
    host.set_daemon_started("2026-11-18T10:00:00.000Z");
    let link = Link::new(
        host.clone(),
        Hello {
            handle: handle.clone(),
            protocol: PROTOCOL_VERSION,
            executable: "1a2b3c4".to_owned(),
            pid: 42,
        },
        timing(),
    );
    link.hello().unwrap();
    let update = StepUpdate {
        handle,
        step: Some("waiting for the express lock".to_owned()),
        waiting: Vec::new(),
        class: None,
        ticket: None,
    };
    link.step(&update).unwrap();
    link.poll().unwrap();
    assert_eq!((host.hellos().len(), host.steps().len()), (1, 1));

    // The daemon restarts: nothing fails, but its start instant changes.
    host.set_daemon_started("2026-11-18T11:00:00.000Z");
    link.poll().unwrap();
    assert_eq!(host.hellos().len(), 2);
    assert_eq!(host.steps().last(), Some(&update));
    link.poll().unwrap();
    assert_eq!(host.hellos().len(), 2, "once per restart");
}

#[test]
fn the_lock_lives_in_the_handle_folder() {
    let handle = Handle::new("L-6").unwrap();
    assert_eq!(
        super::folder(Path::new("/home"), &handle),
        Path::new("/home/verbs/L-6")
    );
}
