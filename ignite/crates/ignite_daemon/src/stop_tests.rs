use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use ignite_core::config::{Config, Floor};
use ignite_core::fakes::{FakeBuilds, FakeClock, FakeGit, FakeHub, FakeLauncher};
use ignite_core::ids::{MachineId, SessionId};
use ignite_core::protocol::Class;
use ignite_core::time::Timestamp;
use ignite_os::hostcpu::HostCpu;

use super::*;
use crate::sched::{GrantRequest, Limits, Scheduler};
use crate::server::{Registry, Server};
use crate::steps;

struct IdleHost;

impl HostCpu for IdleHost {
    fn cores(&self) -> u32 {
        6
    }

    fn busy(&self) -> io::Result<f64> {
        Ok(0.0)
    }

    fn free_memory_mb(&self) -> io::Result<u64> {
        Err(io::Error::other("not read"))
    }
}

/// Six cores and one `pages` resource.
fn scheduler() -> Arc<Scheduler> {
    Arc::new(Scheduler::new(
        Limits {
            cores: 6,
            floor: Floor::default(),
            resources: BTreeMap::from([("pages".to_owned(), 1)]),
        },
        Arc::new(FakeClock::new(Timestamp::EPOCH)),
        Arc::new(IdleHost),
    ))
}

fn pages(class: Class) -> GrantRequest {
    GrantRequest {
        class,
        cores: 2,
        resources: vec!["pages".to_owned()],
        memory_mb: None,
        shrinks: false,
        grows: false,
        cores_file: None,
    }
}

/// A `test` that waits for the `pages` resource, which the test holds, the way a supervised
/// command waits for its grant.
struct WaitsForPages(Arc<Scheduler>);

impl Verb for WaitsForPages {
    fn name(&self) -> &'static str {
        "test"
    }

    fn run(&self, _ctx: &Context, _req: &Request) -> Reply {
        steps::step("running 3 checks of ui-test");
        match self
            .0
            .acquire_until(&pages(Class::Agent), &steps::stop_requested)
        {
            Some(grant) => {
                self.0.release(&grant);
                Reply::text(Exit::Success, "passed   3 checks")
            }
            None => Reply::text(Exit::CheckFailed, "incomplete 3 checks"),
        }
    }
}

/// A `land` whose push has begun: it refuses to stop, then waits until the test lets it end.
struct Pushing(Arc<(Mutex<bool>, Condvar)>);

impl Verb for Pushing {
    fn name(&self) -> &'static str {
        "land"
    }

    fn run(&self, _ctx: &Context, _req: &Request) -> Reply {
        assert!(steps::refuse_stop("its push to staging has begun"));
        steps::step("pushing to staging");
        let (done, changed) = &*self.0;
        let mut done = done.lock().unwrap();
        while !*done {
            done = changed.wait(done).unwrap();
        }
        Reply::text(Exit::Success, "landed")
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
        clock: Arc::new(FakeClock::new(Timestamp::EPOCH)),
        hub: Arc::new(FakeHub::default()),
        git: Arc::new(FakeGit::default()),
        launcher: Arc::new(FakeLauncher::default()),
        builds: Arc::new(FakeBuilds::default()),
        session: None,
        tree: None,
    }
}

fn request(verb: &str, args: &[&str], session: &str, wait_s: u32) -> Request {
    Request {
        verb: verb.to_owned(),
        args: args.iter().map(|s| (*s).to_owned()).collect(),
        cwd: PathBuf::from("/repo"),
        session: Some(SessionId::new(session).unwrap()),
        wait_s,
        pid: None,
    }
}

