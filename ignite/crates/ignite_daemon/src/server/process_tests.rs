//! Long verbs in verb processes, against the fakes: the daemon's side is a real [`Server`] with
//! its [`Host`], the verb process runs on a thread of the test (`crate::verbproc::Process`) and
//! reaches whichever server the test calls the running daemon, and commands are run directories
//! the test writes and finishes in place of supervisors.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rusqlite::Connection;

use ignite_core::config::Config;
use ignite_core::fakes::{FakeBuilds, FakeClock, FakeGit, FakeHub, FakeLauncher};
use ignite_core::host::HostClient;
use ignite_core::ids::{Commit, Handle, MachineId, TreeId};
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::protocol::host::{
    Ack, AllowStop, BorrowCores, BorrowCoresReply, Build, BuildReply, Finished, Hello, HelloReply,
    HostCall, LeaseLanding, LeaseLandingReply, Poll, REQUEST_VARIABLE, RefuseStop, RefuseStopReply,
    ReleaseLanding, RequestId, ReturnCores, Run, RunReply, Selected, SelectedReply, StepUpdate,
    StopState, decode_reply, encode_request,
};
use ignite_core::protocol::{Class, Usage};
use ignite_core::time::Timestamp;
use ignite_core::traits::{CommandSpec, Error, Result};
use ignite_store::local::handles as handle_store;

use super::{Processes, VerbChild, VerbStarter, is_restarting};
use crate::handles::{Handles, Waited, lock};
use crate::host::{Host, HostParts, LandingTrees, RunStarter, Timing as HostTiming};
use crate::server::{Context, Registry, Server, Verb};
use crate::steps::{Kind, Steps, Work};
use crate::stop::EndVerbProcess;
use crate::supervise::{ResultFile, SpecFile};
use crate::trees::TreeLease;
use crate::verbproc::{Assembled, Outcome, Process, RemoteLauncher, Timing as ProcessTiming};

const T0: Timestamp = Timestamp::from_millis(1_800_000_000_000);

