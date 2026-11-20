//! The load outside Ignite: the machine's busy cores less everything the daemon's process tree
//! uses, read against every logical core, the express lane's included.

use std::collections::BTreeMap;

use ignite_core::config::Floor;
use ignite_core::protocol::Class;

use std::sync::Arc;

use super::{T0, ask, at, limits, scheduler};
use crate::sched::{Ledger, Limits};

/// Records a reading of the daemon's tree and of grant `id` one second apart at `seconds` and
/// `seconds + 1`: the tree used `tree` cores and the grant `grant` cores over that second.
fn read_second(ledger: &mut Ledger, id: u64, seconds: f64, tree: f64, grant: f64) {
    ledger.observe_trees(at(seconds), Some(&[(1, 0.0)]));
    ledger.observe_use(id, at(seconds), 0.0);
    ledger.observe_trees(at(seconds + 1.0), Some(&[(1, tree)]));
    ledger.observe_use(id, at(seconds + 1.0), grant);
}

/// On 2026-11-18 the scheduler reported 8 to 11 cores busy outside Ignite while processes
/// outside it used 3.5 to 4: express runs, verb processes and the daemon itself use CPU that no
/// grant's command does, and all of it read as outside load.
#[test]
fn what_the_daemon_runs_outside_its_grants_is_not_outside_load() {
    let mut ledger = Ledger::new(limits());
    let grant = ledger.enqueue(ask(Class::Agent, 8), T0);
    ledger.step(T0);
    // The grant's command used 2 cores; the daemon's tree, an express run among it, used 10.
    read_second(&mut ledger, grant, 0.0, 10.0, 2.0);
    // 18 of 24 cores busy: 10 are Ignite's, so 8 are outside it, not 16.
    ledger.observe(Some(0.75), None);
    assert_eq!(ledger.status().outside, 8);
}

/// The tree's reading can miss time, as when a process exits before its parent reaps it, so
/// Ignite never counts less than its grants' commands used.
#[test]
fn ignite_counts_at_least_what_its_grants_used() {
    let mut ledger = Ledger::new(limits());
    let grant = ledger.enqueue(ask(Class::Agent, 8), T0);
    ledger.step(T0);
    read_second(&mut ledger, grant, 0.0, 1.0, 6.0);
    ledger.observe(Some(0.5), None);
    assert_eq!(ledger.status().outside, 6);
}

/// A grant a previous daemon started still runs, but its supervisor now belongs to launchd, not
/// to this daemon's tree, so its use is added to the tree's.
#[test]
fn a_grant_a_previous_daemon_started_is_counted_beside_the_tree() {
    let mut ledger = Ledger::new(limits());
    let adopted = ledger.adopt(Class::Agent, 4, Vec::new(), None, None, false);
    read_second(&mut ledger, adopted, 0.0, 6.0, 4.0);
    // 18 busy: 6 in the tree and 4 in the adopted command, so 8 outside.
    ledger.observe(Some(0.75), None);
    assert_eq!(ledger.status().outside, 8);
}

/// A failed reading of the tree forgets the earlier ones, so a rate from long ago is never
/// used: until two readings follow each other again, only the grants count.
#[test]
fn a_failed_tree_reading_counts_only_the_grants() {
    let mut ledger = Ledger::new(limits());
    let grant = ledger.enqueue(ask(Class::Agent, 8), T0);
    ledger.step(T0);
    read_second(&mut ledger, grant, 0.0, 10.0, 2.0);
    ledger.observe_trees(at(2.0), None);
    ledger.observe(Some(0.75), None);
    assert_eq!(ledger.status().outside, 16);
}

