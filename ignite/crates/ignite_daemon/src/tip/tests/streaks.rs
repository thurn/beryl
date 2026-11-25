//! A check that keeps failing while landings keep replacing each candidate before its round
//! reaches a verdict is still reported as a regression; and a runnable failure stops other
//! families immediately so its round reaches its failed verdict and culprit search on the
//! first try instead of waiting for a streak.

use ignite_core::protocol::hub::{TipCheckFailures, TipVerified};
use ignite_store::hub;

use super::rounds::{in_family, running_hub, select};
use super::*;

/// When a peer platform reports a required failure on a tip and a newer landing replaces the
/// candidate while this machine's round is running another family, the round is superseded
/// before its verdict. The hub still reports the check once it fails at two candidates in a
/// row, with the range from the last verified tip to the first candidate that failed.
#[test]
fn a_check_failing_at_each_replaced_candidate_is_reported_within_two_rounds() {
    let world = World::new();
    let (server, database, ctx) = running_hub(&world, "failure-streak");
    let master = world.master();
    ctx.hub
        .tip_verified(&TipVerified {
            commit: master.clone(),
            machine: ctx.machine.clone(),
            state: TipState::Verified,
            regression_passes: Some(Vec::new()),
            failure: None,
            failed_descendant: None,
        })
        .unwrap();
    let candidates = [commit("bbbb000"), commit("cccc000"), commit("dddd000")];
    world.land(&candidates[0]);
    select(&ctx, &candidates[0]);
    let mut reported = Vec::new();
    for (round, pair) in candidates.windows(2).enumerate() {
        let (tip, newer) = (&pair[0], &pair[1]);
        // The check's inputs change with each landing, so its hash differs at every tip.
        let mut failing = in_family("a", "first");
        failing.key.hash = InputHash::new(format!("{:064x}", round + 1)).unwrap();
        failing.runnable = false;
        let unfinished = in_family("b", "second");
        world
            .work
            .set_checks(tip, vec![failing.clone(), unfinished]);
        let (git, work, loop_ctx, landed, failed_key, failed_tip) = (
            world.git.clone(),
            world.work.clone(),
            ctx.clone(),
            newer.clone(),
            failing.key.clone(),
            tip.clone(),
        );
        world.work.before_batches.lock().unwrap().insert(
            "second".to_owned(),
            Arc::new(move || {
                loop_ctx
                    .hub
                    .tip_check_failures(&TipCheckFailures {
                        commit: failed_tip.clone(),
                        checks: vec![failed_key.clone()],
                    })
                    .unwrap();
                git.set_tip("staging", landed.clone());
                let selected = TipVerifier::new(work.clone(), "test")
                    .next_target(&loop_ctx)
                    .unwrap();
                assert_eq!(selected.as_ref(), Some(&landed));
            }),
        );

        assert_eq!(
            world.verifier().verify(&ctx, tip).unwrap(),
            TipOutcome::Superseded { by: newer.clone() }
        );
        reported.push(ctx.hub.open_regressions().unwrap());
    }
    let conn = hub::open(&database).unwrap();
    let row: (String, String, String) = conn
        .query_row(
            "SELECT check_name, first_bad, last_good FROM regressions",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    server.stop();

    assert!(reported[0].is_empty(), "{reported:?}");
    let checks: Vec<_> = reported[1].iter().map(|r| r.check.clone()).collect();
    assert_eq!(checks, [name("first:a")]);
    assert_eq!(
        row,
        (
            "first:a".to_owned(),
            candidates[0].to_string(),
            master.to_string()
        )
    );
    assert_eq!(world.master(), master);
}

/// When a runnable required check fails during a tip round, the round stops starting further
/// families and proceeds straight to its failed verdict and culprit search instead of letting
/// a newer landing supersede it mid-round.
#[test]
fn a_runnable_failure_stops_other_families_and_reports_on_the_first_round() {
    let world = World::new();
    let (server, _database, ctx) = running_hub(&world, "fast-fail");
    let master = world.master();
    let tip = commit("bbbb000");
    let newer = commit("cccc000");
    world.land(&tip);
    select(&ctx, &tip);
    let failing = in_family("a", "first");
    let unrelated = in_family("b", "second");
    world
        .work
        .set_checks(&tip, vec![failing.clone(), unrelated.clone()]);
    world.work.set_outcome(&tip, &failing.check, Outcome::Fail);
    world.git.set_tip("staging", newer.clone());

    let outcome = world.verifier().verify(&ctx, &tip).unwrap();
    let ran = world.work.runs();
    let reported = ctx.hub.open_regressions().unwrap();
    server.stop();

    assert_eq!(
        outcome,
        TipOutcome::Failed {
            checks: vec![failing.check.clone()],
            promoted: None,
        }
    );
    assert!(
        !ran.iter()
            .any(|(_, at, checks, _)| at == &tip && checks.contains(&unrelated.check)),
        "unrelated family should not run after a runnable required failure: {ran:?}"
    );
    assert_eq!(
        reported.iter().map(|r| r.check.clone()).collect::<Vec<_>>(),
        [failing.check]
    );
    assert_eq!(world.master(), master);
}

/// When a peer reports a required failure and a newer landing replaces the candidate while this
/// machine's round runs its first family, the round still runs its other families at the same
/// tip before it gives way, so a second breakage among them is seen in this round. On
/// 2026-11-25 a Windows-only failure at every tip ended the other machines' rounds at each
/// landing for six hours, and breakages that landed meanwhile were found one at a time.
#[test]
fn a_peer_failure_lets_the_round_finish_its_own_checks_before_giving_way() {
    let world = World::new();
    let (server, _database, ctx) = running_hub(&world, "peer-failure-finishes");
    let master = world.master();
    let tip = commit("bbbb000");
    let newer = commit("cccc000");
    world.land(&tip);
    world.git.set_not_ancestor(&tip, &master);
    select(&ctx, &tip);
    let mut failing = in_family("a", "first");
    failing.runnable = false;
    let running = in_family("b", "second");
    let later = in_family("c", "third");
    world
        .work
        .set_checks(&tip, vec![failing.clone(), running, later.clone()]);
    let (git, work, peer_ctx, landed, failed_key, failed_tip) = (
        world.git.clone(),
        world.work.clone(),
        ctx.clone(),
        newer.clone(),
        failing.key.clone(),
        tip.clone(),
    );
    world.work.before_batches.lock().unwrap().insert(
        "second".to_owned(),
        Arc::new(move || {
            peer_ctx
                .hub
                .tip_check_failures(&TipCheckFailures {
                    commit: failed_tip.clone(),
                    checks: vec![failed_key.clone()],
                })
                .unwrap();
            git.set_tip("staging", landed.clone());
            let selected = TipVerifier::new(work.clone(), "test")
                .next_target(&peer_ctx)
                .unwrap();
            assert_eq!(selected.as_ref(), Some(&landed));
        }),
    );

    let outcome = world.verifier().verify(&ctx, &tip).unwrap();
    let ran = world.work.runs();
    server.stop();

    assert_eq!(outcome, TipOutcome::Superseded { by: newer });
    assert!(
        ran.iter()
            .any(|(_, at, checks, _)| at == &tip && checks.contains(&later.check)),
        "the round gave way before running its own checks: {ran:?}"
    );
    assert_eq!(world.master(), master);
}
