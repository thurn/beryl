//! The load outside Ignite: the machine's busy cores less everything the daemon's process tree
//! uses, read against every logical core, the express lane's included.

use std::collections::BTreeMap;

use ignite_core::config::Floor;
use ignite_core::protocol::Class;

use super::{T0, ask, at, limits};
use crate::sched::{Ledger, Limits};

/// Records a reading of the daemon's tree and of grant `id` one second apart at `seconds` and
/// `seconds + 1`: the tree used `tree` cores and the grant `grant` cores over that second.
fn read_second(ledger: &mut Ledger, id: u64, seconds: f64, tree: f64, grant: f64) {
    ledger.observe_tree(at(seconds), Some(0.0));
    ledger.observe_use(id, at(seconds), 0.0);
    ledger.observe_tree(at(seconds + 1.0), Some(tree));
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
    ledger.observe_tree(at(2.0), None);
    ledger.observe(Some(0.75), None);
    assert_eq!(ledger.status().outside, 16);
}

/// The busy fraction covers every logical core. With half of a 24-core machine reserved for the
/// express lane and half of it busy outside Ignite, 12 cores are busy, which is every core the
/// scheduler may hand out; read against the 12 it hands out, it was 6.
#[test]
fn the_busy_fraction_is_read_against_every_logical_core() {
    let mut ledger = Ledger::new(Limits {
        cores: 12,
        reserved: 12,
        floor: Floor::default(),
        resources: BTreeMap::new(),
    });
    ledger.observe(Some(0.5), None);
    assert_eq!(ledger.status().outside, 12);
    ledger.enqueue(ask(Class::Agent, 4), T0);
    assert!(ledger.step(T0).granted.is_empty());
}
