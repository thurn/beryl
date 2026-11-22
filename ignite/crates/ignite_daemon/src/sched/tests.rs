use std::collections::BTreeMap;
use std::io;
use std::sync::{Arc, Mutex};
use std::thread;

use ignite_core::capacity::{Floor, MachineFacts, derive};
use ignite_core::config::{Config, Os};
use ignite_core::fakes::FakeClock;
use ignite_core::manifest::Manifest;
use ignite_core::protocol::Class;
use ignite_core::time::Timestamp;
use ignite_os::hostcpu::HostCpu;

use super::{Grant, GrantRequest, Ledger, Limits, Scheduler};

const T0: Timestamp = Timestamp::from_millis(1_800_000_000_000);

fn at(seconds: f64) -> Timestamp {
    T0.plus_seconds(seconds)
}

/// 24 cores, so one command takes at most 16; floors of 2 for `tip` and 8 for `batch`.
fn limits() -> Limits {
    Limits {
        cores: 24,
        reserved: 0,
        tip_build: 0,
        floor: Floor { batch: 8, tip: 2 },
        resources: BTreeMap::from([("oracle".to_owned(), 1), ("browser-pages".to_owned(), 3)]),
    }
}

fn ask(class: Class, cores: u32) -> GrantRequest {
    GrantRequest {
        class,
        cores,
        resources: Vec::new(),
        memory_mb: None,
        shrinks: false,
        grows: false,
        cores_file: None,
        reserve: None,
        priority_ticket: None,
    }
}

fn with_resource(class: Class, cores: u32, resource: &str) -> GrantRequest {
    GrantRequest {
        resources: vec![resource.to_owned()],
        ..ask(class, cores)
    }
}

fn granted_cores(ledger: &Ledger) -> u32 {
    ledger.status().granted
}

fn capacity_config() -> Config {
    Config::parse(
        "ignite.toml",
        r#"
        [repository]
        remote = "origin"
        staging = "s"
        master = "m"
        trees = "t"
        checks = "c"
        [hub]
        url = "http://h"
        project = "p"
        [gate]
        threshold = 1.0
        [resources.browser-pages]
        per_core = 0.5
        [machine.m]
        express_cores = 0
        tip_build_cores = 0
        floor = { batch = 99, tip = 99 }
        resources = { browser-pages = 99, oracle = 2 }
        "#,
    )
    .unwrap()
}

fn derived_limits(os: Os, physical_cores: u32, logical_cores: u32) -> Limits {
    let config = capacity_config();
    let facts = MachineFacts {
        os,
        physical_cores,
        logical_cores,
        memory_mb: 0,
        disk_total_gb: 0,
    };
    let manifest = Manifest::from_files(&[(
        "ui.toml".into(),
        r#"
        [family.ui]
        kind = "regression"
        adapter = "jsonl"
        platforms = ["macos"]
        list = "list"
        run = "run"
        inputs = ["src"]
        resources = ["browser-pages"]
        "#
        .into(),
    )])
    .unwrap();
    Limits::for_machine(
        &config,
        &config.machine["m"],
        &manifest,
        &facts,
        &derive(&facts),
    )
}

#[test]
fn scheduler_uses_derived_floors_and_page_slots_for_available_checks() {
    for (os, physical, logical, batch, pages) in [
        (Os::Macos, 18, 18, 6, Some(9)),
        (Os::Macos, 10, 10, 3, Some(5)),
        (Os::Windows, 12, 20, 4, None),
    ] {
        let limits = derived_limits(os, physical, logical);
        assert_eq!(limits.floor, Floor { batch, tip: 2 });
        assert_eq!(limits.resources.get("browser-pages").copied(), pages);
        assert_eq!(limits.resources["oracle"], 2);
    }
}

/// Sharing the cores evenly among every waiting request ran each of eighteen commands on about
/// one core, so all of them finished late. Requests are served in turn instead: the first gets
/// what it asked for before the next gets anything.
#[test]
fn ten_commands_starting_at_once_are_served_in_turn() {
    let mut ledger = Ledger::new(limits());
    let ids: Vec<u64> = (0..10)
        .map(|_| ledger.enqueue(ask(Class::Agent, 16), T0))
        .collect();
    let step = ledger.step(T0);
    let granted: Vec<(u64, u32)> = step.granted.iter().map(|g| (g.id, g.cores)).collect();
    // 24 cores: the earliest gets its 16 (two thirds of the machine), the next the other 8.
    assert_eq!(granted, [(ids[0], 16), (ids[1], 8)]);
    assert_eq!(ledger.status().classes[1].waiting, 8);
}

