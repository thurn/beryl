//! The tip reserve: cores only requests with `Reserve::TipBuild` are granted, so the build
//! checks of a tip round start at once however full the machine is.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use ignite_core::capacity::Floor;
use ignite_core::config::Os;
use ignite_core::fakes::FakeClock;
use ignite_core::ids::TicketId;
use ignite_core::protocol::Class;
use ignite_core::traits::Reserve;

use super::{FakeHost, T0, ask, at, derived_limits, limits};
use crate::sched::{Adoption, GrantRequest, Ledger, Limits, Scheduler};

/// 28 logical cores: 24 shared and 4 kept for the tip round's builds.
fn with_reserve() -> Limits {
    Limits {
        cores: 24,
        reserved: 4,
        tip_build: 4,
        ..limits()
    }
}

fn tip_build(cores: u32) -> GrantRequest {
    GrantRequest {
        reserve: Some(Reserve::TipBuild),
        ..ask(Class::Tip, cores)
    }
}

#[test]
fn the_tip_reserve_is_left_out_of_the_cores_handed_out() {
    let limits = derived_limits(Os::Macos, 18, 18);
    assert_eq!(
        (limits.cores, limits.reserved, limits.tip_build),
        (11, 7, 4)
    );
    assert_eq!(limits.machine_cores(), 18);
    // At least one core is handed out, and the express lane's cores come first.
    let small = derived_limits(Os::Macos, 4, 4);
    assert_eq!((small.cores, small.reserved, small.tip_build), (1, 3, 1));
    assert_eq!(small.machine_cores(), 4);
}

/// Every shared core is granted to agent work and the rest of the machine is busy outside
/// Ignite; a tip round's build still starts at once on the reserve.
#[test]
fn a_tip_build_starts_at_once_on_the_reserve_when_nothing_else_is_free() {
    let mut ledger = Ledger::new(with_reserve());
    ledger.enqueue(ask(Class::Agent, 16), T0);
    ledger.enqueue(ask(Class::Agent, 16), T0);
    ledger.step(T0);
    assert_eq!(ledger.status().granted, 24);
    ledger.observe(Some(1.0), None);
    let build = ledger.enqueue(tip_build(8), T0);
    let step = ledger.step(at(1.0));
    assert_eq!(step.granted.len(), 1);
    assert_eq!((step.granted[0].id, step.granted[0].cores), (build, 4));
    let status = ledger.status();
    assert_eq!((status.tip_build, status.tip_build_used), (4, 4));
    // The shared cores do not count the reserve's.
    assert_eq!(status.granted, 24);
}

#[test]
fn a_tip_build_takes_the_rest_of_what_it_wants_from_the_shared_cores() {
    let mut ledger = Ledger::new(with_reserve());
    ledger.enqueue(ask(Class::Agent, 16), T0);
    ledger.step(T0);
    ledger.enqueue(tip_build(10), T0);
    let step = ledger.step(T0);
    assert_eq!(step.granted[0].cores, 10);
    let status = ledger.status();
    assert_eq!((status.granted, status.tip_build_used), (22, 4));
}

#[test]
fn no_other_request_is_granted_the_reserve() {
    let mut ledger = Ledger::new(Limits {
        floor: Floor::default(),
        ..with_reserve()
    });
    ledger.enqueue(ask(Class::Land, 16), T0);
    ledger.enqueue(ask(Class::Land, 16), T0);
    ledger.enqueue(ask(Class::Tip, 4), T0);
    let step = ledger.step(T0);
    let cores: Vec<u32> = step.granted.iter().map(|g| g.cores).collect();
    assert_eq!(cores, [16, 8], "the tip request without the reserve waits");
    assert_eq!(ledger.status().tip_build_used, 0);
    assert!(ledger.step(at(20.0)).granted.is_empty());
}

#[test]
fn two_tip_builds_share_the_reserve_in_arrival_order() {
    let mut ledger = Ledger::new(with_reserve());
    for _ in 0..2 {
        ledger.enqueue(ask(Class::Land, 16), T0);
    }
    ledger.step(T0);
    let first = ledger.enqueue(tip_build(3), at(1.0));
    let second = ledger.enqueue(tip_build(3), at(2.0));
    let third = ledger.enqueue(tip_build(3), at(3.0));
    let step = ledger.step(at(3.0));
    let granted: Vec<(u64, u32)> = step.granted.iter().map(|g| (g.id, g.cores)).collect();
    assert_eq!(granted, [(first, 3), (second, 1)]);
    // The third waits until a reserved grant ends, and the reserve is free again.
    assert!(ledger.release(first));
    let step = ledger.step(at(4.0));
    let granted: Vec<(u64, u32)> = step.granted.iter().map(|g| (g.id, g.cores)).collect();
    assert_eq!(granted, [(third, 3)]);
    assert!(ledger.release(second));
    assert!(ledger.release(third));
    assert_eq!(ledger.status().tip_build_used, 0);
}

