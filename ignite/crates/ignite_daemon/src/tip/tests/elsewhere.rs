//! A round builds a family only when this machine still has one of its checks to run.
//!
//! On a slow machine, a round used to lease a tree for each family and build the family's
//! products to list it, and only then claim its batches, by which time faster machines had
//! claimed every one. One such round spent 94 core-minutes building and ran no check. Asking
//! the hub first was not enough when the build itself took most of an hour, so the round also
//! claims the family's first batch before building it.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

use ignite_core::gate::{GateCheck, History};
use ignite_core::score::{AreaRecord, Cost};
use ignite_core::traits::Error;

use super::*;
use crate::checks::Scoring;
use crate::testhub::SpyHub;

/// A record that puts the check within any budget, so the round records its budget.
fn scored(text: &str) -> GateCheck {
    GateCheck {
        family_startup: Cost::Billions(0.0),
        check: name(text),
        always: false,
        edited: false,
        cached: false,
        history: Some(History {
            areas: BTreeMap::from([(
                "src".to_owned(),
                AreaRecord {
                    runs: 20,
                    failures: 1,
                },
            )]),
            median_cost: Cost::Billions(1.0),
        }),
        family_rate: 0.01,
        family_median_cost: Cost::Billions(1.0),
        needs: Vec::new(),
        group: None,
        regression: None,
    }
}

/// A tip with two checks of family `unit`, one batch, whose claim `m-2` holds. Returns the tip
/// and a flag the family's batches set when they start, which is where the real work leases a
/// tree and builds and lists the family.
fn held_by_another_machine(world: &World) -> (Commit, Arc<AtomicBool>) {
    let tip = commit("bbbb000");
    world.land(&tip);
    world
        .work
        .set_checks(&tip, vec![check("a", 'a'), check("b", 'b')]);
    world.work.add_landing("tk.1", &["bbbb000"], &["src/a"]);
    *world.work.scoring.lock().unwrap() = Some(Scoring {
        areas: vec!["src".to_owned()],
        checks: vec![scored("unit:a"), scored("unit:b")],
    });
    let claim = TipClaim {
        commit: tip.clone(),
        family: "unit".to_owned(),
        batch: 0,
        machine: MachineId::new("m-2").unwrap(),
    };
    assert!(world.hub.tip_claim(&claim).unwrap().granted);
    let built = Arc::new(AtomicBool::new(false));
    let flag = built.clone();
    world.work.before_batches.lock().unwrap().insert(
        "unit".to_owned(),
        Arc::new(move || flag.store(true, Ordering::SeqCst)),
    );
    (tip, built)
}

#[test]
fn a_family_whose_batches_another_machine_holds_is_not_built() {
    let world = World::new();
    let (tip, built) = held_by_another_machine(&world);

    world.verifier().verify(&world.ctx("m-1"), &tip).unwrap();

    assert!(!built.load(Ordering::SeqCst), "the family was built");
    assert!(world.work.runs().is_empty());
    let asks = world.hub.tip_claim_asks();
    assert!(
        asks.iter().all(|ask| ask.machine.as_str() == "m-2"),
        "{asks:?}"
    );
    // The checks are left to the machine that holds them, not counted as unrun.
    let budget = world.hub.tip_rounds().pop().unwrap().budget.unwrap();
    assert_eq!(budget.elsewhere, BTreeMap::from([("unit".to_owned(), 2)]));
    assert!(budget.unrun.is_empty(), "{:?}", budget.unrun);
}

/// A hub that cannot answer the lookup leaves the family to its claims: a batch another machine
/// claimed after the lookup is not built for, and the checks are left to that machine.
#[test]
fn a_family_claimed_after_the_lookup_is_not_built() {
    let world = World::new();
    let (tip, built) = held_by_another_machine(&world);
    let mut ctx = world.ctx("m-1");
    ctx.hub = Arc::new(SpyHub::new(world.hub.clone()).refusing(|method| {
        (method == "tip_unclaimed").then(|| Error::Unreachable("hub restarting".to_owned()))
    }));

    world.verifier().verify(&ctx, &tip).unwrap();

    assert!(!built.load(Ordering::SeqCst), "the family was built");
    assert!(world.work.runs().is_empty());
    let budget = world.hub.tip_rounds().pop().unwrap().budget.unwrap();
    assert_eq!(budget.elsewhere, BTreeMap::from([("unit".to_owned(), 2)]));
}

#[test]
fn a_hub_that_cannot_say_what_is_unclaimed_leaves_the_family_to_be_built() {
    let world = World::new();
    let tip = commit("bbbb000");
    world.land(&tip);
    world
        .work
        .set_checks(&tip, vec![check("a", 'a'), check("b", 'b')]);
    let built = Arc::new(AtomicBool::new(false));
    let flag = built.clone();
    world.work.before_batches.lock().unwrap().insert(
        "unit".to_owned(),
        Arc::new(move || flag.store(true, Ordering::SeqCst)),
    );
    let mut ctx = world.ctx("m-1");
    ctx.hub = Arc::new(SpyHub::new(world.hub.clone()).refusing(|method| {
        (method == "tip_unclaimed").then(|| Error::Unreachable("hub restarting".to_owned()))
    }));

    world.verifier().verify(&ctx, &tip).unwrap();

    assert!(built.load(Ordering::SeqCst), "the family was not built");
    assert!(!world.work.runs().is_empty());
}

/// The family's first batch is claimed before its products build, so a machine that builds
/// slowly still holds a batch to run when the build finishes.
#[test]
fn a_family_holds_its_first_batch_while_it_builds() {
    let world = World::new();
    let tip = commit("bbbb000");
    world.land(&tip);
    world
        .work
        .set_checks(&tip, vec![check("a", 'a'), check("b", 'b')]);
    let hub = world.hub.clone();
    let held = Arc::new(AtomicBool::new(false));
    let flag = held.clone();
    world.work.before_batches.lock().unwrap().insert(
        "unit".to_owned(),
        Arc::new(move || {
            let asks = hub.tip_claim_asks();
            let claimed = asks
                .iter()
                .any(|ask| ask.machine.as_str() == "m-1" && ask.family == "unit");
            flag.store(claimed, Ordering::SeqCst);
        }),
    );

    world.verifier().verify(&world.ctx("m-1"), &tip).unwrap();

    assert!(
        held.load(Ordering::SeqCst),
        "no batch was claimed before the build"
    );
    assert!(!world.work.runs().is_empty());
    // The batch claimed before the build is not asked for again when it runs.
    let asks = world.hub.tip_claim_asks();
    assert_eq!(asks.len(), 1, "{asks:?}");
}

#[test]
fn a_family_with_a_batch_left_unclaimed_is_built() {
    let world = World::new();
    let (tip, built) = held_by_another_machine(&world);
    // A third check makes the family two batches when cut by count, and m-2 holds only the
    // first.
    let mut checks = world.work.listed(&tip);
    for n in 0..ignite_core::tipround::UNMEASURED_BATCH {
        let text = format!("z{n:03}");
        checks.push(check(&text, 'c'));
    }
    world.work.set_checks(&tip, checks);
    *world.work.scoring.lock().unwrap() = None;

    world.verifier().verify(&world.ctx("m-1"), &tip).unwrap();

    assert!(built.load(Ordering::SeqCst), "the family was not built");
    assert!(!world.work.runs().is_empty());
}
