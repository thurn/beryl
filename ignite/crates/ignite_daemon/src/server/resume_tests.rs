//! A daemon restarted while verbs ran: against the fakes, the verb is run again under its
//! handle, its finished commands come from the cache, its running ones are taken over, and a
//! client that waits on the handle gets the reply the first daemon would have given.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use ignite_core::config::Config;
use ignite_core::fakes::{FakeBuilds, FakeClock, FakeGit, FakeHub, FakeLauncher};
use ignite_core::ids::{Handle, MachineId, SessionId};
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::protocol::{Class, Usage};
use ignite_core::time::Timestamp;
use ignite_core::traits::{CommandSpec, Launcher, RunResult};

use crate::adopt::{AdoptingLauncher, HANDLE_VARIABLE, Previous};
use crate::server::{Context, Registry, Server, Verb};

/// The result cache the fake verbs share: each command's exit, by command, and how many times
/// each was stored.
#[derive(Default)]
struct Cache {
    results: BTreeMap<String, i32>,
    stores: BTreeMap<String, u32>,
}

/// A verb like `test` or a landing's gate: runs `commands` in order, each from the cache if it
/// is there, and stores each result it runs.
struct Checks {
    name: &'static str,
    commands: Vec<&'static str>,
    cache: Arc<Mutex<Cache>>,
}

impl Verb for Checks {
    fn name(&self) -> &str {
        self.name
    }

    fn run(&self, ctx: &Context, _req: &Request) -> Reply {
        let mut lines = Vec::new();
        for command in &self.commands {
            let cached = self.cache.lock().unwrap().results.get(*command).copied();
            let exit = if let Some(exit) = cached {
                exit
            } else {
                let exit = ctx.launcher.run(&spec(command)).exit.unwrap_or(-1);
                let mut cache = self.cache.lock().unwrap();
                cache.results.insert((*command).to_owned(), exit);
                *cache.stores.entry((*command).to_owned()).or_default() += 1;
                exit
            };
            lines.push(format!("{command}: {exit}"));
        }
        Reply::text(Exit::Success, lines.join("\n"))
    }
}

fn spec(command: &str) -> CommandSpec {
    CommandSpec {
        args: command.split(' ').map(str::to_owned).collect(),
        cwd: PathBuf::from("/repo/trees/tree-01"),
        env: Vec::new(),
        class: Class::Agent,
        cores: 1,
        resources: Vec::new(),
        grows: false,
        tree: None,
        ticket: None,
        hang_limit_s: None,
    }
}

fn passed(exit: i32) -> RunResult {
    RunResult {
        exit: Some(exit),
        output: PathBuf::from("/runs/output.log"),
        usage: Usage::default(),
        hung: false,
    }
}

/// The first daemon's launcher: each command finishes at once with exit 0, except
/// `blocking`, which never finishes in this daemon, as when the daemon restarts while it runs.
/// Every command it starts is passed to `started`.
struct Dying {
    blocking: &'static str,
    started: Mutex<mpsc::Sender<CommandSpec>>,
}

impl Launcher for Dying {
    fn run(&self, spec: &CommandSpec) -> RunResult {
        self.started.lock().unwrap().send(spec.clone()).unwrap();
        if spec.args.join(" ") == self.blocking {
            loop {
                std::thread::park();
            }
        }
        passed(0)
    }
}

fn context(launcher: Arc<dyn Launcher>) -> Context {
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
        clock: Arc::new(FakeClock::new(Timestamp::EPOCH)),
        hub: Arc::new(FakeHub::default()),
        git: Arc::new(FakeGit::default()),
        launcher,
        builds: Arc::new(FakeBuilds::default()),
        session: None,
        tree: None,
    }
}

fn request(verb: &str, args: &[&str], wait_s: u32) -> Request {
    Request {
        verb: verb.to_owned(),
        args: args.iter().map(|s| (*s).to_owned()).collect(),
        cwd: PathBuf::from("/repo/trees/tree-01"),
        session: Some(SessionId::new("local_a").unwrap()),
        wait_s,
        pid: None,
    }
}