#[test]
fn a_shrinking_tip_build_gives_no_cores_back_to_landings() {
    let mut ledger = Ledger::new(with_reserve());
    ledger.enqueue(
        GrantRequest {
            shrinks: true,
            ..tip_build(12)
        },
        T0,
    );
    assert_eq!(ledger.step(T0).granted[0].cores, 12);
    // Land fills the rest of the shared cores and asks for more, but a tip build is scheduled
    // ahead of `land`, so it keeps all it holds. It is still reported in `tip`.
    ledger.enqueue(ask(Class::Land, 16), T0);
    ledger.enqueue(ask(Class::Land, 16), T0);
    for second in 1..40 {
        ledger.step(at(f64::from(second)));
    }
    let tip = ledger.status().classes[2].cores;
    assert_eq!(tip, 12);
}

/// Eight shared cores are free and an agent build and a landing build wait for them; a tip
/// round's build asked for later still takes them, with the reserve, before either starts.
#[test]
fn a_tip_build_is_granted_cores_before_waiting_agent_and_landing_builds() {
    let mut ledger = Ledger::new(Limits {
        floor: Floor::default(),
        ..with_reserve()
    });
    ledger.enqueue(ask(Class::Agent, 16), T0);
    ledger.step(T0);
    let agent = ledger.enqueue(ask(Class::Agent, 8), at(1.0));
    let land = ledger.enqueue(ask(Class::Land, 8), at(2.0));
    let build = ledger.enqueue(tip_build(12), at(3.0));
    let step = ledger.step(at(3.0));
    let granted: Vec<(u64, u32)> = step.granted.iter().map(|g| (g.id, g.cores)).collect();
    assert_eq!(granted, [(build, 12)]);
    // The landing goes next, then the agent build.
    assert!(ledger.release(build));
    let step = ledger.step(at(4.0));
    let granted: Vec<(u64, u32)> = step.granted.iter().map(|g| (g.id, g.cores)).collect();
    assert_eq!(granted, [(land, 8)]);
    assert_eq!(
        ledger.status().classes[1].waiting,
        1,
        "agent request {agent} waits"
    );
}

/// Shrinking agent builds hold every shared core; a growing tip build starts on the reserve,
/// and the agent builds give their cores back to it one a step until it has what it asked for.
#[test]
fn shrinking_agent_builds_give_their_cores_back_to_a_tip_build() {
    let mut ledger = Ledger::new(with_reserve());
    for _ in 0..2 {
        ledger.enqueue(
            GrantRequest {
                shrinks: true,
                ..ask(Class::Agent, 12)
            },
            T0,
        );
    }
    ledger.step(T0);
    assert_eq!(ledger.status().granted, 24);
    ledger.enqueue(
        GrantRequest {
            grows: true,
            cores_file: Some(std::path::PathBuf::from("cores")),
            ..tip_build(12)
        },
        at(1.0),
    );
    assert_eq!(ledger.step(at(1.0)).granted[0].cores, 4);
    for second in 2..20 {
        ledger.step(at(f64::from(second)));
    }
    let classes = ledger.status().classes;
    assert_eq!((classes[1].cores, classes[2].cores), (16, 12));
}

/// A restarted daemon takes over a tip build's grant: it holds the reserve until it is
/// released, and no new tip build takes those cores in the meantime.
#[test]
fn an_adopted_tip_build_holds_the_reserve_until_released() {
    let clock = Arc::new(FakeClock::new(T0));
    let host = Arc::new(FakeHost {
        free_memory_mb: Mutex::new(None),
        busy: Mutex::new(0.0),
        trees: Mutex::new(BTreeMap::new()),
    });
    let scheduler = Scheduler::new(with_reserve(), clock, host);
    let adopted = scheduler.adopt_all(vec![Adoption {
        class: Class::Tip,
        cores: 4,
        resources: Vec::new(),
        memory_mb: None,
        follow: None,
        reserve: Some(Reserve::TipBuild),
        priority_ticket: None,
    }]);
    assert_eq!(scheduler.status().tip_build_used, 4);
    assert_eq!(scheduler.status().granted, 0);
    scheduler.release(&adopted[0]);
    assert_eq!(scheduler.status().tip_build_used, 0);
}

