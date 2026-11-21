//! The host protocol's daemon side against fakes: a starter that writes run directories without
//! supervisors, the fake builds, a fake set of landing trees and an in-memory database. A
//! restart is a second [`Host`] over the same database and runs folder.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::Connection;

use ignite_core::config::{Config, Floor};
use ignite_core::fakes::{FakeBuilds, FakeClock, FakeGit, FakeHub, FakeLauncher};
use ignite_core::ids::{Commit, Handle, MachineId, SessionId, TreeId};
use ignite_core::protocol::Class;
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::protocol::host::{
    AllowStop, Build, BuildReply, BuildWho, Command, Finished, Hello, HostCall, LeaseLanding,
    LeaseLandingReply, PROTOCOL_VERSION, Poll, RefuseStop, ReleaseLanding, RequestId, Run,
    RunReply, Selected, StepUpdate, decode_reply, encode_request,
};
use ignite_core::time::Timestamp;
use ignite_core::traits::{BuildResult, CommandSpec, Result};
use ignite_os::hostcpu::HostCpu;
use ignite_store::local::{handle_stops, handles as handle_store, rows};

use super::{Host, HostParts, LandingTrees, RunStarter, Timing};
use crate::handles::lock;
use crate::sched::{Grant, GrantRequest, Limits, Scheduler};
use crate::server::{Context, Registry, Server};
use crate::steps::Steps;
use crate::supervise::{SpecFile, requests_in, unfinished_of_in};
use crate::trees::TreeLease;

const T0: Timestamp = Timestamp::from_millis(1_800_000_000_000);
const SESSION: &str = "local_a";

fn timing() -> Timing {
    Timing {
        block: Duration::from_millis(100),
        abandon: Duration::from_secs(2),
        watch: Duration::from_millis(20),
        grace: Duration::from_millis(50),
    }
}

/// Starts commands by writing their run directories, without supervisors; waits for a grant
/// first when it has a scheduler. Grants are held until the test releases them.
struct Starter {
    runs: PathBuf,
    scheduler: Option<Arc<Scheduler>>,
    begun: Mutex<Vec<CommandSpec>>,
    grants: Mutex<Vec<Grant>>,
    ended: Mutex<Vec<PathBuf>>,
}

impl Starter {
    fn new(runs: &Path, scheduler: Option<Arc<Scheduler>>) -> Arc<Starter> {
        Arc::new(Starter {
            runs: runs.to_path_buf(),
            scheduler,
            begun: Mutex::new(Vec::new()),
            grants: Mutex::new(Vec::new()),
            ended: Mutex::new(Vec::new()),
        })
    }

    fn begun(&self) -> usize {
        lock(&self.begun).len()
    }

    fn ended(&self) -> Vec<PathBuf> {
        lock(&self.ended).clone()
    }
}

fn grant_request(spec: &CommandSpec) -> GrantRequest {
    GrantRequest {
        class: spec.class,
        cores: spec.cores,
        resources: spec.resources.clone(),
        memory_mb: None,
        shrinks: false,
        grows: false,
        cores_file: None,
        reserve: spec.reserve,
    }
}

impl RunStarter for Starter {
    fn begin(
        &self,
        spec: &CommandSpec,
        since: Timestamp,
        cancelled: &dyn Fn() -> bool,
    ) -> io::Result<Option<PathBuf>> {
        if let Some(scheduler) = &self.scheduler {
            let Some(grant) = scheduler.acquire_since(&grant_request(spec), since, cancelled)
            else {
                return Ok(None);
            };
            lock(&self.grants).push(grant);
        }
        let mut begun = lock(&self.begun);
        let dir = self.runs.join(format!(
            "{}-{}",
            std::process::id(),
            fs::read_dir(&self.runs).map_or(0, Iterator::count)
        ));
        fs::create_dir_all(&dir)?;
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
        begun.push(spec.clone());
        Ok(Some(dir))
    }

    fn find(&self, request: &RequestId) -> Option<PathBuf> {
        let id = request.to_string();
        requests_in(&self.runs)
            .into_iter()
            .find(|(found, _)| *found == id)
            .map(|(_, dir)| dir)
    }

