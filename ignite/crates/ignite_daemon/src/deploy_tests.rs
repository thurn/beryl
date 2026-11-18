use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use ignite_core::config::Config;
use ignite_core::fakes::{FakeBuilds, FakeClock, FakeGit, FakeHub, FakeLauncher};
use ignite_core::ids::MachineId;
use ignite_core::protocol::client::Request;
use ignite_core::protocol::fleet::MachineVersion;
use ignite_core::traits::Error;

use super::*;
use crate::handles::{Handles, WaitVerb};
use crate::server::{Context, Verb};

/// Commits in the order they were made: a later one contains every earlier one.
const HISTORY: [&str; 3] = ["aaaaaaa", "bbbbbbb", "ccccccc"];

fn commit(hex: &str) -> Commit {
    Commit::new(hex).unwrap()
}

fn start() -> Timestamp {
    Timestamp::from_millis(1_800_000_000_000)
}

/// A daemon's self-update, as the tests set it.
struct FakeState {
    local: Mutex<LocalRelease>,
    /// What a check of the release branch answers.
    check: Mutex<Result<(Commit, Checked)>>,
    rejected: Mutex<Option<(Commit, String)>>,
    checks_asked: AtomicUsize,
}

impl FakeState {
    fn new(own: &str, tip: &str) -> FakeState {
        FakeState {
            local: Mutex::new(LocalRelease {
                own: Some(commit(own)),
                branch: "release".to_owned(),
                next_check: Some(start().plus_seconds(240.0)),
                work: None,
            }),
            check: Mutex::new(Ok((commit(tip), Checked::Build))),
            rejected: Mutex::new(None),
            checks_asked: AtomicUsize::new(0),
        }
    }

    fn edit(&self, change: impl FnOnce(&mut LocalRelease)) {
        change(&mut self.local.lock().unwrap());
    }
}

impl DeployState for FakeState {
    fn local(&self) -> LocalRelease {
        self.local.lock().unwrap().clone()
    }

    fn check(&self) -> Result<(Commit, Checked)> {
        self.check.lock().unwrap().clone()
    }

    fn check_soon(&self) {
        self.checks_asked.fetch_add(1, Ordering::SeqCst);
    }

    fn reached(&self, version: &Commit, target: &Commit) -> bool {
        let at = |c: &Commit| HISTORY.iter().position(|h| *h == c.as_str());
        matches!((at(version), at(target)), (Some(v), Some(t)) if v >= t)
    }

    fn rejection(&self, commit: &Commit) -> Option<String> {
        self.rejected
            .lock()
            .unwrap()
            .as_ref()
            .filter(|(bad, _)| bad == commit)
            .map(|(_, reason)| reason.clone())
    }
}

fn machine(name: &str, version: &str, seen_s_ago: f64) -> MachineVersion {
    MachineVersion {
        machine: MachineId::new(format!("m-{name}")).unwrap(),
        name: name.to_owned(),
        version: version.to_owned(),
        last_seen: start().plus_seconds(-seen_s_ago),
    }
}

fn fleet(hub: &str, machines: Vec<MachineVersion>) -> FleetVersions {
    FleetVersions {
        hub_version: hub.to_owned(),
        machines,
    }
}

struct World {
    state: Arc<FakeState>,
    hub: Arc<FakeHub>,
    clock: Arc<FakeClock>,
}

impl World {
    fn new(own: &str, tip: &str) -> World {
        let hub = Arc::new(FakeHub::default());
        hub.set_fleet_versions(fleet(own, vec![machine("jarvis", own, 5.0)]));
        World {
            state: Arc::new(FakeState::new(own, tip)),
            hub,
            clock: Arc::new(FakeClock::new(start())),
        }
    }

    /// A waiter whose pause moves the fake clock and then runs `step` with the number of the
    /// pause, starting at 1.
    fn waiter(&self, step: impl Fn(usize) + Send + Sync + 'static) -> DeployWaiter {
        let clock = self.clock.clone();
        let pauses = AtomicUsize::new(0);
        DeployWaiter {
            state: self.state.clone(),
            hub: self.hub.clone(),
            clock: self.clock.clone(),
            pause: Arc::new(move |seconds| {
                clock.advance(seconds);
                step(pauses.fetch_add(1, Ordering::SeqCst) + 1);
            }),
        }
    }

    fn section(&self) -> Vec<String> {
        ReleaseSection {
            state: self.state.clone(),
            hub: self.hub.clone(),
            clock: self.clock.clone(),
        }
        .lines()
    }
}

#[test]
fn a_deploy_handle_names_its_commit() {
    let target = commit("bbbbbbb");
    assert_eq!(handle(&target), "deploy-bbbbbbb");
    assert_eq!(parse_handle("deploy-bbbbbbb"), Some(target));
    assert_eq!(parse_handle("L-5521"), None);
    assert_eq!(parse_handle("deploy-not-hex"), None);
}

