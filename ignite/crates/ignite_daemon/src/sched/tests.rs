use std::collections::BTreeMap;
use std::io;
use std::sync::{Arc, Mutex};
use std::thread;

use ignite_core::config::Floor;
use ignite_core::fakes::FakeClock;
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

#[test]
fn ten_commands_starting_at_once_share_the_cores() {
    let mut ledger = Ledger::new(limits());
    let ids: Vec<u64> = (0..10)
        .map(|_| ledger.enqueue(ask(Class::Agent, 16), T0))
        .collect();
    let step = ledger.step(T0);
    let cores: Vec<u32> = step.granted.iter().map(|g| g.cores).collect();
    // 24 cores among 10: four get 3 and six get 2, earliest arrivals first.
    assert_eq!(cores, [3, 3, 3, 3, 2, 2, 2, 2, 2, 2]);
    assert_eq!(step.granted.iter().map(|g| g.id).collect::<Vec<_>>(), ids);
}

#[test]
fn a_small_request_leaves_its_share_to_the_others() {
    let mut ledger = Ledger::new(limits());
    ledger.enqueue(ask(Class::Agent, 2), T0);
    ledger.enqueue(ask(Class::Agent, 16), T0);
    ledger.enqueue(ask(Class::Agent, 16), T0);
    let cores: Vec<u32> = ledger.step(T0).granted.iter().map(|g| g.cores).collect();
    assert_eq!(cores, [2, 11, 11]);
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
    assert_eq!(step.granted.len(), 5);
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
}

impl HostCpu for FakeHost {
    fn cores(&self) -> u32 {
        24
    }

    fn busy(&self) -> io::Result<f64> {
        Ok(*self.busy.lock().unwrap())
    }

    fn free_memory_mb(&self) -> io::Result<u64> {
        Err(io::Error::other("not read"))
    }
}

fn scheduler() -> (Arc<Scheduler>, Arc<FakeClock>, Arc<FakeHost>) {
    let clock = Arc::new(FakeClock::new(T0));
    let host = Arc::new(FakeHost {
        busy: Mutex::new(0.0),
    });
    let scheduler = Scheduler::new(limits(), clock.clone(), host.clone());
    (Arc::new(scheduler), clock, host)
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
    let adopted = ledger.adopt(Class::Agent, 4, vec!["oracle".to_owned()], None);
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