/// A one-card change's listing steps, which want one core each, waited behind ten-core builds.
/// The smallest request of a class goes first.
#[test]
fn the_smallest_request_starts_first() {
    let mut ledger = Ledger::new(limits());
    let big = ledger.enqueue(ask(Class::Agent, 16), T0);
    let other = ledger.enqueue(ask(Class::Agent, 16), T0);
    let small = ledger.enqueue(ask(Class::Agent, 2), at(1.0));
    let granted: Vec<(u64, u32)> = ledger
        .step(at(1.0))
        .granted
        .iter()
        .map(|g| (g.id, g.cores))
        .collect();
    assert_eq!(granted, [(small, 2), (big, 16), (other, 6)]);
}

/// A stream of small requests must not hold a large one back for ever: once it has waited
/// `AGED_SECONDS` it goes ahead of every request that has waited less, whatever its size.
#[test]
fn a_request_that_waited_long_goes_ahead_of_smaller_ones() {
    use crate::sched::AGED_SECONDS;
    let mut ledger = Ledger::new(limits());
    let holder = ledger.enqueue(ask(Class::Agent, 16), T0);
    // One core is left free.
    ledger.enqueue(ask(Class::Agent, 7), T0);
    ledger.step(T0);
    let big = ledger.enqueue(ask(Class::Agent, 16), at(1.0));
    // Before it has waited long, a later small request goes ahead of it.
    let early = ledger.enqueue(ask(Class::Agent, 1), at(2.0));
    let step = ledger.step(at(3.0));
    assert_eq!(step.granted[0].id, early);
    assert!(ledger.release(early));

    let late = ledger.enqueue(ask(Class::Agent, 1), at(AGED_SECONDS));
    assert!(ledger.release(holder));
    let step = ledger.step(at(AGED_SECONDS + 1.0));
    let granted: Vec<(u64, u32)> = step.granted.iter().map(|g| (g.id, g.cores)).collect();
    assert_eq!(granted, [(big, 16), (late, 1)]);
}

#[test]
fn one_command_takes_at_most_two_thirds_of_the_machine() {
    let mut ledger = Ledger::new(limits());
    ledger.enqueue(ask(Class::Land, 64), T0);
    assert_eq!(ledger.step(T0).granted[0].cores, 16);
}

#[test]
fn a_land_request_stops_an_idle_command_and_takes_its_cores() {
    let mut ledger = Ledger::new(limits());
    let idle = ledger.enqueue(ask(Class::Idle, 16), T0);
    assert_eq!(ledger.step(T0).granted[0].cores, 16);
    let other = ledger.enqueue(ask(Class::Agent, 8), T0);
    assert_eq!(ledger.step(T0).granted[0].id, other);

    let land = ledger.enqueue(ask(Class::Land, 16), at(1.0));
    let step = ledger.step(at(1.0));
    assert!(step.granted.is_empty());
    assert_eq!(step.stop, [idle]);
    // Asked once: a later step does not ask again while the command ends.
    assert!(ledger.step(at(2.0)).stop.is_empty());

    assert!(ledger.release(idle));
    let step = ledger.step(at(3.0));
    assert_eq!(step.granted.len(), 1);
    assert_eq!(step.granted[0].id, land);
    assert_eq!(step.granted[0].cores, 16);
    assert_eq!(step.granted[0].queued_s, 2);
}