    fn runs_of(&self, handle: &Handle) -> Vec<PathBuf> {
        unfinished_of_in(&self.runs, handle)
            .into_iter()
            .map(|run| run.dir().to_path_buf())
            .collect()
    }

    fn end(&self, run: &Path) {
        lock(&self.ended).push(run.to_path_buf());
        // The supervisor would write the result and the daemon mark the run collected.
        let _ = fs::write(run.join("collected"), "");
    }

    fn kill(&self, run: &Path) {
        self.end(run);
    }

    fn waiting_for(&self, spec: &CommandSpec) -> String {
        format!("{} cores", spec.cores)
    }
}

/// One landing tree, `landing-1`.
#[derive(Default)]
struct Landing {
    held: Mutex<Option<(Handle, Commit)>>,
    released: Mutex<u32>,
}

impl LandingTrees for Landing {
    fn lease(
        &self,
        _holder: &str,
        owner: &Handle,
        at: &Commit,
        _keep_free: usize,
    ) -> Result<Option<TreeLease>> {
        let mut held = lock(&self.held);
        match &*held {
            Some((holder, _)) if holder != owner => return Ok(None),
            _ => *held = Some((owner.clone(), at.clone())),
        }
        Ok(Some(TreeLease {
            tree: TreeId::new("landing-1").unwrap(),
            path: PathBuf::from("/repo/trees/landing-1"),
            branch: "landing-1".to_owned(),
            at: at.clone(),
        }))
    }

    fn release(&self, owner: &Handle, _tree: &TreeId, _built: Option<&Commit>) -> Result<()> {
        let mut held = lock(&self.held);
        if held.as_ref().is_some_and(|(holder, _)| holder == owner) {
            *held = None;
            *lock(&self.released) += 1;
        }
        Ok(())
    }
}

struct Daemon {
    host: Arc<Host>,
    starter: Arc<Starter>,
    builds: Arc<FakeBuilds>,
    steps: Arc<Steps>,
    registry: Registry,
}