/// The busy fraction covers every logical core. With half of a 24-core machine reserved for the
/// express lane and three quarters of it busy outside Ignite, 18 cores are busy: 12 on the idle
/// express cores and 6 on the cores the scheduler hands out. Read against the 12 it hands out,
/// it was 9.
#[test]
fn the_busy_fraction_is_read_against_every_logical_core() {
    let mut ledger = Ledger::new(Limits {
        cores: 12,
        reserved: 12,
        tip_build: 0,
        floor: Floor::default(),
        resources: BTreeMap::new(),
    });
    ledger.observe(Some(0.75), None);
    assert_eq!(ledger.status().outside, 6);
    ledger.enqueue(ask(Class::Agent, 8), T0);
    assert_eq!(ledger.step(T0).granted[0].cores, 6);
}

/// A verb process that a previous daemon started, which launchd adopted when that daemon
/// exited, and the hub, which its own login service starts.
const ADOPTED_VERB: u32 = 4_000_001;
const HUB: u32 = 4_000_002;

/// Samples the scheduler twice, a second apart, with 18 of 24 cores busy: the daemon's tree
/// used 2 cores over that second and each process `roots` lists (by the fake host's PID) its
/// share of `listed` cores.
fn outside_after_sampling(roots: &[u32], listed: &[(u32, f64)]) -> u32 {
    let (scheduler, clock, host) = scheduler();
    let daemon = std::process::id();
    let roots = roots.to_vec();
    scheduler.count_as_ignite(Arc::new(move || roots.clone()));
    *host.busy.lock().unwrap() = 0.75;
    {
        let mut trees = host.trees.lock().unwrap();
        trees.insert(daemon, 100.0);
        for &(pid, _) in listed {
            trees.insert(pid, 50.0);
        }
    }
    scheduler.sample();
    clock.advance(1.0);
    {
        let mut trees = host.trees.lock().unwrap();
        trees.insert(daemon, 102.0);
        for &(pid, cores) in listed {
            trees.insert(pid, 50.0 + cores);
        }
    }
    scheduler.sample();
    scheduler.status().outside
}

/// On 2026-11-19 jarvis read 10 cores busy outside Ignite, while a verb process from an older
/// release, adopted by launchd, used most of a core outside the daemon's tree.
#[test]
fn a_verb_process_outside_the_daemons_tree_is_not_outside_load() {
    // 18 busy: 2 in the daemon's tree and 3 in the adopted verb process, so 13 outside.
    assert_eq!(
        outside_after_sampling(&[ADOPTED_VERB], &[(ADOPTED_VERB, 3.0)]),
        13
    );
}

/// The hub, which launchd starts as a service of its own, used most of a core that read as
/// load outside Ignite.
#[test]
fn the_hubs_process_is_not_outside_load() {
    // 18 busy: 2 in the daemon's tree and 1 in the hub, so 15 outside.
    assert_eq!(outside_after_sampling(&[HUB], &[(HUB, 1.0)]), 15);
}

/// A listed PID that no longer runs counts as nothing, and a process the list does not name
/// is outside load as before.
#[test]
fn a_listed_process_that_has_exited_counts_as_nothing() {
    assert_eq!(outside_after_sampling(&[HUB], &[]), 16);
    // The hub runs but is not listed, as when it holds no lock: its core is outside load.
    assert_eq!(outside_after_sampling(&[], &[(HUB, 1.0)]), 16);
}

/// Each root keeps its own readings, so a root that first appears in the second sample,
/// carrying hours of CPU time, adds nothing until it has been read twice.
#[test]
fn a_root_read_once_adds_nothing() {
    let (scheduler, clock, host) = scheduler();
    let daemon = std::process::id();
    scheduler.count_as_ignite(Arc::new(|| vec![HUB]));
    *host.busy.lock().unwrap() = 0.75;
    host.trees.lock().unwrap().insert(daemon, 100.0);
    scheduler.sample();
    clock.advance(1.0);
    {
        let mut trees = host.trees.lock().unwrap();
        trees.insert(daemon, 102.0);
        trees.insert(HUB, 36_000.0);
    }
    scheduler.sample();
    assert_eq!(scheduler.status().outside, 16);
}
