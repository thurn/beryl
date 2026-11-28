//! Tip verification's handling of rounds that cannot end cleanly: a check that stays
//! incomplete, a family that cannot be listed, a refused push, telemetry that fails.

use std::sync::atomic::{AtomicBool, Ordering};

use ignite_core::traits::Error;

use super::*;
use crate::testhub::SpyHub;

#[test]
fn a_round_that_ran_a_batch_is_finished_when_a_newer_tip_arrives() {
    let world = World::new();
    let ctx = world.ctx("m-1");
    let tip = commit("bbbb000");
    world.land(&tip);
    let mut first = check("a", 'a');
    first.family = "alpha".to_owned();
    let mut second = check("b", 'b');
    second.family = "beta".to_owned();
    world.work.set_checks(&tip, vec![first, second]);
    let newer = commit("cccc000");
    world.land(&newer);

    let outcome = world.verifier().verify(&ctx, &tip).unwrap();

    // Both batches ran, and `master` moved to the tip although `staging` is ahead of it.
    assert_eq!(outcome, TipOutcome::Verified);
    assert_eq!(world.work.runs().len(), 2);
    assert_eq!(world.master(), tip);
}

#[test]
fn a_round_keeps_its_candidate_when_the_hub_cannot_confirm_a_replacement() {
    for unreachable in [false, true] {
        let world = World::new();
        // The hub cannot say which candidate it selected while `down` is raised.
        let down = Arc::new(AtomicBool::new(false));
        let mut ctx = world.ctx("m-1");
        let refusing = down.clone();
        ctx.hub = Arc::new(SpyHub::new(world.hub.clone()).refusing(move |method| {
            (method == "tip_candidate" && refusing.load(Ordering::SeqCst))
                .then(|| Error::Unreachable("hub restarting".to_owned()))
        }));
        let tip = commit("bbbb000");
        world.land(&tip);
        let mut first = check("a", 'a');
        first.family = "alpha".to_owned();
        let mut second = check("b", 'b');
        second.family = "beta".to_owned();
        world.work.set_checks(&tip, vec![first, second]);
        // While this machine builds the first family, staging moves on and its candidate is
        // marked for retry, which does not select a replacement. The round goes on to run the
        // batch it claimed before the build, and the second family.
        let (other, candidate) = (ctx.clone(), tip.clone());
        let (git, flag) = (world.git.clone(), down.clone());
        world.work.before_batches.lock().unwrap().insert(
            "alpha".to_owned(),
            Arc::new(move || {
                git.set_tip("staging", commit("cccc000"));
                super::super::candidate::allow_newer(&other, &candidate).unwrap();
                flag.store(unreachable, Ordering::SeqCst);
            }),
        );
        world.work.before_batches.lock().unwrap().insert(
            "beta".to_owned(),
            Arc::new(move || down.store(false, Ordering::SeqCst)),
        );
        assert_eq!(
            world.verifier().round(&ctx, 0).unwrap(),
            Some((tip, TipOutcome::Verified))
        );
        assert_eq!(world.work.runs().len(), 2, "both families ran");
    }
}

/// Under load a family can spend ten minutes building and listing before it claims its first
/// batch. A round used to count that as having run nothing, so a landing in that window ended
/// it, and with landings arriving more often than that, `master` never moved.
#[test]
fn a_round_building_a_family_is_not_ended_by_a_newer_tip() {
    use std::sync::{Condvar, Mutex};
    use std::time::Duration;

    let world = World::new();
    let ctx = world.ctx("m-1");
    let tip = commit("bbbb000");
    world.land(&tip);
    let checks = [("alpha", 'a'), ("beta", 'b'), ("gamma", 'c')]
        .into_iter()
        .map(|(family, hash)| {
            let mut c = check(family, hash);
            c.family = family.to_owned();
            c
        })
        .collect();
    world.work.set_checks(&tip, checks);
    *world.work.parallel.lock().unwrap() = 2;
    // Another machine holds beta's batch, so beta's thread finishes having run nothing and
    // asks for the next family while alpha is still building.
    world
        .hub
        .tip_claim(&TipClaim {
            commit: tip.clone(),
            family: "beta".to_owned(),
            batch: 0,
            machine: MachineId::new("m-2").unwrap(),
        })
        .unwrap();
    let newer = commit("cccc000");
    // (newer tip landed, gamma started), with a condition variable to wait on either.
    let state = Arc::new((Mutex::new((false, false)), Condvar::new()));
    let wait_for = |state: &Arc<(Mutex<(bool, bool)>, Condvar)>,
                    done: fn(&(bool, bool)) -> bool| {
        let (lock, wake) = &**state;
        let guard = lock.lock().unwrap();
        // The timeout only stops a hang if the round never starts gamma.
        drop(
            wake.wait_timeout_while(guard, Duration::from_secs(2), |s| !done(s))
                .unwrap(),
        );
    };
    let mut hooks = world.work.before_batches.lock().unwrap();
    let (git, landed, s) = (world.git.clone(), newer.clone(), state.clone());
    // Alpha is building when the newer tip lands, and keeps building until gamma starts.
    hooks.insert(
        "alpha".to_owned(),
        Arc::new(move || {
            git.set_tip("staging", landed.clone());
            s.0.lock().unwrap().0 = true;
            s.1.notify_all();
            wait_for(&s, |s| s.1);
        }),
    );
    let s = state.clone();
    hooks.insert("beta".to_owned(), Arc::new(move || wait_for(&s, |s| s.0)));
    let s = state.clone();
    hooks.insert(
        "gamma".to_owned(),
        Arc::new(move || {
            s.0.lock().unwrap().1 = true;
            s.1.notify_all();
        }),
    );
    drop(hooks);

    let outcome = world.verifier().verify(&ctx, &tip).unwrap();

    assert_ne!(outcome, TipOutcome::Superseded { by: newer });
    let ran: Vec<CheckName> = world.work.runs().into_iter().flat_map(|r| r.2).collect();
    assert!(ran.contains(&name("unit:alpha")), "{ran:?}");
    assert!(ran.contains(&name("unit:gamma")), "{ran:?}");
}