#[test]
fn batch_keeps_its_floor_under_load() {
    let mut ledger = Ledger::new(limits());
    let batch = ledger.enqueue(ask(Class::Batch, 16), T0);
    for _ in 0..6 {
        ledger.enqueue(ask(Class::Agent, 16), T0);
    }
    ledger.enqueue(ask(Class::Land, 16), T0);
    let step = ledger.step(T0);
    let batch_grant = step
        .granted
        .iter()
        .find(|g| g.id == batch)
        .expect("batch starts");
    assert_eq!(batch_grant.cores, 8);
    assert_eq!(granted_cores(&ledger), 24);

    // More higher-class work arrives; batch keeps what it holds and nobody takes the floor.
    ledger.enqueue(ask(Class::Land, 16), at(1.0));
    let step = ledger.step(at(1.0));
    assert!(step.granted.is_empty());
    assert!(step.stop.is_empty());
    assert!(ledger.holds(batch));
}

#[test]
fn a_floor_is_not_held_back_when_its_class_has_no_work() {
    let mut ledger = Ledger::new(limits());
    for _ in 0..2 {
        ledger.enqueue(ask(Class::Agent, 16), T0);
    }
    assert_eq!(granted_cores(&ledger), 0);
    ledger.step(T0);
    assert_eq!(granted_cores(&ledger), 24);
}

#[test]
fn a_running_floor_class_with_nothing_waiting_holds_back_no_cores() {
    let mut ledger = Ledger::new(limits());
    ledger.enqueue(ask(Class::Batch, 1), T0);
    ledger.step(T0);
    assert_eq!(granted_cores(&ledger), 1);
    // Batch runs one command on one core and asks for nothing more, so its floor of eight
    // keeps nothing back: the agents share all 23 other cores.
    for _ in 0..30 {
        ledger.enqueue(ask(Class::Agent, 1), at(1.0));
    }
    let step = ledger.step(at(1.0));
    assert_eq!(step.granted.len(), 23);
    assert_eq!(granted_cores(&ledger), 24);
}

#[test]
fn a_lower_class_starts_nothing_while_a_higher_one_waits() {
    let mut ledger = Ledger::new(limits());
    for _ in 0..30 {
        ledger.enqueue(ask(Class::Agent, 1), T0);
    }
    let tip = ledger.enqueue(ask(Class::Tip, 8), T0);
    ledger.enqueue(ask(Class::Idle, 1), T0);
    let step = ledger.step(T0);
    // The agents take all but the tip floor and eight of them still wait, so `tip` gets only
    // its floor, not the 8 it asked for, and `idle` gets nothing.
    assert_eq!(step.granted.len(), 23);
    let tip_grant = step
        .granted
        .iter()
        .find(|g| g.id == tip)
        .expect("tip starts");
    assert_eq!(tip_grant.cores, 2);
    let status = ledger.status();
    assert_eq!(status.classes[1].waiting, 8);
    assert_eq!(status.classes[4].waiting, 1);
}

#[test]
fn tip_takes_its_floor_even_while_a_higher_class_waits() {
    let mut ledger = Ledger::new(limits());
    let tip = ledger.enqueue(ask(Class::Tip, 8), T0);
    for _ in 0..4 {
        ledger.enqueue(ask(Class::Agent, 16), T0);
    }
    let step = ledger.step(T0);
    let tip_grant = step
        .granted
        .iter()
        .find(|g| g.id == tip)
        .expect("tip starts");
    assert_eq!(tip_grant.cores, 2);
    // The agents take the other 22 in turn: 16 and 6, and two wait.
    assert_eq!(step.granted.len(), 3);
}

#[test]
fn a_resource_of_capacity_one_runs_one_holder_at_a_time() {
    let mut ledger = Ledger::new(limits());
    let first = ledger.enqueue(with_resource(Class::Agent, 2, "oracle"), T0);
    let second = ledger.enqueue(with_resource(Class::Land, 2, "oracle"), T0);
    let plain = ledger.enqueue(ask(Class::Batch, 2), T0);
    let step = ledger.step(T0);
    // `land` goes first; the agent waits for the oracle but does not hold up `batch`.
    let ids: Vec<u64> = step.granted.iter().map(|g| g.id).collect();
    assert_eq!(ids, [second, plain]);
    assert_eq!(ledger.status().resources[1].held, 1);
    assert!(ledger.step(T0).granted.is_empty());
    ledger.release(second);
    assert_eq!(ledger.step(T0).granted[0].id, first);
}

