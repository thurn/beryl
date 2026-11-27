//! Emergencies and rushes: scheduled ahead of every class, behind only a tip round's builds.

use ignite_core::ids::TicketId;
use ignite_core::protocol::Class;
use ignite_core::traits::Reserve;

use super::{T0, ask, at, limits};
use crate::sched::{GrantRequest, Ledger, Urgency};

fn urgent(class: Class, cores: u32, urgency: Urgency) -> GrantRequest {
    GrantRequest {
        urgency: Some(urgency),
        ..ask(class, cores)
    }
}

fn granted(ledger: &mut Ledger, seconds: f64) -> Vec<(u64, u32)> {
    ledger
        .step(at(seconds))
        .granted
        .iter()
        .map(|g| (g.id, g.cores))
        .collect()
}

#[test]
fn a_rush_goes_after_tip_builds_and_emergencies_and_before_landings_and_master_blockers() {
    let mut ledger = Ledger::new(limits());
    let blocker = ledger.enqueue(
        GrantRequest {
            priority_ticket: Some(TicketId::new("FIX").unwrap()),
            ..ask(Class::Agent, 8)
        },
        T0,
    );
    let land = ledger.enqueue(ask(Class::Land, 8), T0);
    let rush = ledger.enqueue(urgent(Class::Agent, 8, Urgency::Rush), T0);
    let emergency = ledger.enqueue(urgent(Class::Agent, 8, Urgency::Emergency), T0);
    let tip_build = ledger.enqueue(
        GrantRequest {
            reserve: Some(Reserve::TipBuild),
            ..ask(Class::Tip, 8)
        },
        T0,
    );
    // 24 cores: the first three get 8 each and the others wait.
    assert_eq!(
        granted(&mut ledger, 0.0),
        [(tip_build, 8), (emergency, 8), (rush, 8)]
    );
    assert!(ledger.release(rush));
    assert_eq!(granted(&mut ledger, 1.0), [(land, 8)]);
    assert!(ledger.release(land));
    assert_eq!(granted(&mut ledger, 2.0), [(blocker, 8)]);
}

#[test]
fn a_rush_may_take_every_shared_core() {
    let mut ledger = Ledger::new(limits());
    let rush = ledger.enqueue(urgent(Class::Agent, 64, Urgency::Rush), T0);
    assert_eq!(granted(&mut ledger, 0.0), [(rush, 24)]);
}

#[test]
fn a_rush_leaves_the_batch_floor() {
    let mut ledger = Ledger::new(limits());
    let batch = ledger.enqueue(ask(Class::Batch, 8), T0);
    let rush = ledger.enqueue(urgent(Class::Agent, 64, Urgency::Rush), T0);
    let mut grants = granted(&mut ledger, 0.0);
    grants.sort_unstable();
    assert_eq!(grants, [(batch, 8), (rush, 16)]);
}

/// Compiling grants of lower classes give cores back for a rush, one per step, and the rush's
/// own compiling grant gives none back for a landing that arrives after it.
#[test]
fn compiling_grants_shrink_for_a_rush_and_a_rush_does_not_shrink() {
    let mut ledger = Ledger::new(limits());
    let compiling = |request: GrantRequest| GrantRequest {
        shrinks: true,
        ..request
    };
    let first = ledger.enqueue(compiling(ask(Class::Agent, 16)), T0);
    let second = ledger.enqueue(compiling(ask(Class::Agent, 8)), T0);
    assert_eq!(granted(&mut ledger, 0.0), [(second, 8), (first, 16)]);

    // A test run, which keeps the threads it starts with, waits for the cores it wants.
    let rush = ledger.enqueue(urgent(Class::Agent, 8, Urgency::Rush), at(1.0));
    let mut rush_cores = None;
    for t in 1..10 {
        let step = ledger.step(at(f64::from(t)));
        if let Some(grant) = step.granted.iter().find(|g| g.id == rush) {
            rush_cores = Some(grant.cores);
            break;
        }
    }
    assert_eq!(rush_cores, Some(8));
    assert_eq!(ledger.status().rush_running, 1);
    assert!(ledger.release(rush));
    assert!(ledger.release(first));
    assert!(ledger.release(second));

    let build = ledger.enqueue(compiling(urgent(Class::Agent, 24, Urgency::Rush)), at(20.0));
    assert_eq!(granted(&mut ledger, 20.0), [(build, 24)]);
    ledger.enqueue(ask(Class::Land, 2), at(21.0));
    for t in 21..40 {
        let step = ledger.step(at(f64::from(t)));
        assert!(step.resized.is_empty(), "{step:?}");
        assert!(step.granted.is_empty(), "{step:?}");
    }
}