/// A world whose tip `bbbb000` has `checks` and is the newest on `staging`.
fn world_at(checks: Vec<TipCheck>) -> (World, Commit) {
    let world = World::new();
    let tip = commit("bbbb000");
    world.land(&tip);
    world.work.set_checks(&tip, checks);
    (world, tip)
}

#[test]
fn a_check_that_stays_incomplete_is_reported_once_and_left_alone_for_an_hour() {
    let (world, tip) = world_at(vec![check("a", 'a'), check("b", 'b')]);
    let ctx = world.ctx("m-1");
    world
        .work
        .set_outcome(&tip, &name("unit:a"), Outcome::Incomplete);
    let mut verifier = world.verifier();

    let first = verifier.verify(&ctx, &tip).unwrap();

    // `unit:b` passed; `unit:a` holds `master` and has a ticket.
    assert_eq!(
        first,
        TipOutcome::Waiting {
            checks: vec![name("unit:a")],
            unrunnable: Vec::new(),
        }
    );
    let reports = world.hub.incomplete_reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].check, name("unit:a"));
    assert_eq!(reports[0].reason, IncompleteReason::Unreported);
    assert_eq!(reports[0].commit, tip);
    assert_eq!(reports[0].attempts, 3);
    assert_eq!(reports[0].machine.as_str(), "m-1");
    assert_eq!(reports[0].host, "jarvis");
    assert_eq!(world.work.runs().len(), 1);

    // Later rounds on the same tip keep waiting without running it or reporting it again.
    world.clock.advance(1_800.0);
    assert!(matches!(
        verifier.verify(&ctx, &tip).unwrap(),
        TipOutcome::Waiting { .. }
    ));
    assert_eq!(world.work.runs().len(), 1);
    assert_eq!(world.hub.incomplete_reports().len(), 1);

    // After the hour it runs once more; it is still one ticket, and the next wait is longer.
    world.clock.advance(1_801.0);
    verifier.verify(&ctx, &tip).unwrap();
    assert_eq!(world.work.runs().len(), 2);
    assert_eq!(world.hub.incomplete_reports().len(), 1);
    world.clock.advance(3_601.0);
    verifier.verify(&ctx, &tip).unwrap();
    assert_eq!(world.work.runs().len(), 2, "the second wait is two hours");
}

#[test]
fn an_incomplete_run_that_was_not_the_checks_fault_is_run_again_without_a_report() {
    let (world, tip) = world_at(vec![check("a", 'a')]);
    let ctx = world.ctx("m-1");
    world
        .work
        .set_outcome(&tip, &name("unit:a"), Outcome::Incomplete);
    world.work.unreasoned.lock().unwrap().insert(name("unit:a"));
    let mut verifier = world.verifier();

    verifier.verify(&ctx, &tip).unwrap();
    verifier.verify(&ctx, &tip).unwrap();

    assert_eq!(world.work.runs().len(), 2);
    assert!(world.hub.incomplete_reports().is_empty());
}

#[test]
fn a_cached_failure_from_this_machine_is_run_again_once() {
    let (world, tip) = world_at(vec![check("a", 'a')]);
    let ctx = world.ctx("m-1");
    // A failure this machine stored, which a rerun finds was a fault of that one run.
    world.store(&check("a", 'a').key, Outcome::Fail, "m-1");
    let mut verifier = world.verifier();

    assert_eq!(verifier.verify(&ctx, &tip).unwrap(), TipOutcome::Verified);

    let runs = world.work.runs();
    assert_eq!(runs.len(), 1);
    assert!(runs[0].3, "the rerun ignores the cache");
}