/// A folder of the test's own: the Ignite home, with the runs inside.
struct Scratch {
    home: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Scratch {
        let home = ignite_core::test_files::temp_dir()
            .join(format!("ignite-process-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        fs::create_dir_all(home.join("runs")).unwrap();
        Scratch { home }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.home);
    }
}

fn context() -> Context {
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

fn request(verb: &str, wait_s: u32) -> Request {
    Request {
        verb: verb.to_owned(),
        args: Vec::new(),
        cwd: PathBuf::from("/repo"),
        session: None,
        wait_s,
        pid: None,
    }
}

fn spec(args: &[&str]) -> CommandSpec {
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
        reserve: None,
    }
}

/// Waits until `done` holds, for at most ten seconds.
fn until(done: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

/// Commands that run until the test finishes them: each run directory's `lock` is held, as its
/// supervisor would hold it, until [`Commands::finish_all`]. The directories outlive a
/// restart of the daemon, as on disk.
#[derive(Default)]
struct Commands {
    runs: PathBuf,
    begun: Mutex<Vec<(String, PathBuf)>>,
    locks: Mutex<BTreeMap<PathBuf, File>>,
}

impl Commands {
    fn begun(&self) -> usize {
        lock(&self.begun).len()
    }

    /// Writes each run's result, exit 0, and frees its lock.
    fn finish_all(&self) {
        let mut locks = lock(&self.locks);
        for dir in locks.keys() {
            let result = ResultFile {
                exit: Some(0),
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
        locks.clear();
    }
}

impl RunStarter for Commands {
    fn begin(
        &self,
        spec: &CommandSpec,
        _since: Timestamp,
        _cancelled: &dyn Fn() -> bool,
    ) -> io::Result<Option<PathBuf>> {
        let mut begun = lock(&self.begun);
        let dir = self.runs.join(format!("run-{}", begun.len() + 1));
        fs::create_dir_all(&dir)?;
        let held = File::create(dir.join("lock"))?;
        held.lock()?;
        let file = SpecFile {
            args: spec.args.clone(),
            cwd: spec.cwd.clone(),
            env: spec.env.clone(),
            queued_s: 0.0,
            jobserver: None,
            build_jobs: None,
            hang_limit_s: None,
            grant: None,
        };
        fs::write(dir.join("spec.json"), serde_json::to_vec(&file)?)?;
        let request = spec
            .env
            .iter()
            .find(|(name, _)| name == REQUEST_VARIABLE)
            .map(|(_, value)| value.clone())
            .unwrap_or_default();
        begun.push((request, dir.clone()));
        lock(&self.locks).insert(dir.clone(), held);
        Ok(Some(dir))
    }

    fn find(&self, request: &RequestId) -> Option<PathBuf> {
        let id = request.to_string();
        lock(&self.begun)
            .iter()
            .find(|(found, _)| *found == id)
            .map(|(_, dir)| dir.clone())
    }

    fn runs_of(&self, handle: &Handle) -> Vec<PathBuf> {
        lock(&self.begun)
            .iter()
            .filter(|(id, dir)| {
                RequestId::of_handle(id, handle) && !dir.join("result.json").exists()
            })
            .map(|(_, dir)| dir.clone())
            .collect()
    }

    fn end(&self, _run: &Path) {}

    fn kill(&self, _run: &Path) {}

    fn waiting_for(&self, spec: &CommandSpec) -> String {
        format!("{} cores", spec.cores)
    }
}

/// No landing tree is ever free.
struct NoTrees;

impl LandingTrees for NoTrees {
    fn lease(&self, _: &str, _: &Handle, _: &Commit, _: usize) -> Result<Option<TreeLease>> {
        Ok(None)
    }

    fn release(&self, _: &Handle, _: &TreeId, _: Option<&Commit>) -> Result<()> {
        Ok(())
    }
}

/// The verb process's way to the daemon: the server the test says is running, or none while it
/// restarts, when every call fails as an unreachable socket does.
#[derive(Default)]
struct Switch {
    server: Mutex<Option<Arc<Server>>>,
    /// How many answers to `host-poll` and `host-step` have told a verb process to stop.
    stops: AtomicUsize,
}

impl Switch {
    fn set(&self, server: Option<Arc<Server>>) {
        *lock(&self.server) = server;
    }

    fn call<C: HostCall>(&self, call: &C) -> Result<C::Reply> {
        let server = lock(&self.server).clone();
        let Some(server) = server else {
            return Err(Error::Unreachable("the daemon is restarting".to_owned()));
        };
        decode_reply::<C>(server.handle(&encode_request(call, PathBuf::from("/home"))))
    }

    fn count_stop(&self, answer: Result<StopState>) -> Result<StopState> {
        if answer.as_ref().is_ok_and(|state| state.stop) {
            self.stops.fetch_add(1, Ordering::SeqCst);
        }
        answer
    }
}

impl HostClient for Switch {
    fn hello(&self, call: &Hello) -> Result<HelloReply> {
        self.call(call)
    }
    fn run(&self, call: &Run) -> Result<RunReply> {
        self.call(call)
    }
    fn build(&self, call: &Build) -> Result<BuildReply> {
        self.call(call)
    }
    fn lease_landing(&self, call: &LeaseLanding) -> Result<LeaseLandingReply> {
        self.call(call)
    }
    fn release_landing(&self, call: &ReleaseLanding) -> Result<Ack> {
        self.call(call)
    }
    fn step(&self, call: &StepUpdate) -> Result<StopState> {
        self.count_stop(self.call(call))
    }
    fn refuse_stop(&self, call: &RefuseStop) -> Result<RefuseStopReply> {
        self.call(call)
    }
    fn allow_stop(&self, call: &AllowStop) -> Result<Ack> {
        self.call(call)
    }
    fn poll(&self, call: &Poll) -> Result<StopState> {
        self.count_stop(self.call(call))
    }
    fn finished(&self, call: &Finished) -> Result<Ack> {
        self.call(call)
    }
    fn borrow_cores(&self, call: &BorrowCores) -> Result<BorrowCoresReply> {
        self.call(call)
    }
    fn return_cores(&self, call: &ReturnCores) -> Result<Ack> {
        self.call(call)
    }
    fn selected(&self, call: &Selected) -> Result<SelectedReply> {
        self.call(call)
    }
}

/// A verb that runs one command, `cargo <verb>`, and answers with its exit. It counts its runs.
struct OneCommand {
    name: &'static str,
    ran: Arc<AtomicUsize>,
}

impl Verb for OneCommand {
    fn name(&self) -> &str {
        self.name
    }

    fn run(&self, ctx: &Context, _req: &Request) -> Reply {
        self.ran.fetch_add(1, Ordering::SeqCst);
        crate::steps::step(format!("running cargo {}", self.name));
        let result = ctx.launcher.run(&spec(&["cargo", self.name]));
        Reply::text(Exit::Success, format!("{}: {:?}", self.name, result.exit))
    }
}

/// A verb process finished, or not.
struct ThreadChild(Option<JoinHandle<Outcome>>);

impl VerbChild for ThreadChild {
    fn exited(&mut self) -> bool {
        self.0.as_ref().is_none_or(JoinHandle::is_finished)
    }
}

/// Starts each verb process on a thread of the test, reaching the daemon through `switch`; or,
/// with `dies`, starts a process that ends at once without doing anything.
struct Threads {
    home: PathBuf,
    db: Arc<Mutex<Connection>>,
    switch: Arc<Switch>,
    dies: bool,
    started: AtomicUsize,
    /// How many times each verb ran in a verb process.
    ran: BTreeMap<&'static str, Arc<AtomicUsize>>,
}

impl VerbStarter for Threads {
    fn start(&self, handle: &Handle) -> io::Result<Box<dyn VerbChild>> {
        self.started.fetch_add(1, Ordering::SeqCst);
        if self.dies {
            return Ok(Box::new(ThreadChild(None)));
        }
        let process = Process {
            handle: handle.clone(),
            home: self.home.clone(),
            db: self.db.clone(),
            host: self.switch.clone(),
            executable: "87631b5".to_owned(),
            clock: Arc::new(FakeClock::new(T0)),
            timing: ProcessTiming {
                retry_for: Duration::from_secs(20),
                pause: Duration::from_millis(5),
                poll: Duration::from_millis(10),
            },
        };
        let ran = self.ran.clone();
        let thread = std::thread::spawn(move || {
            process.run(|remote, name, _req| {
                let (&name, ran) = ran
                    .iter()
                    .find(|(verb, _)| **verb == name)
                    .expect("a verb the test knows");
                let mut ctx = context();
                ctx.launcher = Arc::new(RemoteLauncher(remote.clone()));
                Ok(Assembled {
                    verb: Arc::new(OneCommand {
                        name,
                        ran: ran.clone(),
                    }),
                    ctx,
                })
            })
        });
        Ok(Box::new(ThreadChild(Some(thread))))
    }
}

/// What lasts across a restart of the daemon: the database, the home, the runs, the verb
/// processes and how often each verb ran in the daemon itself.
struct Machine {
    scratch: Scratch,
    db: Arc<Mutex<Connection>>,
    commands: Arc<Commands>,
    switch: Arc<Switch>,
    threads: Arc<Threads>,
    in_daemon: BTreeMap<&'static str, Arc<AtomicUsize>>,
    /// What the daemons it starts compare a landing's build with theirs through, if anything.
    git: Mutex<Option<Arc<FakeGit>>>,
    /// What ends a verb process that does not stop, and how long it has: by default an ender
    /// that ends nothing, since the native one would signal the test's own process group.
    ender: Mutex<(Arc<dyn EndVerbProcess>, Duration)>,
}

/// Ends no verb process, as when none runs the handle.
struct EndsNothing;

impl EndVerbProcess for EndsNothing {
    fn end(&self, _handle: &Handle) -> std::result::Result<Option<String>, String> {
        Ok(None)
    }
}

const VERBS: [&str; 3] = ["land", "test", "run"];

impl Machine {
    fn new(name: &str, dies: bool) -> Machine {
        let scratch = Scratch::new(name);
        let db = Arc::new(Mutex::new(ignite_store::local::open_in_memory().unwrap()));
        let switch = Arc::new(Switch::default());
        let counters = || {
            VERBS
                .iter()
                .map(|verb| (*verb, Arc::new(AtomicUsize::new(0))))
                .collect::<BTreeMap<_, _>>()
        };
        let threads = Arc::new(Threads {
            home: scratch.home.clone(),
            db: db.clone(),
            switch: switch.clone(),
            dies,
            started: AtomicUsize::new(0),
            ran: counters(),
        });
        Machine {
            commands: Arc::new(Commands {
                runs: scratch.home.join("runs"),
                ..Commands::default()
            }),
            scratch,
            db,
            switch,
            threads,
            in_daemon: counters(),
            git: Mutex::new(None),
            ender: Mutex::new((Arc::new(EndsNothing), crate::stop::STOP_WAIT)),
        }
    }

    /// A daemon starting on the machine, as after a restart: a new server and host over the
    /// same database and runs, with a registry of its own.
    fn daemon(&self) -> Arc<Server> {
        self.daemon_at(None)
    }

    /// [`Machine::daemon`], whose new verb processes would start from the Ignite commit
    /// `version`, as a daemon restarted into that release.
    fn daemon_at(&self, version: Option<&str>) -> Arc<Server> {
        let steps = Arc::new(Steps::new(Arc::new(FakeClock::new(T0))));
        let host = Host::with_timing(
            HostParts {
                starter: self.commands.clone(),
                builds: Arc::new(FakeBuilds::default()),
                landing: Arc::new(NoTrees),
                db: self.db.clone(),
                steps: steps.clone(),
                clock: Arc::new(FakeClock::new(T0)),
            },
            HostTiming {
                block: Duration::from_millis(200),
                abandon: Duration::from_secs(5),
                watch: Duration::from_millis(20),
                grace: Duration::from_millis(50),
            },
        );
        let mut registry = Registry::default();
        for (name, ran) in &self.in_daemon {
            let verb = Box::new(OneCommand {
                name,
                ran: ran.clone(),
            });
            // `run` is not run again after a restart; `land` and `test` are.
            if *name == "run" {
                registry.register(verb);
            } else {
                registry.register_resumable(verb);
            }
        }
        host.register(&mut registry);
        let (ender, stop_wait) = lock(&self.ender).clone();
        let processes = Processes {
            host,
            starter: self.threads.clone(),
            home: self.scratch.home.clone(),
            verbs: VERBS
                .iter()
                .map(|v| (*v).to_owned())
                .collect::<BTreeSet<_>>(),
            watch: Duration::from_millis(20),
            version: version.map(str::to_owned),
            git: lock(&self.git)
                .clone()
                .map(|git| git as Arc<dyn ignite_core::traits::Git>),
            ender: Some(ender),
            stop_wait,
        };
        let server = Server::with_processes(
            context(),
            registry,
            Some(self.db.clone()),
            steps,
            Some(processes),
        )
        .unwrap();
        Arc::new(server)
    }

    fn ran_in_process(&self, verb: &str) -> usize {
        self.threads.ran[verb].load(Ordering::SeqCst)
    }

    fn ran_in_daemon(&self, verb: &str) -> usize {
        self.in_daemon[verb].load(Ordering::SeqCst)
    }
}

fn handed_off(reply: &Reply) -> Handle {
    assert_eq!(reply.exit, Exit::Running, "{}", reply.text);
    reply.handle.clone().expect("a handle")
}

fn waited(server: &Server, handle: &Handle) -> Reply {
    match server.handles().wait(handle, 20, None) {
        Waited::Done(reply) => reply,
        other => panic!("{handle} did not finish: {other:?}"),
    }
}

/// The acceptance test: a landing and a test run in verb processes survive a restart of the
/// daemon while their commands run. No verb process is started again, neither verb runs again
/// (in its process or in the daemon), no command is started again, and each handle ends with
/// the reply it would have had without the restart.
#[test]
fn a_land_and_a_test_survive_a_restart_without_running_anything_again() {
    let machine = Machine::new("survive", false);
    let first = machine.daemon();
    machine.switch.set(Some(first.clone()));
    let land = handed_off(&first.handle(&request("land", 0)));
    let test = handed_off(&first.handle(&request("test", 0)));
    assert!(
        until(|| machine.commands.begun() == 2),
        "both commands started"
    );
    assert!(until(|| first.steps().by_handle(&land).is_some_and(|a| {
        a.view().step.as_deref() == Some("running cargo land")
    })));

    // The daemon restarts into a new build while both commands run.
    machine.switch.set(None);
    drop(first);
    let second = machine.daemon();
    // It finds both verb processes alive and lists them at once.
    assert!(second.steps().by_handle(&land).is_some());
    assert!(second.steps().by_handle(&test).is_some());
    machine.switch.set(Some(second.clone()));
    machine.commands.finish_all();

    assert_eq!(waited(&second, &land).text, "land: Some(0)");
    assert_eq!(waited(&second, &test).text, "test: Some(0)");
    assert_eq!(machine.threads.started.load(Ordering::SeqCst), 2);
    assert_eq!(
        (
            machine.ran_in_process("land"),
            machine.ran_in_process("test")
        ),
        (1, 1)
    );
    assert_eq!(
        (machine.ran_in_daemon("land"), machine.ran_in_daemon("test")),
        (0, 0)
    );
    assert_eq!(machine.commands.begun(), 2, "no command started again");
    let row = handle_store::get(&lock(&machine.db), land.as_str())
        .unwrap()
        .unwrap();
    assert_eq!(
        (row.state.as_str(), row.executable.as_deref()),
        ("done", Some("87631b5"))
    );
}

/// A client that waits within its own wait gets the verb process's steps as progress lines and
/// the reply itself, as from a verb on a thread of the daemon.
#[test]
fn a_client_hears_a_verb_processs_steps_and_gets_its_reply() {
    let machine = Machine::new("steps", false);
    let server = machine.daemon();
    machine.switch.set(Some(server.clone()));
    let finisher = {
        let commands = machine.commands.clone();
        std::thread::spawn(move || {
            assert!(until(|| commands.begun() == 1));
            commands.finish_all();
        })
    };
    let mut heard = Vec::new();
    let mut progress = |line: &str| {
        heard.push(line.to_owned());
        true
    };
    let reply = server.handle_streaming(&request("test", 20), None, Some(&mut progress));
    finisher.join().unwrap();
    assert_eq!(reply.text, "test: Some(0)");
    assert!(
        heard.iter().any(|line| line == "running cargo test"),
        "{heard:?}"
    );
}

/// A verb process found dead when the daemon starts (its handle `running`, its lock free) has
/// its verb run again under the same handle, in a new verb process, so that the run again
/// outlives the daemon's next restart too.
#[test]
fn a_verb_process_found_dead_at_start_is_run_again_in_a_new_one() {
    let machine = Machine::new("dead-at-start", false);
    let handle = {
        let handles = Handles::open(Arc::new(FakeClock::new(T0)), machine.db.clone()).unwrap();
        handles.create_for("test", None, None, Some(&request("test", 0)))
    };
    let process = handle_store::Process {
        pid: 999_999,
        executable: "87631b5".to_owned(),
        protocol: 1,
    };
    handle_store::set_process(&lock(&machine.db), handle.as_str(), &process).unwrap();

    let server = machine.daemon();
    machine.switch.set(Some(server.clone()));
    assert!(until(|| machine.commands.begun() == 1));
    machine.commands.finish_all();
    assert_eq!(waited(&server, &handle).text, "test: Some(0)");
    assert_eq!(
        (
            machine.ran_in_process("test"),
            machine.ran_in_daemon("test")
        ),
        (1, 0)
    );
    assert_eq!(machine.threads.started.load(Ordering::SeqCst), 1);
    let row = handle_store::get(&lock(&machine.db), handle.as_str())
        .unwrap()
        .unwrap();
    assert_ne!(
        row.pid,
        Some(999_999),
        "the row no longer names the dead process"
    );
}

/// A verb process that dies while the daemon runs is noticed, and its verb is run again in the
/// daemon; a verb that cannot be run again safely ends `incomplete`.
#[test]
fn a_verb_process_that_dies_is_run_again_or_ended_incomplete() {
    let machine = Machine::new("dies", true);
    let server = machine.daemon();
    let reply = server.handle(&request("land", 10));
    assert_eq!(reply.text, "land: Some(0)");
    assert_eq!(machine.ran_in_daemon("land"), 1);

    let handle = handed_off(&server.handle(&request("run", 0)));
    let reply = waited(&server, &handle);
    assert_eq!(reply.exit, Exit::Conflict);
    assert!(
        reply.text.contains("cannot be run again safely"),
        "{}",
        reply.text
    );
    assert_eq!(machine.ran_in_daemon("run"), 0);
}

/// A daemon about to restart answers new requests with the reply that makes clients send them
/// again, still answers the calls of verb processes, and waits for its own verbs.
#[test]
fn a_draining_daemon_refuses_new_requests_and_waits_for_its_own_verbs() {
    let machine = Machine::new("drain", false);
    let server = machine.daemon();
    server.drain(Duration::from_millis(1));
    assert!(is_restarting(&server.handle(&request("land", 0))));
    let hello = encode_request(
        &Poll {
            handle: Handle::new("L-1").unwrap(),
        },
        PathBuf::from("/home"),
    );
    assert!(!is_restarting(&server.handle(&hello)));
    server.undrain();
    machine.switch.set(Some(server.clone()));
    let handle = handed_off(&server.handle(&request("test", 0)));
    // Verb processes are not waited for. A process still starting counts as a running verb, so
    // under load the drain may wait a moment; it must never wait out its whole limit.
    let since = Instant::now();
    server.drain(Duration::from_secs(120));
    assert!(since.elapsed() < Duration::from_secs(60));
    server.undrain();
    // `finish_all` only finishes commands that have begun. Under load the verb process may not
    // have started its command yet, and a command that begins afterwards is never finished.
    assert!(
        until(|| machine.commands.begun() == 1),
        "the command started"
    );
    machine.commands.finish_all();
    assert_eq!(waited(&server, &handle).text, "test: Some(0)");
}

/// `ignite wait` passes on the steps of the work it waits on to a client that asks for them.
#[test]
fn a_wait_that_asks_for_progress_hears_the_steps() {
    let machine = Machine::new("wait", false);
    let server = machine.daemon();
    let handle = server
        .handles()
        .create_for("test", None, None, Some(&request("test", 0)));
    let activity = server
        .steps()
        .begin(Kind::Verb("test".to_owned()), Work::default());
    activity.attach_handle(&handle);
    {
        let (handles, activity, handle) =
            (server.handles().clone(), activity.clone(), handle.clone());
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            activity.set_step("running cargo test");
            std::thread::sleep(Duration::from_millis(300));
            handles.finish(&handle, Reply::text(Exit::Success, "passed"));
            activity.finish();
        });
    }
    let mut heard = Vec::new();
    let mut progress = |line: &str| {
        heard.push(line.to_owned());
        true
    };
    let wait = Request {
        args: vec![handle.to_string()],
        ..request("wait", 10)
    };
    let reply = server.handle_streaming(&wait, None, Some(&mut progress));
    assert_eq!(reply.text, "passed");
    assert_eq!(heard, ["running cargo test"]);
}

/// A landing tree's owner whose verb process is dead still owns it when the daemon runs that
/// verb again (`land`), and no longer does otherwise (`rebuild`).
#[test]
fn a_dead_verb_process_owns_its_landing_tree_only_if_it_is_run_again() {
    let db = ignite_store::local::open_in_memory().unwrap();
    let db = Arc::new(Mutex::new(db));
    let handles = Handles::open(Arc::new(FakeClock::new(T0)), db.clone()).unwrap();
    let land = handles.create_for("land", None, None, Some(&request("land", 0)));
    let rebuild = handles.create_for("rebuild", None, None, Some(&request("rebuild", 0)));
    let in_daemon = handles.create_for("check", None, None, Some(&request("check", 0)));
    for handle in [&land, &rebuild] {
        let process = handle_store::Process {
            pid: 999_999,
            executable: "87631b5".to_owned(),
            protocol: 1,
        };
        handle_store::set_process(&lock(&db), handle.as_str(), &process).unwrap();
    }
    let db = lock(&db);
    assert!(crate::trees::owner_alive(&db, land.as_str()));
    assert!(!crate::trees::owner_alive(&db, rebuild.as_str()));
    assert!(crate::trees::owner_alive(&db, in_daemon.as_str()));
}

#[path = "process_tip_tests.rs"]
mod tip_rounds;

#[path = "outdated_tests.rs"]
mod outdated;