#[test]
fn stopping_a_test_that_waits_for_cores_withdraws_its_request_and_ends_it_stopped() {
    let scheduler = scheduler();
    let held = scheduler.acquire(&pages(Class::Agent));
    let mut registry = Registry::default();
    registry.register(Box::new(WaitsForPages(scheduler.clone())));
    let steps = Arc::new(steps::Steps::new(Arc::new(ignite_core::time::SystemClock)));
    let server = Server::with_steps(context(), registry, None, steps.clone()).unwrap();

    let started = server.handle(&request("test", &[], "local_a", 0));
    assert_eq!(started.exit, Exit::Running);
    let handle = started.handle.unwrap();

    let other = server.handle(&request("stop", &[handle.as_str()], "local_b", 1));
    assert_eq!(other.exit, Exit::Conflict, "{}", other.text);
    assert!(
        other.text.contains("belongs to session local_a"),
        "{}",
        other.text
    );

    let stopped = server.handle(&request("stop", &[handle.as_str()], "local_a", 1));
    assert_eq!(stopped.exit, Exit::Success, "{}", stopped.text);
    assert!(
        stopped.text.starts_with("stopped  `ignite test`"),
        "{}",
        stopped.text
    );
    let pages = scheduler.status().resources;
    assert_eq!(pages[0].waiting, 0, "the waiting request was withdrawn");
    assert!(steps.live().is_empty());

    let waited = server.handle(&request("wait", &[handle.as_str()], "local_a", 1));
    assert_eq!(waited.exit, Exit::Conflict);
    assert!(
        waited.text.starts_with("stopped  `ignite test`"),
        "{}",
        waited.text
    );
    let again = server.handle(&request("stop", &[handle.as_str()], "local_a", 1));
    assert!(again.text.contains("nothing to stop"), "{}", again.text);
    scheduler.release(&held);
}

#[test]
fn a_landing_whose_push_began_refuses_to_stop_and_says_why() {
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let mut registry = Registry::default();
    registry.register(Box::new(Pushing(gate.clone())));
    let steps = Arc::new(steps::Steps::new(Arc::new(ignite_core::time::SystemClock)));
    let server = Server::with_steps(context(), registry, None, steps.clone()).unwrap();

    let started = server.handle(&request("land", &[], "local_a", 0));
    let handle = started.handle.unwrap();
    // The refusal is recorded before the step, so once the step shows the refusal holds.
    let activity = steps.by_handle(&handle).unwrap();
    while activity.view().step.is_none() {
        std::thread::yield_now();
    }
    let refused = server.handle(&request("stop", &[handle.as_str()], "local_a", 1));
    assert_eq!(refused.exit, Exit::Conflict);
    assert!(
        refused
            .text
            .contains("cannot be stopped: its push to staging has begun"),
        "{}",
        refused.text
    );
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    assert!(activity.wait_finished(Duration::from_secs(60)));
    let waited = server.handle(&request("wait", &[handle.as_str()], "local_a", 1));
    assert_eq!(waited.text, "landed");
}

#[test]
fn a_wait_that_ends_first_names_the_step_and_how_long_it_has_lasted() {
    let scheduler = scheduler();
    let held = scheduler.acquire(&pages(Class::Agent));
    let mut registry = Registry::default();
    registry.register(Box::new(WaitsForPages(scheduler.clone())));
    let clock = Arc::new(FakeClock::new(Timestamp::EPOCH));
    let steps = Arc::new(steps::Steps::new(clock.clone()));
    let server = Server::with_steps(context(), registry, None, steps.clone()).unwrap();

    let handle = server
        .handle(&request("test", &[], "local_a", 0))
        .handle
        .unwrap();
    let activity = steps.by_handle(&handle).unwrap();
    while activity.view().waiting_for.is_none() {
        std::thread::yield_now();
    }
    clock.advance(240.0);
    let waited = server.handle(&request("wait", &[handle.as_str()], "local_a", 0));
    assert_eq!(waited.exit, Exit::Running);
    assert!(
        waited.text.contains(
            "step     running 3 checks of ui-test (4 min), waiting for 2 cores for agent work"
        ),
        "{}",
        waited.text
    );
    scheduler.release(&held);
    assert!(activity.wait_finished(Duration::from_secs(60)));
}