#[test]
fn a_resource_the_machine_does_not_declare_has_capacity_one() {
    let mut ledger = Ledger::new(limits());
    ledger.enqueue(with_resource(Class::Agent, 1, "bench-vm"), T0);
    ledger.enqueue(with_resource(Class::Agent, 1, "bench-vm"), T0);
    assert_eq!(ledger.step(T0).granted.len(), 1);
}

#[test]
fn nothing_is_granted_beyond_what_processes_outside_ignite_leave() {
    let mut ledger = Ledger::new(limits());
    ledger.observe(Some(0.5), None);
    for _ in 0..3 {
        ledger.enqueue(ask(Class::Agent, 16), T0);
    }
    ledger.step(T0);
    assert_eq!(granted_cores(&ledger), 12);

    // Ignite's own commands are not counted as outside load: the machine is now fully busy,
    // half of it ours.
    ledger.observe(Some(1.0), None);
    assert_eq!(ledger.status().outside, 12);
    ledger.enqueue(ask(Class::Agent, 4), at(1.0));
    assert!(ledger.step(at(1.0)).granted.is_empty());

    // The outside processes finish.
    ledger.observe(Some(0.5), None);
    assert_eq!(ledger.status().outside, 0);
    assert_eq!(ledger.step(at(2.0)).granted[0].cores, 4);
}

#[test]
fn outside_load_fills_the_idle_express_cores_first() {
    // 18 cores, 3 kept for the express lane.
    let mut ledger = Ledger::new(Limits {
        cores: 15,
        reserved: 3,
        ..limits()
    });
    // 7 cores busy and none of them Ignite's: 3 fit on the idle express cores, 4 are outside.
    ledger.observe(Some(7.0 / 18.0), None);
    assert_eq!(ledger.status().outside, 4);
    ledger.enqueue(ask(Class::Agent, 10), T0);
    assert_eq!(ledger.step(T0).granted[0].cores, 10);

    // The express lane, which runs in the daemon's tree, now uses its 3 cores: the same
    // outside processes count fully again.
    ledger.observe_trees(at(1.0), Some(&[(1, 0.0)]));
    ledger.observe_trees(at(2.0), Some(&[(1, 13.0)]));
    ledger.observe(Some(1.0), None);
    assert_eq!(ledger.status().outside, 5);
}

#[test]
fn idle_work_starts_only_under_seventy_percent_busy() {
    let mut ledger = Ledger::new(limits());
    ledger.observe(Some(0.5), None);
    ledger.enqueue(ask(Class::Idle, 16), T0);
    // 70% of 24 is 16 cores; 12 are busy outside Ignite, so idle work gets 4.
    assert_eq!(ledger.step(T0).granted[0].cores, 4);
    ledger.observe(Some(0.75), None);
    ledger.enqueue(ask(Class::Idle, 16), at(1.0));
    assert!(ledger.step(at(1.0)).granted.is_empty());
}

#[test]
fn a_command_waits_for_the_memory_its_family_needs() {
    let mut ledger = Ledger::new(limits());
    ledger.observe(Some(0.0), Some(10_000));
    let big = GrantRequest {
        memory_mb: Some(8_000),
        ..ask(Class::Agent, 4)
    };
    let first = ledger.enqueue(big.clone(), T0);
    let second = ledger.enqueue(big, T0);
    let step = ledger.step(T0);
    assert_eq!(
        step.granted.iter().map(|g| g.id).collect::<Vec<_>>(),
        [first]
    );
    // A new reading that still shows too little memory keeps it waiting.
    ledger.observe(Some(0.0), Some(2_000));
    assert!(ledger.step(at(1.0)).granted.is_empty());
    ledger.release(first);
    // With nothing else running it starts anyway, since waiting would never end.
    assert_eq!(ledger.step(at(2.0)).granted[0].id, second);
}

#[test]
fn a_waiting_request_can_be_withdrawn() {
    let mut ledger = Ledger::new(limits());
    ledger.enqueue(ask(Class::Agent, 16), T0);
    ledger.enqueue(ask(Class::Agent, 16), T0);
    ledger.step(T0);
    let waiting = ledger.enqueue(ask(Class::Agent, 4), T0);
    assert!(ledger.release(waiting));
    assert_eq!(ledger.status().classes[1].waiting, 0);
    assert!(!ledger.release(waiting));
}

