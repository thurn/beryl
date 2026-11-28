//! A settled failed tip is verified again once the checks that failed at it are switched off
//! at the hub, as `ignite flaky off` or upkeep does. A switched-off check never records a
//! pass, so during a landing freeze `master` once stood still behind a tip whose every failure
//! was switched off.

use super::*;
use crate::tip::cache::CHECK_STATES_TTL_S;

/// A tip at which `failing` fail and the other `checks` pass, after its failed round.
fn failed_tip(world: &World, verifier: &mut TipVerifier, checks: &[&str], failing: &[&str]) {
    let tip = commit("bbbb000");
    world.land(&tip);
    let listed = checks
        .iter()
        .zip('a'..)
        .map(|(text, hash)| check(text, hash))
        .collect();
    world.work.set_checks(&tip, listed);
    for text in failing {
        world
            .work
            .set_outcome(&tip, &name(&format!("unit:{text}")), Outcome::Fail);
    }
    let outcome = verifier.round_at(&world.ctx("m-1"), &tip, Seq(0)).unwrap();
    assert!(matches!(outcome, TipOutcome::Failed { .. }), "{outcome:?}");
    assert!(!verifier.due(&world.ctx("m-1"), &tip));
}

#[test]
fn a_failed_tip_whose_failure_is_switched_off_is_verified_again() {
    let world = World::new();
    let ctx = world.ctx("m-1");
    let mut verifier = world.verifier();
    failed_tip(&world, &mut verifier, &["a", "b"], &["a"]);
    let tip = commit("bbbb000");

    world.work.switch_off(&tip, &name("unit:a"));
    world.clock.advance(CHECK_STATES_TTL_S);

    assert!(verifier.due(&ctx, &tip));
    assert_eq!(
        verifier.round_at(&ctx, &tip, Seq(0)).unwrap(),
        TipOutcome::Verified
    );
    assert_eq!(world.master(), tip);
}

#[test]
fn a_failed_tip_with_a_failure_still_switched_on_stays_settled() {
    let world = World::new();
    let ctx = world.ctx("m-1");
    let mut verifier = world.verifier();
    failed_tip(&world, &mut verifier, &["a", "b"], &["a", "b"]);
    let tip = commit("bbbb000");
    let still_failing = world
        .work
        .listed(&tip)
        .into_iter()
        .find(|check| check.check == name("unit:b"))
        .unwrap();

    world.work.switch_off(&tip, &name("unit:a"));
    world.clock.advance(CHECK_STATES_TTL_S);
    assert!(!verifier.due(&ctx, &tip));

    // A pass for the check still switched on recovers the tip.
    world.store(&still_failing.key, Outcome::Pass, "m-2");
    assert!(verifier.due(&ctx, &tip));
}

#[test]
fn an_unreachable_hub_leaves_the_failed_tip_settled() {
    let world = World::new();
    let ctx = world.ctx("m-1");
    let mut verifier = world.verifier();
    failed_tip(&world, &mut verifier, &["a"], &["a"]);
    let tip = commit("bbbb000");
    world.work.switch_off(&tip, &name("unit:a"));
    world.clock.advance(CHECK_STATES_TTL_S);

    // The check states cannot be read, though the result cache answers.
    *world.work.states_down.lock().unwrap() = true;
    assert!(!verifier.due(&ctx, &tip));
    // Nothing at the hub answers.
    world.hub.set_unreachable(true);
    assert!(!verifier.due(&ctx, &tip));

    world.hub.set_unreachable(false);
    *world.work.states_down.lock().unwrap() = false;
    assert!(verifier.due(&ctx, &tip));
}