#[test]
fn a_landing_names_the_deploy_wait_only_when_it_moved_the_release() {
    let landed = commit("bbbbbbb");
    let (next, deploy) = landed_next(Some("release"), Some("release moved to bbbbbbb"), &landed);
    assert_eq!(deploy.as_deref(), Some("deploy-bbbbbbb"));
    assert!(next.starts_with("ignite wait deploy-bbbbbbb until every machine runs"));
    let quiet = ("nothing; wait for your next ticket".to_owned(), None);
    assert_eq!(landed_next(Some("release"), None, &landed), quiet);
    let refused = Some("release not moved: rel checks did not pass");
    assert_eq!(landed_next(Some("release"), refused, &landed), quiet);
    assert_eq!(
        landed_next(None, Some("release moved to bbbbbbb"), &landed),
        quiet
    );
}

/// After a landing moved the release, `ignite status` said nothing about it; now it says the
/// release is pending and when the daemon checks for it, then that it runs.
#[test]
fn status_says_the_release_is_pending_then_deployed() {
    let world = World::new("aaaaaaa", "bbbbbbb");
    assert_eq!(
        world.section(),
        [
            "release at bbbbbbb",
            "this daemon runs aaaaaaa; pending: it checks release next in 4 min",
            "the hub runs aaaaaaa, behind the release",
            "jarvis aaaaaaa, behind the release",
        ]
    );

    world
        .state
        .edit(|local| local.work = Some(Work::Building(commit("bbbbbbb"))));
    assert_eq!(
        world.section()[1],
        "this daemon runs aaaaaaa; building bbbbbbb"
    );
    world
        .state
        .edit(|local| local.work = Some(Work::SelfTesting(commit("bbbbbbb"))));
    assert_eq!(
        world.section()[1],
        "this daemon runs aaaaaaa; self-testing bbbbbbb"
    );
    world
        .state
        .edit(|local| local.work = Some(Work::Restarting(commit("bbbbbbb"))));
    assert_eq!(
        world.section()[1],
        "this daemon runs aaaaaaa; restarting into bbbbbbb at its next heartbeat"
    );

    world.state.edit(|local| {
        local.own = Some(commit("bbbbbbb"));
        local.work = None;
    });
    world
        .hub
        .set_fleet_versions(fleet("bbbbbbb", vec![machine("jarvis", "bbbbbbb", 5.0)]));
    assert_eq!(
        world.section(),
        [
            "release at bbbbbbb",
            "this daemon runs bbbbbbb, the release",
            "the hub runs bbbbbbb, the release",
            "jarvis bbbbbbb, the release",
        ]
    );
}

#[test]
fn status_gives_the_reason_of_a_rollback_and_says_why_a_daemon_stays() {
    let world = World::new("aaaaaaa", "bbbbbbb");
    *world.state.rejected.lock().unwrap() = Some((
        commit("bbbbbbb"),
        "bbbbbbb started 2 times within 300 s".to_owned(),
    ));
    assert_eq!(
        world.section()[1],
        "this daemon runs aaaaaaa; rolled back bbbbbbb: bbbbbbb started 2 times within 300 s"
    );

    let world = World::new("aaaaaaa", "bbbbbbb");
    *world.state.check.lock().unwrap() = Ok((commit("bbbbbbb"), Checked::Unchanged));
    assert_eq!(
        world.section()[1],
        "this daemon runs aaaaaaa; bbbbbbb changed nothing under Ignite's source, so it stays"
    );

    world.hub.set_unreachable(true);
    assert_eq!(
        world.section()[2],
        "the hub cannot be reached: unreachable: fake hub is down"
    );
    *world.state.check.lock().unwrap() = Err(Error::Unreachable("offline".to_owned()));
    assert_eq!(
        world.section(),
        [
            "release cannot be checked: unreachable: offline",
            "this daemon runs aaaaaaa"
        ]
    );
}