#[test]
fn a_grant_sets_the_thread_counts() {
    let grant = Grant {
        id: 1,
        cores: 6,
        resources: Vec::new(),
        queued_s: 0,
        cores_file: Some("/runs/1/cores".into()),
        priority_ticket: None,
    };
    let env = grant.env();
    for name in [
        "CARGO_BUILD_JOBS",
        "NEXTEST_TEST_THREADS",
        "RUST_TEST_THREADS",
        "IGNITE_CORES",
    ] {
        assert!(env.contains(&(name.to_owned(), "6".to_owned())), "{name}");
    }
    let file = ("IGNITE_CORES_FILE".to_owned(), "/runs/1/cores".to_owned());
    assert!(env.contains(&file));
}

/// A host whose CPU and memory readings the test sets.
struct FakeHost {
    busy: Mutex<f64>,
    free_memory_mb: Mutex<Option<u64>>,
    /// The CPU seconds each process tree has used so far, by the PID at its root. A PID not
    /// listed does not run.
    trees: Mutex<BTreeMap<u32, f64>>,
}

impl HostCpu for FakeHost {
    fn cores(&self) -> u32 {
        24
    }

    fn busy(&self) -> io::Result<f64> {
        Ok(*self.busy.lock().unwrap())
    }

    fn free_memory_mb(&self) -> io::Result<u64> {
        self.free_memory_mb
            .lock()
            .unwrap()
            .ok_or_else(|| io::Error::other("not read"))
    }

    fn process_tree_cpu_seconds(&self, pid: u32) -> io::Result<f64> {
        self.trees
            .lock()
            .unwrap()
            .get(&pid)
            .copied()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }
}

fn scheduler() -> (Arc<Scheduler>, Arc<FakeClock>, Arc<FakeHost>) {
    let clock = Arc::new(FakeClock::new(T0));
    let host = Arc::new(FakeHost {
        busy: Mutex::new(0.0),
        free_memory_mb: Mutex::new(None),
        trees: Mutex::new(BTreeMap::new()),
    });
    let scheduler = Scheduler::new(limits(), clock.clone(), host.clone());
    (Arc::new(scheduler), clock, host)
}

#[test]
fn try_acquire_grants_what_is_free_now_and_never_waits() {
    let (scheduler, _, _) = scheduler();
    let held = scheduler.try_acquire(&ask(Class::Land, 16)).unwrap();
    assert_eq!(held.cores, 16, "one command may take two thirds of 24");
    let rest = scheduler.try_acquire(&ask(Class::Land, 16)).unwrap();
    assert_eq!(rest.cores, 8, "only what is free is granted");
    assert!(scheduler.try_acquire(&ask(Class::Land, 4)).is_none());
    let status = scheduler.status();
    assert_eq!(
        status.classes[0].waiting, 0,
        "a refused request is withdrawn"
    );
    scheduler.release_id(rest.id);
    assert_eq!(
        scheduler.try_acquire(&ask(Class::Land, 4)).unwrap().cores,
        4
    );
}

#[test]
fn acquire_blocks_until_a_holder_of_the_resource_releases_it() {
    let (scheduler, clock, _) = scheduler();
    let held = scheduler.acquire(&with_resource(Class::Agent, 2, "oracle"));
    let waiter = {
        let scheduler = scheduler.clone();
        thread::spawn(move || scheduler.acquire(&with_resource(Class::Agent, 2, "oracle")))
    };
    // Wait, without a timer, until the second request is queued.
    while scheduler.status().resources[1].waiting == 0 {
        thread::yield_now();
    }
    clock.set(at(5.0));
    scheduler.release(&held);
    let grant = waiter.join().unwrap();
    assert_eq!(grant.queued_s, 5);
    assert_eq!(scheduler.status().resources[1].held, 1);
}

