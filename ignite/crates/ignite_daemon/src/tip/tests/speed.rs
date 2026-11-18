//! What keeps a round short: families running at once, one report for a failed build, a
//! waiting tip left alone for a while, and tips already on `master` recorded.

use super::*;

fn in_family(text: &str, hash: char, family: &str) -> TipCheck {
    let mut check = check(text, hash);
    check.check = name(&format!("{family}:{text}"));
    check.key.check = check.check.clone();
    check.family = family.to_owned();
    check
}

/// A cold round ran 1,175 commands one after another on an 18-core machine, using about four
/// cores. Families now run at once, up to what the work allows.
#[test]
fn families_run_at_once() {
    let world = World::new();
    let ctx = world.ctx("m-1");
    let tip = commit("bbbb000");
    world.land(&tip);
    *world.work.parallel.lock().unwrap() = 2;
    world.work.set_checks(
        &tip,
        vec![in_family("a", 'a', "alpha"), in_family("b", 'b', "beta")],
    );

    let outcome = world.verifier().verify(&ctx, &tip).unwrap();

    assert_eq!(outcome, TipOutcome::Verified);
    assert_eq!(world.work.runs().len(), 2);
    assert_eq!(world.work.active.lock().unwrap().1, 2);
    assert_eq!(world.hub.tip_claims().len(), 2);
}

/// One refused Rust build filed a P1 ticket for each of its 1,456 checks. It now files one,
/// for the family's build, and the checks are not held off.
#[test]
fn a_failed_build_is_reported_once_for_its_family() {
    let world = World::new();
    let ctx = world.ctx("m-1");
    let tip = commit("bbbb000");
    world.land(&tip);
    let checks = vec![check("a", 'a'), check("b", 'b')];
    for c in &checks {
        world.work.set_outcome(&tip, &c.check, Outcome::Incomplete);
        world
            .work
            .unbuilt
            .lock()
            .unwrap()
            .insert(c.check.clone(), "rust-tests".to_owned());
    }
    world.work.set_checks(&tip, checks);
    let mut verifier = world.verifier();

    let outcome = verifier.verify(&ctx, &tip).unwrap();
    assert!(matches!(outcome, TipOutcome::Waiting { .. }), "{outcome:?}");
    let reports = world.hub.incomplete_reports();
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0].check, name("unit:build"));
    assert!(
        reports[0].output.contains("rust-tests"),
        "{}",
        reports[0].output
    );

    // The checks run again in the next round; the build is not told twice.
    verifier.verify(&ctx, &tip).unwrap();
    assert_eq!(world.work.runs().len(), 2);
    assert_eq!(world.hub.incomplete_reports().len(), 1);
}

/// A tip held by incomplete checks was listed again every time the feed changed, every minute
/// or two, for nothing. A waiting tip is now left for ten minutes unless a newer one arrives.
#[test]
fn a_waiting_tip_is_left_alone_until_the_retry_time() {
    let world = World::new();
    let ctx = world.ctx("m-1");
    let tip = commit("bbbb000");
    world.land(&tip);
    let stuck = check("a", 'a');
    world
        .work
        .set_outcome(&tip, &stuck.check, Outcome::Incomplete);
    world.work.set_checks(&tip, vec![stuck]);
    let mut verifier = world.verifier();

    let first = verifier.round(&ctx, 0).unwrap();
    assert!(
        matches!(first, Some((_, TipOutcome::Waiting { .. }))),
        "{first:?}"
    );
    world.clock.advance(WAITING_RETRY_S - 1.0);
    assert_eq!(verifier.round(&ctx, 0).unwrap(), None);

    // A newer tip is verified at once.
    let newer = commit("cccc000");
    world.land(&newer);
    world.work.set_checks(&newer, Vec::new());
    assert_eq!(
        verifier.round(&ctx, 0).unwrap(),
        Some((newer, TipOutcome::Verified))
    );
}

/// After `master` was pushed by hand to the tip, the hub kept that tip's row missing and older
/// tips `verifying`. The verifier now records a tip `master` already holds.
#[test]
fn a_tip_master_already_holds_is_recorded_as_verified() {
    let world = World::new();
    let ctx = world.ctx("m-1");
    let tip = commit("aaaa000");
    world.land(&tip);

    let outcome = world.verifier().verify(&ctx, &tip).unwrap();

    assert_eq!(outcome, TipOutcome::Verified);
    let states = world.hub.tip_states();
    assert_eq!(states.len(), 1);
    assert_eq!(states[0].commit, tip);
    assert_eq!(states[0].state, TipState::Verified);
}

/// The builds of a tip round's families ran as `agent` work, because the threads that run the
/// families did not carry the round's activity. Each family's thread now works for the round.
#[test]
fn families_run_in_the_rounds_class() {
    use crate::steps::{Kind, Steps, Work};
    use ignite_core::protocol::Class;

    let world = World::new();
    let ctx = world.ctx("m-1");
    let tip = commit("bbbb000");
    world.land(&tip);
    *world.work.parallel.lock().unwrap() = 2;
    world.work.set_checks(
        &tip,
        vec![in_family("a", 'a', "alpha"), in_family("b", 'b', "beta")],
    );
    let steps = Steps::new(world.clock.clone());
    let round = steps.begin(
        Kind::Loop("ignite-tip".to_owned()),
        Work {
            class: Some(Class::Tip),
            ..Work::default()
        },
    );
    let _entered = round.enter();

    world.verifier().verify(&ctx, &tip).unwrap();

    let classes = world.work.classes.lock().unwrap().clone();
    assert_eq!(classes, vec![Some(Class::Tip), Some(Class::Tip)]);
}