/// The wait asks the daemon to check the branch now, follows the build and ends once this
/// daemon, the hub and every machine run the release.
#[test]
fn a_wait_ends_once_every_machine_runs_the_release() {
    let world = World::new("aaaaaaa", "bbbbbbb");
    let (state, hub) = (world.state.clone(), world.hub.clone());
    let waiter = world.waiter(move |pause| match pause {
        1 => state.edit(|local| local.work = Some(Work::Building(commit("bbbbbbb")))),
        2 => {
            // The hub restarted first; this daemon follows.
            hub.set_fleet_versions(fleet("bbbbbbb", vec![machine("jarvis", "aaaaaaa", 5.0)]));
        }
        3 => {
            state.edit(|local| {
                local.own = Some(commit("bbbbbbb"));
                local.work = None;
            });
            hub.set_fleet_versions(fleet(
                "bbbbbbb",
                // A machine switched off for an hour does not hold the deploy back.
                vec![
                    machine("jarvis", "bbbbbbb", 5.0),
                    machine("cygnus", "aaaaaaa", 3600.0),
                ],
            ));
        }
        _ => {}
    });
    let mut said = Vec::new();
    let mut progress = |line: &str| {
        said.push(line.to_owned());
        true
    };
    let reply = waiter.wait(&commit("bbbbbbb"), 600, Some(&mut progress), &|| false);
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
    assert!(
        reply
            .text
            .starts_with("deployed bbbbbbb runs on every machine")
    );
    assert!(
        reply
            .text
            .contains("cygnus aaaaaaa, not heard from for 60 min")
    );
    assert_eq!(world.state.checks_asked.load(Ordering::SeqCst), 1);
    assert!(
        said.contains(&"this daemon runs aaaaaaa; building bbbbbbb".to_owned()),
        "{said:?}"
    );
    // Seconds of pauses, not a sleep.
    assert_eq!(world.clock.now(), start().plus_seconds(3.0 * LOOK_EVERY_S));
}

#[test]
fn a_wait_on_a_rolled_back_release_exits_2_with_the_reason() {
    let world = World::new("aaaaaaa", "bbbbbbb");
    let state = world.state.clone();
    let waiter = world.waiter(move |_| {
        *state.rejected.lock().unwrap() = Some((
            commit("bbbbbbb"),
            "bbbbbbb failed its selftest: bad migration".to_owned(),
        ));
    });
    let reply = waiter.wait(&commit("bbbbbbb"), 600, None, &|| false);
    assert_eq!(reply.exit, Exit::CheckFailed);
    assert!(
        reply
            .text
            .contains("rolled back bbbbbbb on this machine: bbbbbbb failed its selftest"),
        "{}",
        reply.text
    );

    let world = World::new("aaaaaaa", "bbbbbbb");
    world.state.edit(|local| {
        local.work = Some(Work::Failed(commit("bbbbbbb"), "cargo failed".to_owned()));
    });
    let reply = world
        .waiter(|_| {})
        .wait(&commit("bbbbbbb"), 600, None, &|| false);
    assert_eq!(reply.exit, Exit::CheckFailed);
    assert!(reply.text.contains("bbbbbbb failed to build: cargo failed"));
}

#[test]
fn a_wait_that_ends_first_exits_5_or_4_for_an_unreachable_hub() {
    let world = World::new("aaaaaaa", "bbbbbbb");
    let reply = world
        .waiter(|_| {})
        .wait(&commit("bbbbbbb"), 10, None, &|| false);
    assert_eq!(reply.exit, Exit::Running);
    assert_eq!(reply.handle.unwrap().as_str(), "deploy-bbbbbbb");
    assert!(
        reply
            .text
            .contains("the hub runs aaaaaaa, behind the release")
    );
    assert!(reply.text.ends_with("next     ignite wait deploy-bbbbbbb"));
    assert_eq!(world.clock.now(), start().plus_seconds(10.0));

    world.hub.set_unreachable(true);
    let reply = world
        .waiter(|_| {})
        .wait(&commit("bbbbbbb"), 10, None, &|| false);
    assert_eq!(reply.exit, Exit::Unreachable);

    // A client that left ends the wait at once.
    let reply = world
        .waiter(|_| {})
        .wait(&commit("bbbbbbb"), 600, None, &|| true);
    assert_eq!(reply.exit, Exit::Unreachable);
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
        clock: Arc::new(FakeClock::new(start())),
        hub: Arc::new(FakeHub::default()),
        git: Arc::new(FakeGit::default()),
        launcher: Arc::new(FakeLauncher::default()),
        builds: Arc::new(FakeBuilds::default()),
        session: None,
        tree: None,
    }
}

/// `ignite wait deploy-<commit>` goes to the deploy waiter, not to the daemon's handles.
#[test]
fn the_wait_verb_hands_deploy_handles_to_the_waiter() {
    let ctx = context();
    let handles = Arc::new(Handles::new(ctx.clock.clone()));
    let steps = Arc::new(crate::steps::Steps::new(ctx.clock.clone()));
    let request = Request {
        verb: "wait".to_owned(),
        args: vec!["deploy-bbbbbbb".to_owned()],
        cwd: PathBuf::from("/repo"),
        session: None,
        wait_s: 0,
        pid: None,
    };
    let slot: Slot = Box::leak(Box::new(OnceLock::new()));
    let verb = WaitVerb::new(handles.clone(), steps.clone()).with_deploys(slot);
    assert_eq!(verb.run(&ctx, &request).exit, Exit::Usage);

    let world = World::new("bbbbbbb", "bbbbbbb");
    let _ = slot.set(Arc::new(world.waiter(|_| {})));
    let reply = verb.run(&ctx, &request);
    assert_eq!(reply.exit, Exit::Success, "{}", reply.text);
}