/// The express lane borrows what is free when it starts, and idle cores of the tip reserve
/// count as free, so keeping them for the tip round does not slow an update of Ignite. A tip
/// build that arrives during the loan still starts on the whole reserve.
#[test]
fn the_express_lane_borrows_the_idle_reserve_and_a_tip_build_still_gets_it() {
    let clock = Arc::new(FakeClock::new(T0));
    let host = Arc::new(FakeHost {
        free_memory_mb: Mutex::new(None),
        busy: Mutex::new(0.0),
        trees: Mutex::new(BTreeMap::new()),
    });
    let scheduler = Scheduler::new(with_reserve(), clock, host);
    let agent = scheduler.acquire(&ask(Class::Agent, 16));
    let loan = scheduler.try_borrow(&ask(Class::Land, 12)).unwrap();
    assert_eq!(
        loan.cores, 12,
        "8 shared cores and the 4 idle reserved ones"
    );
    // A second loan finds the reserve already lent.
    scheduler.release(&agent);
    let second = scheduler.try_borrow(&ask(Class::Land, 12)).unwrap();
    assert_eq!(second.cores, 12);
    scheduler.release(&second);
    // Nothing shared is free, yet a loan gets the idle reserve alone.
    let filler = scheduler.acquire(&ask(Class::Agent, 16));
    scheduler.release_id(loan.id);
    let filler2 = scheduler.acquire(&ask(Class::Agent, 8));
    let reserve_only = scheduler.try_borrow(&ask(Class::Land, 12)).unwrap();
    assert_eq!(reserve_only.cores, 4);
    let build = scheduler.try_acquire(&tip_build(4)).unwrap();
    assert_eq!(build.cores, 4, "the loan does not hold the reserve back");
    scheduler.release(&reserve_only);
    // With no idle reserve and nothing shared free, nothing is lent.
    assert!(scheduler.try_borrow(&ask(Class::Land, 12)).is_none());
    for grant in [&filler, &filler2, &build] {
        scheduler.release(grant);
    }
}

/// With no tip reserve a `Reserve::TipBuild` request is still served first, from the shared
/// cores alone.
#[test]
fn without_a_reserve_a_tip_build_is_served_first_from_the_shared_cores() {
    let mut ledger = Ledger::new(Limits {
        floor: Floor::default(),
        ..limits()
    });
    ledger.enqueue(ask(Class::Land, 16), T0);
    ledger.enqueue(ask(Class::Land, 16), T0);
    let build = ledger.enqueue(tip_build(4), T0);
    let step = ledger.step(T0);
    let granted: Vec<(u64, u32)> = step.granted.iter().map(|g| (g.id, g.cores)).collect();
    assert_eq!(granted[0], (build, 4));
    assert_eq!(ledger.status().tip_build_used, 0);
    assert!(ledger.lend_reserve(None, Class::Land, 4).is_none());
}

#[test]
fn master_blocking_work_follows_tip_builds_and_goes_ahead_of_older_agent_work() {
    let mut ledger = Ledger::new(Limits {
        floor: Floor::default(),
        ..with_reserve()
    });
    let running = ledger.enqueue(ask(Class::Agent, 16), T0);
    let finishing = ledger.enqueue(ask(Class::Agent, 8), T0);
    assert_eq!(ledger.step(T0).granted.len(), 2);

    let feature = ledger.enqueue(ask(Class::Agent, 1), T0);
    let now = at(super::super::AGED_SECONDS + 1.0);
    let ticket = TicketId::new("FIX").unwrap();
    let regression = ledger.enqueue(
        GrantRequest {
            priority_ticket: Some(ticket.clone()),
            ..ask(Class::Agent, 8)
        },
        now,
    );
    let waiting = ledger.step(now);
    assert!(waiting.granted.is_empty());
    assert!(waiting.stop.is_empty());
    assert!(waiting.resized.is_empty());

    let tip = ledger.enqueue(tip_build(12), now);
    assert!(ledger.release(finishing));
    let step = ledger.step(now);
    assert_eq!(step.granted.len(), 1);
    assert_eq!((step.granted[0].id, step.granted[0].cores), (tip, 12));
    assert_eq!(ledger.status().tip_build_used, 4);
    assert!(ledger.status().priority_grants.is_empty());

    assert!(ledger.release(tip));
    let step = ledger.step(now);
    assert_eq!(step.granted.len(), 1);
    assert_eq!((step.granted[0].id, step.granted[0].cores), (regression, 8));
    assert!(ledger.holds(running));
    assert!(!ledger.holds(feature));
    assert_eq!(
        ledger.status().priority_grants,
        BTreeMap::from([(ticket, 1)])
    );

    assert!(ledger.release(regression));
    assert_eq!(ledger.step(now).granted[0].id, feature);
    assert!(ledger.status().priority_grants.is_empty());
}
