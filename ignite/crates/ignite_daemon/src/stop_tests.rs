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
            reserved: 0,
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

/// A verb that never looks at its stop, as a hung verb process does, until the test's fake
/// ender ends it.
struct Hangs(Arc<(Mutex<bool>, Condvar)>);

impl Verb for Hangs {
    fn name(&self) -> &'static str {
        "tip-round"
    }

    fn run(&self, _ctx: &Context, _req: &Request) -> Reply {
        steps::step("running checks past the tip budget");
        let (done, changed) = &*self.0;
        let mut done = done.lock().unwrap();
        while !*done {
            done = changed.wait(done).unwrap();
        }
        Reply::text(Exit::Success, "tip verified")
    }
}

/// Ends a [`Hangs`] as ending its verb process would, and counts the calls.
struct FakeEnder(Arc<(Mutex<bool>, Condvar)>, Mutex<u32>);

impl EndVerbProcess for FakeEnder {
    fn end(&self, _handle: &Handle) -> Result<Option<String>, String> {
        *self.1.lock().unwrap() += 1;
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_all();
        Ok(Some(
            "its verb process 7 and its group (1 processes), with `SIGTERM`".to_owned(),
        ))
    }
}

/// An `in_progress` ticket labelled `justiciar` that `holder` holds.
fn justiciar_ticket(holder: &str) -> ignite_core::protocol::tickets::TicketDetail {
    serde_json::from_value(serde_json::json!({
        "id": "T.1", "parent": null, "next_child": 1, "title": "Master is stuck",
        "body": "", "type": "task", "status": "in_progress", "priority": 0, "assignee": holder,
        "labels": ["justiciar"], "defer_until": null, "close_reason": null,
        "created_at": "2027-01-01T00:00:00.000Z", "created_by": "test",
        "updated_at": "2027-01-01T00:00:00.000Z", "closed_at": null, "version": 1,
        "session": holder, "edges": [], "children": [], "ready": false, "blocked_by": [],
    }))
    .unwrap()
}

fn as_session(session: Option<&str>) -> Context {
    let mut ctx = context();
    ctx.session = session.map(|s| SessionId::new(s).unwrap());
    ctx
}

/// `--force` stops a verb that belongs to no session, as a daemon loop's tip round does, for
/// a session holding a justiciar ticket or for the operator, ending its verb process when it
/// does not stop when asked; any other session is refused.
#[test]
fn a_justiciar_or_the_operator_stops_a_hung_internal_verb_by_force() {
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let mut registry = Registry::default();
    registry.register(Box::new(Hangs(gate.clone())));
    let steps = Arc::new(steps::Steps::new(Arc::new(ignite_core::time::SystemClock)));
    let server = Server::with_steps(context(), registry, None, steps.clone()).unwrap();
    let mut started = request("tip-round", &[], "local_a", 0);
    started.session = None;
    let handle = server.handle(&started).handle.unwrap();
    let activity = steps.by_handle(&handle).unwrap();
    while activity.view().step.is_none() {
        std::thread::yield_now();
    }
    let ender = Arc::new(FakeEnder(gate.clone(), Mutex::new(0)));
    let stop = StopVerb::new(server.handles().clone(), steps.clone())
        .with_wait(Duration::from_millis(20))
        .ending_processes_with(ender.clone());
    let force = request("stop", &[handle.as_str(), "--force"], "local_w", 0);

    let plain = stop.run(
        &as_session(Some("local_j")),
        &request("stop", &[handle.as_str()], "local_j", 0),
    );
    assert_eq!(plain.exit, Exit::Conflict, "{}", plain.text);
    assert!(
        plain.text.contains("belongs to no session"),
        "{}",
        plain.text
    );
    assert!(plain.text.contains("--force"), "{}", plain.text);

    let worker = stop.run(&as_session(Some("local_w")), &force);
    assert_eq!(worker.exit, Exit::Conflict, "{}", worker.text);
    assert!(
        worker.text.contains("holds no in_progress ticket"),
        "{}",
        worker.text
    );
    assert_eq!(*ender.1.lock().unwrap(), 0);

    let justiciar = as_session(Some("local_j"));
    let hub = FakeHub::default();
    hub.insert_ticket(justiciar_ticket("local_j"));
    let justiciar = Context {
        hub: Arc::new(hub),
        ..justiciar
    };
    let stopped = stop.run(&justiciar, &force);
    assert_eq!(stopped.exit, Exit::Success, "{}", stopped.text);
    assert!(
        stopped.text.starts_with("stopped  `ignite tip-round`")
            && stopped.text.contains("by force"),
        "{}",
        stopped.text
    );
    assert!(
        stopped.text.contains("ended    its verb process 7"),
        "{}",
        stopped.text
    );
    assert_eq!(*ender.1.lock().unwrap(), 1);

    // The operator names no session and needs no ticket.
    assert!(StopVerb::may_force(&as_session(None)).is_ok());
}