#[test]
fn an_idle_command_is_told_to_stop_when_land_waits() {
    let (scheduler, _, _) = scheduler();
    let idle = scheduler.acquire(&ask(Class::Idle, 16));
    let _agent = scheduler.acquire(&ask(Class::Agent, 8));
    let supervisor = {
        let scheduler = scheduler.clone();
        let idle = idle.clone();
        thread::spawn(move || {
            let stop = scheduler.wait_stop(idle.id);
            scheduler.release(&idle);
            stop
        })
    };
    let land = scheduler.acquire(&ask(Class::Land, 16));
    assert!(supervisor.join().unwrap());
    assert_eq!(land.cores, 16);
    // A grant released without being asked to stop reports false.
    scheduler.release(&land);
    assert!(!scheduler.wait_stop(land.id));
}

#[test]
fn worker_admission_reads_memory_since_the_last_scheduler_sample() {
    let (scheduler, _, host) = scheduler();
    *host.free_memory_mb.lock().unwrap() = Some(8_000);
    scheduler.sample();

    *host.free_memory_mb.lock().unwrap() = Some(512);
    assert_eq!(scheduler.worker_memory(), (true, Some(512)));

    *host.free_memory_mb.lock().unwrap() = None;
    assert_eq!(scheduler.worker_memory(), (false, None));
}

#[test]
fn sample_reads_load_from_outside_ignite() {
    let (scheduler, _, host) = scheduler();
    *host.busy.lock().unwrap() = 0.25;
    scheduler.sample();
    let status = scheduler.status();
    assert_eq!(status.outside, 6);
    assert_eq!(status.free_memory_mb, None);
    assert_eq!(scheduler.acquire(&ask(Class::Agent, 24)).cores, 16);
    assert_eq!(scheduler.acquire(&ask(Class::Agent, 24)).cores, 2);
}

#[test]
fn dispatch_uses_recent_host_cpu_even_when_every_core_is_granted() {
    let (scheduler, clock, host) = scheduler();
    assert_eq!(scheduler.dispatch_cpu_percent(), None);
    let first = scheduler.try_acquire(&ask(Class::Agent, 16)).unwrap();
    let second = scheduler.try_acquire(&ask(Class::Agent, 8)).unwrap();
    assert_eq!(first.cores + second.cores, 24);

    *host.busy.lock().unwrap() = 0.65;
    scheduler.sample();
    assert_eq!(scheduler.status().free_cores(), 0);
    assert_eq!(scheduler.dispatch_cpu_percent(), Some(65));

    clock.set(at(5.0));
    assert_eq!(scheduler.dispatch_cpu_percent(), None);
    scheduler.sample();
    assert_eq!(scheduler.dispatch_cpu_percent(), Some(65));
}

#[test]
fn land_and_a_floor_class_get_a_core_when_outside_load_fills_the_machine() {
    let mut ledger = Ledger::new(limits());
    ledger.observe(Some(1.0), None);
    assert_eq!(ledger.status().outside, 24);
    let agent = ledger.enqueue(ask(Class::Agent, 4), T0);
    let land = ledger.enqueue(ask(Class::Land, 16), T0);
    let tip = ledger.enqueue(ask(Class::Tip, 4), T0);
    let step = ledger.step(T0);
    let granted: Vec<(u64, u32)> = step.granted.iter().map(|g| (g.id, g.cores)).collect();
    assert_eq!(granted, [(land, 1), (tip, 1)]);
    assert!(ledger.holds(land) && ledger.holds(tip) && !ledger.holds(agent));
    // A second land request waits for the first one's core instead of taking another.
    ledger.enqueue(ask(Class::Land, 1), at(1.0));
    assert!(ledger.step(at(1.0)).granted.is_empty());
}

#[test]
fn an_idle_command_holding_a_resource_is_stopped_for_a_higher_class_that_needs_it() {
    let mut ledger = Ledger::new(limits());
    let idle = ledger.enqueue(with_resource(Class::Idle, 2, "oracle"), T0);
    assert_eq!(ledger.step(T0).granted[0].id, idle);
    // Plenty of cores are free: only the resource is missing.
    let agent = ledger.enqueue(with_resource(Class::Agent, 2, "oracle"), T0);
    let step = ledger.step(T0);
    assert!(step.granted.is_empty());
    assert_eq!(step.stop, [idle]);
    assert!(ledger.step(at(1.0)).stop.is_empty(), "asked once");
    ledger.release(idle);
    assert_eq!(ledger.step(at(2.0)).granted[0].id, agent);
}