#[test]
fn a_failure_that_fails_again_stands_without_more_reruns() {
    let (world, tip) = world_at(vec![check("a", 'a')]);
    let ctx = world.ctx("m-1");
    world.store(&check("a", 'a').key, Outcome::Fail, "m-1");
    world.work.set_outcome(&tip, &name("unit:a"), Outcome::Fail);
    let mut verifier = world.verifier();

    assert!(matches!(
        verifier.verify(&ctx, &tip).unwrap(),
        TipOutcome::Failed { .. }
    ));
    assert!(matches!(
        verifier.verify(&ctx, &tip).unwrap(),
        TipOutcome::Failed { .. }
    ));

    assert_eq!(world.work.runs().len(), 1);
    assert_eq!(world.hub.regressions().len(), 1);
}

fn unlisted(family: &str, reportable: bool, platforms: Vec<Os>) -> Unlisted {
    Unlisted {
        name: name(&format!("{family}:listing")),
        family: family.to_owned(),
        message: format!("the family {family} could not be listed"),
        reportable,
        platforms,
    }
}

#[test]
fn a_family_that_cannot_be_listed_makes_the_tip_wait_and_is_reported_once() {
    let (world, tip) = world_at(vec![check("a", 'a')]);
    let ctx = world.ctx("m-1");
    world
        .work
        .unlisted
        .lock()
        .unwrap()
        .insert(tip.clone(), vec![unlisted("shots", true, Vec::new())]);
    let mut verifier = world.verifier();

    let outcome = verifier.verify(&ctx, &tip).unwrap();
    verifier.verify(&ctx, &tip).unwrap();

    assert_eq!(
        outcome,
        TipOutcome::Waiting {
            checks: vec![name("shots:listing")],
            unrunnable: Vec::new(),
        }
    );
    // The other family was still run.
    assert_eq!(world.work.runs().len(), 1);
    let reports = world.hub.incomplete_reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].check, name("shots:listing"));
    assert!(reports[0].output.contains("could not be listed"));
    assert_eq!(world.master(), commit("aaaa000"));
}

#[test]
fn a_family_this_machine_does_not_run_waits_for_a_machine_that_does() {
    let (world, tip) = world_at(vec![check("a", 'a')]);
    let ctx = world.ctx("m-1");
    world
        .work
        .unlisted
        .lock()
        .unwrap()
        .insert(tip.clone(), vec![unlisted("shots", false, vec![Os::Macos])]);
    let mut verifier = world.verifier();

    assert!(matches!(
        verifier.verify(&ctx, &tip).unwrap(),
        TipOutcome::Waiting { .. }
    ));
    // No ticket for a machine's own lack, and after an hour the tip's wait is reported for the
    // platform that can run it.
    assert!(world.hub.incomplete_reports().is_empty());
    world.clock.advance(MACHINE_WAIT_S + 1.0);
    verifier.verify(&ctx, &tip).unwrap();
    let waiting = world.hub.tips_waiting();
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].platform, Os::Macos);
    assert_eq!(waiting[0].checks, vec![name("shots:listing")]);
}

#[test]
fn a_refused_push_to_master_is_reported_once_and_the_tip_waits() {
    let (world, tip) = world_at(vec![check("a", 'a')]);
    let ctx = world.ctx("m-1");
    world.git.reject_pushes(Some("not a fast-forward"));
    world.git.set_not_ancestor(&tip, &commit("aaaa000"));
    let mut verifier = world.verifier();

    let outcome = verifier.verify(&ctx, &tip).unwrap();
    verifier.verify(&ctx, &tip).unwrap();

    assert_eq!(
        outcome,
        TipOutcome::Waiting {
            checks: vec![name("tip:move-master")],
            unrunnable: Vec::new(),
        }
    );
    let reports = world.hub.incomplete_reports();
    assert_eq!(reports.len(), 1);
    assert!(reports[0].output.contains("not a fast-forward"));
    assert_eq!(world.master(), commit("aaaa000"));
}

#[test]
fn a_tip_that_master_already_holds_is_verified_when_the_push_is_refused() {
    let (world, tip) = world_at(vec![check("a", 'a')]);
    let ctx = world.ctx("m-1");
    // Another machine verified a later tip first, so `master` is past this one and the push
    // is not a fast-forward.
    world.git.reject_pushes(Some("non-fast-forward"));

    let outcome = world.verifier().verify(&ctx, &tip).unwrap();

    assert_eq!(outcome, TipOutcome::Verified);
    assert!(world.hub.incomplete_reports().is_empty());
}