/// A daemon's host over `db` and the runs folder `runs`.
fn daemon(db: &Arc<Mutex<Connection>>, runs: &Path, scheduler: Option<Arc<Scheduler>>) -> Daemon {
    let starter = Starter::new(runs, scheduler);
    let builds = Arc::new(FakeBuilds::default());
    let steps = Arc::new(Steps::new(Arc::new(ignite_core::time::SystemClock)));
    let host = Host::with_timing(
        HostParts {
            dispatcher: None,
            starter: starter.clone(),
            builds: builds.clone(),
            landing: Arc::new(Landing::default()),
            db: db.clone(),
            steps: steps.clone(),
            clock: Arc::new(FakeClock::new(T0)),
        },
        timing(),
    );
    let mut registry = Registry::default();
    host.register(&mut registry);
    Daemon {
        host,
        starter,
        builds,
        steps,
        registry,
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

fn database() -> Arc<Mutex<Connection>> {
    Arc::new(Mutex::new(ignite_store::local::open_in_memory().unwrap()))
}

fn runs_folder(name: &str) -> PathBuf {
    let dir = ignite_core::test_files::temp_dir()
        .join(format!("ignite-host-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Records `handle` as a running `land` of [`SESSION`], as the server does before it starts a
/// verb process.
fn running(db: &Arc<Mutex<Connection>>, handle: &str) -> Handle {
    let row = rows::Handle {
        id: handle.to_owned(),
        verb: "land".to_owned(),
        session: Some(SESSION.to_owned()),
        tree: Some("tree-01".to_owned()),
        state: "running".to_owned(),
        exit: None,
        reply: None,
        started_at: T0.to_string(),
        finished_at: None,
        pid: None,
        executable: None,
        protocol: None,
    };
    handle_store::upsert(&lock(db), &row).unwrap();
    Handle::new(handle).unwrap()
}

/// Writes the verb process's reply into its handle's row, as it does before `host-finished`.
fn finish_row(db: &Arc<Mutex<Connection>>, handle: &Handle, state: &str, reply: &Reply) {
    let conn = lock(db);
    let mut row = handle_store::get(&conn, handle.as_str()).unwrap().unwrap();
    row.state = state.to_owned();
    row.exit = Some(i64::from(reply.exit.code()));
    row.reply = Some(serde_json::to_string(reply).unwrap());
    row.finished_at = Some(T0.to_string());
    handle_store::upsert(&conn, &row).unwrap();
}

/// Sends `call` through the daemon's verb for it, as a verb process's connection would.
fn send<C: HostCall>(registry: &Registry, call: &C) -> Result<C::Reply> {
    let request = encode_request(call, PathBuf::from("/repo/trees/tree-01"));
    decode_reply::<C>(registry.handle(&context(), &request))
}

fn run_call(handle: &Handle, counter: u64, asked: Timestamp) -> Run {
    Run {
        handle: handle.clone(),
        request_id: RequestId::new(handle.clone(), counter),
        first_asked_at: asked,
        spec: Command {
            args: vec!["cargo".to_owned(), "nextest".to_owned()],
            cwd: PathBuf::from("/repo/trees/landing-1"),
            env: vec![("CARGO_TERM_COLOR".to_owned(), "never".to_owned())],
            class: Class::Land,
            cores: 2,
            resources: vec!["pages".to_owned()],
            grows: false,
            tree: None,
            ticket: None,
            hang_limit_s: None,
            reserve: None,
        },
    }
}

fn hello(handle: &Handle) -> Hello {
    Hello {
        handle: handle.clone(),
        protocol: PROTOCOL_VERSION,
        executable: "87631b5".to_owned(),
        pid: 4242,
    }
}

fn started(reply: RunReply) -> PathBuf {
    match reply {
        RunReply::Started { run } => run,
        other => panic!("expected a started run, got {other:?}"),
    }
}

/// Repeats a `host-run` call while it answers `waiting`, as a verb process does: on a loaded
/// machine the thread that starts the command may take longer than one call's wait.
fn until_started(mut call: impl FnMut() -> Result<RunReply>) -> PathBuf {
    for _ in 0..600 {
        match call().unwrap() {
            RunReply::Waiting { .. } => {}
            other => return started(other),
        }
    }
    panic!("the command never started");
}

#[test]
fn a_run_repeated_after_a_restart_returns_the_same_run_and_starts_nothing() {
    let db = database();
    let runs = runs_folder("repeat");
    let handle = running(&db, "L-6");
    let first = daemon(&db, &runs, None);
    let call = run_call(&handle, 17, T0);
    let run = until_started(|| send(&first.registry, &call));
    assert_eq!(started(send(&first.registry, &call).unwrap()), run);
    assert_eq!(first.starter.begun(), 1);
    let env = lock(&first.starter.begun)[0].env.clone();
    for variable in ["CARGO_TERM_COLOR", "IGNITE_REQUEST", "IGNITE_HANDLE"] {
        assert!(env.iter().any(|(name, _)| name == variable), "{env:?}");
    }

    // The daemon restarts: the new one finds the run by its request ID in `spec.json`.
    drop(first);
    let second = daemon(&db, &runs, None);
    assert!(send(&second.registry, &hello(&handle)).unwrap().running);
    assert_eq!(started(send(&second.registry, &call).unwrap()), run);
    assert_eq!(second.starter.begun(), 0, "nothing was started again");
    let next = until_started(|| send(&second.registry, &run_call(&handle, 18, T0)));
    assert_ne!(next, run);
    assert_eq!(second.starter.begun(), 1);
    let _ = fs::remove_dir_all(&runs);
}

struct IdleHost;

impl HostCpu for IdleHost {
    fn cores(&self) -> u32 {
        8
    }

    fn busy(&self) -> io::Result<f64> {
        Ok(0.0)
    }

    fn free_memory_mb(&self) -> io::Result<u64> {
        Err(io::Error::other("not read"))
    }
}

/// Eight cores and one `pages` resource, which every command in these tests asks for.
fn scheduler() -> Arc<Scheduler> {
    Arc::new(Scheduler::new(
        Limits {
            cores: 8,
            reserved: 0,
            tip_build: 0,
            floor: Floor::default(),
            resources: BTreeMap::from([("pages".to_owned(), 1)]),
        },
        Arc::new(FakeClock::new(T0.plus_seconds(60.0))),
        Arc::new(IdleHost),
    ))
}

#[test]
fn a_waiting_run_keeps_its_place_in_the_queue_across_a_restart() {
    let db = database();
    let runs = runs_folder("queue");
    let (early, late) = (running(&db, "L-1"), running(&db, "L-2"));
    let pages = GrantRequest {
        resources: vec!["pages".to_owned()],
        ..grant_request(&CommandSpec::from(run_call(&early, 1, T0).spec))
    };
    let first_scheduler = scheduler();
    let holder = first_scheduler.acquire(&pages);
    let first = daemon(&db, &runs, Some(first_scheduler.clone()));
    let early_call = run_call(&early, 1, T0);
    let late_call = run_call(&late, 1, T0.plus_seconds(5.0));
    let waiting = send(&first.registry, &early_call).unwrap();
    assert_eq!(
        waiting,
        RunReply::Waiting {
            what: "2 cores".to_owned()
        }
    );
    assert!(matches!(
        send(&first.registry, &late_call),
        Ok(RunReply::Waiting { .. })
    ));
    assert_eq!(first_scheduler.status().resources[0].waiting, 2);

    // A restarted daemon has a scheduler of its own, with the running command's grant
    // registered again. The later request reaches it first.
    let second_scheduler = scheduler();
    let adopted = second_scheduler.adopt(Class::Land, 2, vec!["pages".to_owned()], None, None);
    let second = daemon(&db, &runs, Some(second_scheduler.clone()));
    assert!(matches!(
        send(&second.registry, &late_call),
        Ok(RunReply::Waiting { .. })
    ));
    assert!(matches!(
        send(&second.registry, &early_call),
        Ok(RunReply::Waiting { .. })
    ));
    second_scheduler.release(&adopted);
    until_started(|| send(&second.registry, &early_call));
    assert!(matches!(
        send(&second.registry, &late_call),
        Ok(RunReply::Waiting { .. })
    ));
    assert_eq!(second.starter.begun(), 1);
    for grant in lock(&second.starter.grants).drain(..) {
        second_scheduler.release(&grant);
    }
    until_started(|| send(&second.registry, &late_call));
    // The first daemon's requests are withdrawn once nobody asks about them; its holder is
    // never released, as the command of a daemon that went away would not be.
    drop(holder);
    let _ = fs::remove_dir_all(&runs);
}

fn stop_request(handle: &Handle) -> Request {
    Request {
        verb: "stop".to_owned(),
        args: vec![handle.to_string()],
        cwd: PathBuf::from("/repo"),
        session: Some(SessionId::new(SESSION).unwrap()),
        wait_s: 5,
        pid: None,
    }
}

/// Sends `call` through the server, as a verb process's connection would.
fn through<C: HostCall>(server: &Server, call: &C) -> Result<C::Reply> {
    decode_reply::<C>(server.handle(&encode_request(call, PathBuf::from("/repo"))))
}

#[test]
fn stopping_a_remote_verb_ends_its_runs_and_is_refused_while_it_refuses() {
    let db = database();
    let runs = runs_folder("stop");
    let Daemon {
        host,
        starter,
        steps,
        registry,
        ..
    } = daemon(&db, &runs, None);
    let server = Server::with_steps(context(), registry, Some(db.clone()), steps.clone()).unwrap();
    let server = Arc::new(server);
    host.set_handles(server.handles().clone());
    let handle = running(&db, "L-6");

    let greeted = through(&server, &hello(&handle)).unwrap();
    assert!(greeted.running && !greeted.stop);
    let run = until_started(|| through(&server, &run_call(&handle, 1, T0)));
    let refuse = RefuseStop {
        handle: handle.clone(),
        reason: "pushing to staging".to_owned(),
    };
    assert!(through(&server, &refuse).unwrap().refused);

    let reply = server.handle(&stop_request(&handle));
    assert_eq!(reply.exit, Exit::Conflict, "{}", reply.text);
    assert!(reply.text.contains("pushing to staging"), "{}", reply.text);
    assert!(starter.ended().is_empty());
    // A restarted daemon refuses as well: the refusal is in the database.
    let kept = handle_stops::get(&lock(&db), "L-6").unwrap().unwrap();
    assert_eq!(kept.refused.as_deref(), Some("pushing to staging"));
    let allow = AllowStop {
        handle: handle.clone(),
    };
    through(&server, &allow).unwrap();

    // The verb process polls until it is asked to stop, waits for its command, which the
    // daemon ends, then writes its reply into the handle's row and says so.
    let process = {
        let (db, handle, server, run) = (db.clone(), handle.clone(), server.clone(), run.clone());
        std::thread::spawn(move || {
            let poll = Poll {
                handle: handle.clone(),
            };
            while !through(&server, &poll).unwrap().stop {
                std::thread::sleep(Duration::from_millis(10));
            }
            while !run.join("collected").exists() {
                std::thread::sleep(Duration::from_millis(10));
            }
            let reply = crate::server::stopped_reply("land", Some(&handle));
            finish_row(&db, &handle, "stopped", &reply);
            through(&server, &Finished { handle }).unwrap();
        })
    };
    let reply = server.handle(&stop_request(&handle));
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(
        reply.text.starts_with("stopped  `ignite land` (L-6)"),
        "{}",
        reply.text
    );
    process.join().unwrap();
    assert_eq!(
        starter.ended(),
        [run],
        "the daemon ended the verb's command"
    );
    assert!(handle_stops::get(&lock(&db), "L-6").unwrap().unwrap().asked);
    assert!(steps.live().is_empty());

    let mut wait = stop_request(&handle);
    wait.verb = "wait".to_owned();
    let waited = server.handle(&wait);
    assert!(
        waited.text.starts_with("stopped  `ignite land`"),
        "{}",
        waited.text
    );
    let later = through(&server, &run_call(&handle, 2, T0));
    assert!(matches!(later, Ok(RunReply::Refused { .. })), "{later:?}");
    let _ = fs::remove_dir_all(&runs);
}

#[test]
fn a_restarted_daemon_keeps_a_refusal_and_a_stop_asked_before_it() {
    let db = database();
    let runs = runs_folder("kept");
    let (pushing, stopping) = (running(&db, "L-1"), running(&db, "L-2"));
    let first = daemon(&db, &runs, None);
    let refuse = RefuseStop {
        handle: pushing.clone(),
        reason: "pushing to staging".to_owned(),
    };
    assert!(send(&first.registry, &refuse).unwrap().refused);
    drop(first);
    // What the first daemon's watcher records once `ignite stop` asks (see the test above).
    let asked = rows::HandleStop {
        handle: "L-2".to_owned(),
        asked: true,
        refused: None,
        at: T0.to_string(),
    };
    handle_stops::set(&lock(&db), &asked).unwrap();

    let second = daemon(&db, &runs, None);
    assert!(send(&second.registry, &hello(&stopping)).unwrap().stop);
    let late_refusal = RefuseStop {
        handle: stopping.clone(),
        reason: "pushing to staging".to_owned(),
    };
    assert!(
        !send(&second.registry, &late_refusal).unwrap().refused,
        "a stop was asked first"
    );
    assert!(!send(&second.registry, &hello(&pushing)).unwrap().stop);
    let activity = second.steps.by_handle(&pushing).unwrap();
    assert_eq!(
        activity.request_stop(),
        Err("pushing to staging".to_owned())
    );
    let _ = fs::remove_dir_all(&runs);
}

#[test]
fn a_verb_process_that_ends_without_saying_so_is_noticed() {
    let db = database();
    let runs = runs_folder("gone");
    let handle = running(&db, "T-3");
    let daemon = daemon(&db, &runs, None);
    send(&daemon.registry, &hello(&handle)).unwrap();
    let activity = daemon.steps.by_handle(&handle).unwrap();
    finish_row(&db, &handle, "done", &Reply::text(Exit::Success, "passed"));
    assert!(activity.wait_finished(Duration::from_secs(10)));
    let reply = send(&daemon.registry, &hello(&handle)).unwrap();
    assert!(!reply.running);
    assert!(send(&daemon.registry, &Poll { handle }).unwrap().stop);
    let _ = fs::remove_dir_all(&runs);
}

#[test]
fn selected_asks_the_build_manager() {
    let db = database();
    let runs = runs_folder("selected");
    let handle = running(&db, "L-6");
    let daemon = daemon(&db, &runs, None);
    let tree = TreeId::new("landing-1").unwrap();
    let ask = |product: &str| Selected {
        handle: handle.clone(),
        tree: tree.clone(),
        product: product.to_owned(),
        packages: vec!["engine".to_owned()],
    };
    daemon.builds.script_selected(&tree, "rust-tests", &[]);
    assert!(
        send(&daemon.registry, &ask("rust-tests"))
            .unwrap()
            .packages
            .is_empty()
    );
    assert_eq!(
        send(&daemon.registry, &ask("other")).unwrap().packages,
        ["engine"]
    );
    let _ = fs::remove_dir_all(&runs);
}

#[test]
fn a_repeated_build_answers_once_and_leases_are_kept_by_their_owner() {
    let db = database();
    let runs = runs_folder("build");
    let handle = running(&db, "L-6");
    let daemon = daemon(&db, &runs, None);
    let tree = TreeId::new("landing-1").unwrap();
    daemon
        .builds
        .script(&tree, "rust-tests", BuildResult::Current);
    let build = Build {
        handle: handle.clone(),
        request_id: RequestId::new(handle.clone(), 3),
        first_asked_at: T0,
        tree: tree.clone(),
        product: "rust-tests".to_owned(),
        packages: vec!["engine".to_owned()],
        who: BuildWho {
            class: Class::Land,
            ticket: None,
            session: None,
        },
    };
    assert_eq!(send(&daemon.registry, &build).unwrap(), BuildReply::Current);
    assert_eq!(send(&daemon.registry, &build).unwrap(), BuildReply::Current);
    assert_eq!(daemon.builds.requests().len(), 1);

    let commit = Commit::new("87631b5").unwrap();
    let lease = LeaseLanding {
        handle: handle.clone(),
        holder: "m-1:T1".to_owned(),
        commit: commit.clone(),
        keep_free: 0,
    };
    let leased = send(&daemon.registry, &lease).unwrap();
    assert!(matches!(leased, LeaseLandingReply::Leased { .. }));
    assert_eq!(send(&daemon.registry, &lease).unwrap(), leased);
    let other = running(&db, "L-7");
    let busy = LeaseLanding {
        handle: other,
        ..lease
    };
    assert_eq!(
        send(&daemon.registry, &busy).unwrap(),
        LeaseLandingReply::Busy
    );
    let release = ReleaseLanding {
        handle: handle.clone(),
        tree,
        built: Some(commit),
    };
    send(&daemon.registry, &release).unwrap();
    send(&daemon.registry, &release).unwrap();

    // A handle that is not running leases nothing and starts nothing.
    let gone = Handle::new("L-9").unwrap();
    assert!(
        send(
            &daemon.registry,
            &Build {
                handle: gone.clone(),
                request_id: RequestId::new(gone.clone(), 1),
                ..build
            }
        )
        .is_err()
    );
    assert!(matches!(
        send(&daemon.registry, &run_call(&gone, 1, T0)),
        Ok(RunReply::Refused { .. })
    ));
    let _ = fs::remove_dir_all(&runs);
}

#[test]
fn a_verb_adopted_after_a_restart_ages_from_its_handle_and_says_its_step_is_not_reported() {
    use ignite_core::traits::Clock as _;
    let db = database();
    let runs = runs_folder("adopted");
    let handle = running(&db, "L-9");
    let half_hour_ago =
        Timestamp::from_millis(ignite_core::time::SystemClock.now().millis() - 30 * 60 * 1000);
    {
        let conn = lock(&db);
        let mut row = handle_store::get(&conn, handle.as_str()).unwrap().unwrap();
        row.started_at = half_hour_ago.to_string();
        handle_store::upsert(&conn, &row).unwrap();
    }
    let daemon = daemon(&db, &runs, None);
    send(&daemon.registry, &hello(&handle)).unwrap();
    let view = daemon.steps.by_handle(&handle).unwrap().view();
    assert!(view.age_s >= 30.0 * 60.0, "age {}", view.age_s);
    assert_eq!(view.doing(), "step not reported since the daemon restarted");

    let update = StepUpdate {
        handle: handle.clone(),
        step: Some("running the gate".to_owned()),
        waiting: Vec::new(),
        class: None,
        ticket: None,
    };
    let state = send(&daemon.registry, &update).unwrap();
    assert!(state.daemon_started.is_some());
    assert!(
        daemon
            .steps
            .by_handle(&handle)
            .unwrap()
            .view()
            .doing()
            .starts_with("running the gate")
    );
    let _ = fs::remove_dir_all(&runs);
}