#[test]
fn draining_stops_idle_work_and_starts_none() {
    let mut ledger = Ledger::new(limits());
    let idle = ledger.enqueue(ask(Class::Idle, 2), T0);
    ledger.step(T0);
    ledger.set_draining(true);
    assert_eq!(ledger.step(at(1.0)).stop, [idle]);
    ledger.release(idle);
    ledger.enqueue(ask(Class::Idle, 2), at(2.0));
    assert!(ledger.step(at(2.0)).granted.is_empty());
    ledger.set_draining(false);
    assert_eq!(ledger.step(at(3.0)).granted.len(), 1);
}

#[test]
fn an_adopted_grant_holds_its_cores_and_resources() {
    let mut ledger = Ledger::new(limits());
    let adopted = ledger.adopt(
        Class::Agent,
        4,
        vec!["oracle".to_owned()],
        None,
        None,
        false,
    );
    assert_eq!(granted_cores(&ledger), 4);
    ledger.enqueue(with_resource(Class::Agent, 2, "oracle"), T0);
    assert!(ledger.step(T0).granted.is_empty(), "oracle has capacity 1");
    ledger.release(adopted);
    assert_eq!(ledger.step(at(1.0)).granted.len(), 1);
}

#[test]
fn a_stopped_grant_is_killed_when_it_is_still_held_after_the_grace() {
    let (scheduler, _, _) = scheduler();
    let grant = scheduler.acquire(&ask(Class::Idle, 2));
    // The clock never moves, so only a grace of nothing has run out.
    assert!(!scheduler.wait_released(grant.id, 0.0));
    scheduler.release(&grant);
    assert!(scheduler.wait_released(grant.id, 10.0));
}

mod elastic;
mod lending;
mod outside;
mod reserve;

/// A verb process asks again for a command after the daemon restarted, passing the moment it
/// first asked. The new daemon's queue puts it ahead of a request first asked for later, even
/// one that reached the new daemon first.
#[test]
fn a_request_asked_again_keeps_its_place_by_when_it_was_first_asked() {
    let mut ledger = Ledger::new(limits());
    let holder = ledger.enqueue(with_resource(Class::Agent, 2, "oracle"), at(10.0));
    assert_eq!(ledger.step(at(10.0)).granted.len(), 1);
    let later = ledger.enqueue(with_resource(Class::Agent, 2, "oracle"), at(5.0));
    let earlier = ledger.enqueue(with_resource(Class::Agent, 2, "oracle"), at(1.0));
    let fresh = ledger.enqueue(with_resource(Class::Agent, 2, "oracle"), at(11.0));
    assert!(ledger.step(at(11.0)).granted.is_empty());
    assert!(ledger.release(holder));
    let step = ledger.step(at(12.0));
    let ids: Vec<u64> = step.granted.iter().map(|g| g.id).collect();
    assert_eq!(ids, [earlier]);
    // Its queued time counts from when it was first asked for.
    assert_eq!(step.granted[0].queued_s, 11);
    assert!(ledger.release(earlier));
    let ids: Vec<u64> = ledger.step(at(13.0)).granted.iter().map(|g| g.id).collect();
    assert_eq!(ids, [later]);
    assert!(ledger.release(later));
    let ids: Vec<u64> = ledger.step(at(14.0)).granted.iter().map(|g| g.id).collect();
    assert_eq!(ids, [fresh]);
}

#[test]
fn adopted_grants_can_pass_the_cores_and_are_named_as_adopted() {
    let mut ledger = Ledger::new(limits());
    ledger.enqueue(ask(Class::Agent, 4), T0);
    ledger.step(T0);
    // A previous daemon's commands are recorded whatever is free, so the total can pass the
    // machine's cores; the log line names which grants were adopted.
    ledger.adopt(Class::Agent, 16, Vec::new(), None, None, false);
    ledger.adopt(Class::Tip, 8, Vec::new(), None, None, false);
    assert_eq!(granted_cores(&ledger), 28);
    let mut holders = ledger.holders();
    holders.sort_by_key(|&(_, cores, _)| cores);
    assert_eq!(
        holders,
        [
            (Class::Agent, 4, false),
            (Class::Tip, 8, true),
            (Class::Agent, 16, true)
        ]
    );
}