fn temp_db(name: &str) -> PathBuf {
    let dir = ignite_core::test_files::temp_dir().join(format!("ignite-resume-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("daemon.db")
}

fn db(path: &std::path::Path) -> Arc<Mutex<rusqlite::Connection>> {
    Arc::new(Mutex::new(ignite_store::local::open(path).unwrap()))
}

fn registry(verb: Checks) -> Registry {
    let mut registry = Registry::default();
    registry.register_resumable(Box::new(verb));
    registry
}

/// Runs `verb` named `name` in a first daemon until its command `blocking` has started, then
/// restarts: a second daemon on the same database runs it again, with the commands the first
/// one started available to take over. Returns the handle the client was given, what a wait on
/// it answers, the commands the second daemon started itself, and the cache.
fn restart_during(
    name: &'static str,
    commands: &[&'static str],
    blocking: &'static str,
) -> (Handle, Reply, Vec<CommandSpec>, Arc<Mutex<Cache>>) {
    let path = temp_db(name);
    let cache = Arc::new(Mutex::new(Cache::default()));
    let checks = || Checks {
        name,
        commands: commands.to_vec(),
        cache: cache.clone(),
    };

    let (started, heard) = mpsc::channel();
    let dying = Arc::new(Dying {
        blocking,
        started: Mutex::new(started),
    });
    let first_launcher = Arc::new(AdoptingLauncher::new(dying, Vec::new()));
    let first = Arc::new(
        Server::new(context(first_launcher), registry(checks()), Some(db(&path))).unwrap(),
    );
    let (named, handle) = mpsc::channel();
    {
        let first = first.clone();
        std::thread::spawn(move || {
            let mut tell = |handle: &Handle| named.send(handle.clone()).unwrap();
            first.handle_answering(&request(name, &[], 0), None, None, Some(&mut tell))
        });
    }
    // The client hears the handle before the verb starts.
    let handle: Handle = handle.recv().unwrap();
    // The first daemon "dies" once the blocking command has started: its verb thread never
    // finishes, and its commands go on under their supervisors.
    let mut running = Vec::new();
    loop {
        let spec: CommandSpec = heard.recv().unwrap();
        let last = spec.args.join(" ") == blocking;
        running.push(spec);
        if last {
            break;
        }
    }
    let previous = running
        .iter()
        .filter(|spec| spec.args.join(" ") == blocking)
        .map(|spec| {
            let tagged = spec
                .env
                .iter()
                .find(|(name, _)| name == HANDLE_VARIABLE)
                .map(|(_, value)| Handle::new(value.as_str()).unwrap())
                .expect("every command of a handled verb carries its handle");
            Previous {
                handle: tagged,
                args: spec.args.clone(),
                cwd: spec.cwd.clone(),
                env: spec.env.clone(),
                // The supervisor outlived the daemon and its command finished with exit 2.
                wait: Box::new(|| passed(2)),
                end: Box::new(|| {}),
            }
        })
        .collect();

    let fresh = Arc::new(FakeLauncher::default());
    fresh.script(&[""], passed(0));
    let second_launcher = Arc::new(AdoptingLauncher::new(fresh.clone(), previous));
    let second = Server::new(
        context(second_launcher),
        registry(checks()),
        Some(db(&path)),
    )
    .unwrap();
    let waited = second.handle(&request("wait", &[handle.as_str()], 60));
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
    (handle, waited, fresh.runs(), cache)
}

#[test]
fn a_restart_during_a_test_reattaches_to_its_running_command_and_stores_its_result_once() {
    let (handle, waited, started_again, cache) =
        restart_during("test", &["cargo nextest run"], "cargo nextest run");
    assert_eq!(handle.as_str(), "T-1");
    // The reply the first daemon would have given: the command's own exit.
    assert_eq!(waited, Reply::text(Exit::Success, "cargo nextest run: 2"));
    assert!(started_again.is_empty(), "{started_again:?}");
    assert_eq!(cache.lock().unwrap().stores["cargo nextest run"], 1);
}

#[test]
fn a_restart_during_a_gate_takes_finished_checks_from_the_cache_and_adopts_the_running_one() {
    let (_, waited, started_again, cache) = restart_during(
        "land",
        &["cargo clippy", "cargo nextest run", "npm test"],
        "cargo nextest run",
    );
    assert_eq!(
        waited.text,
        "cargo clippy: 0\ncargo nextest run: 2\nnpm test: 0"
    );
    // Only the check that had not started yet runs in the second daemon.
    let again: Vec<String> = started_again.iter().map(|s| s.args.join(" ")).collect();
    assert_eq!(again, ["npm test"]);
    let cache = cache.lock().unwrap();
    assert!(
        cache.stores.values().all(|stores| *stores == 1),
        "{:?}",
        cache.stores
    );
    assert_eq!(cache.stores.len(), 3);
}

#[test]
fn a_verb_that_cannot_be_run_again_ends_incomplete_and_says_why() {
    let path = temp_db("incomplete");
    let handle = {
        let handles =
            crate::handles::Handles::open(Arc::new(FakeClock::new(Timestamp::EPOCH)), db(&path))
                .unwrap();
        handles.create_for("release", None, None, Some(&request("release", &[], 540)))
    };
    let launcher = Arc::new(FakeLauncher::default());
    let server = Server::new(context(launcher), Registry::default(), Some(db(&path))).unwrap();
    let waited = server.handle(&request("wait", &[handle.as_str()], 0));
    assert_eq!(waited.exit, Exit::Conflict);
    assert!(
        waited.text.starts_with(
            "incomplete the daemon restarted while `ignite release` (R-1) ran, and this daemon \
             has no such verb"
        ),
        "{}",
        waited.text
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