#[test]
fn a_hub_that_refuses_telemetry_does_not_fail_verification() {
    let (world, tip) = world_at(vec![check("a", 'a')]);
    let ctx = world.ctx("m-1");
    world.store(&check("a", 'a').key, Outcome::Pass, "m-2");
    world.hub.set_records_down(true);

    let outcome = world.verifier().verify(&ctx, &tip).unwrap();

    assert_eq!(outcome, TipOutcome::Verified);
}

#[test]
fn a_failed_culprit_search_still_reports_the_range_and_records_the_tip() {
    let (world, tip) = world_at(vec![check("a", 'a')]);
    let ctx = world.ctx("m-1");
    world.work.set_outcome(&tip, &name("unit:a"), Outcome::Fail);
    *world.work.landings_fail.lock().unwrap() = true;

    let outcome = world.verifier().verify(&ctx, &tip).unwrap();

    assert!(matches!(outcome, TipOutcome::Failed { .. }));
    let reports = world.hub.regressions();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].first_bad, tip);
    assert_eq!(reports[0].last_good, commit("aaaa000"));
    assert_eq!(
        world.hub.tip_states().last().unwrap().state,
        TipState::Failed
    );
}

/// Checks held back by one cause went to the hub one report per check, and the hub filed a
/// ticket for each. They now go in one report that lists and counts them.
#[test]
fn checks_of_one_family_that_stay_incomplete_are_reported_together() {
    let (world, tip) = world_at(vec![check("a", 'a'), check("b", 'b'), check("c", 'c')]);
    let ctx = world.ctx("m-1");
    for c in ["unit:a", "unit:b"] {
        world.work.set_outcome(&tip, &name(c), Outcome::Incomplete);
    }

    world.verifier().verify(&ctx, &tip).unwrap();

    let reports = world.hub.incomplete_reports();
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert_eq!(reports[0].reason, IncompleteReason::Unreported);
    assert_eq!(reports[0].count, 2);
    assert_eq!(reports[0].checks, vec![name("unit:a"), name("unit:b")]);
}

#[test]
fn checks_with_one_invalid_manifest_entry_are_reported_together() {
    let (world, tip) = world_at(vec![check("a", 'a')]);
    let ctx = world.ctx("m-1");
    let invalid = |check: &str, why: &str| Unlisted {
        name: name(check),
        family: "allocations".to_owned(),
        message: format!("the inputs of {check} cannot be resolved: invalid: {why}"),
        reportable: true,
        platforms: Vec::new(),
    };
    world.work.unlisted.lock().unwrap().insert(
        tip.clone(),
        vec![
            invalid(
                "allocations:bench",
                "crate-closure: the check belongs to no package",
            ),
            invalid(
                "allocations:search",
                "crate-closure: the check belongs to no package",
            ),
            invalid("allocations:other", "x/*.rs: matches no tracked file"),
        ],
    );
    let mut verifier = world.verifier();

    verifier.verify(&ctx, &tip).unwrap();
    verifier.verify(&ctx, &tip).unwrap();

    let reports = world.hub.incomplete_reports();
    assert_eq!(reports.len(), 2, "one per message, told once: {reports:?}");
    assert!(
        reports
            .iter()
            .all(|r| r.reason == IncompleteReason::InvalidEntry)
    );
    assert_eq!(
        reports[0].checks,
        vec![name("allocations:bench"), name("allocations:search")]
    );
    assert_eq!(
        reports[0].output,
        "invalid: crate-closure: the check belongs to no package"
    );
}

#[test]
fn a_family_no_machine_of_its_platform_runs_is_reported_at_once() {
    // The only Windows machine lists its families and leaves out `unit`, so no machine can
    // ever pass `unit:w` on Windows. Waiting an hour would only hold `master` longer.
    let world = World::new();
    let mut ctx = world.ctx("m-1");
    ctx.config.machine.get_mut("windows").unwrap().families = Some(vec!["other".to_owned()]);
    let tip = commit("bbbb000");
    world.land(&tip);
    let mut elsewhere = check("w", 'e');
    elsewhere.runnable = false;
    elsewhere.key.platform = Some(Os::Windows);
    world
        .work
        .set_checks(&tip, vec![check("a", 'a'), elsewhere]);
    let mut verifier = world.verifier();

    let outcome = verifier.verify(&ctx, &tip).unwrap();
    let TipOutcome::Waiting { unrunnable, .. } = &outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(unrunnable.len(), 1, "{unrunnable:?}");
    assert!(
        unrunnable[0].contains("unit") && unrunnable[0].contains("windows"),
        "{unrunnable:?}"
    );
    let reports = world.hub.tips_waiting();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].unrunnable, vec!["unit".to_owned()]);
    assert_eq!(reports[0].checks, vec![name("unit:w")]);

    // The next round at the same tip says so again but tells the hub only once.
    verifier.verify(&ctx, &tip).unwrap();
    assert_eq!(world.hub.tips_waiting().len(), 1);
    assert_eq!(world.master(), commit("aaaa000"));
}
