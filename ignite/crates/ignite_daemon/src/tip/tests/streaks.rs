//! A check that keeps failing while landings keep replacing each candidate before its round
//! reaches a verdict is still reported as a regression.

use ignite_core::protocol::hub::TipVerified;
use ignite_store::hub;

use super::rounds::{in_family, running_hub, select};
use super::*;

/// Each round sees `first:a` fail, then a landing moves `staging` while the round's next
/// family starts, and the tip loop selects the newer tip, which ends the round before its
/// verdict and culprit search. The failure at the second candidate is reported, with the range
/// from the last verified tip to the first candidate that failed.
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
        let unfinished = in_family("b", "second");
        world
            .work
            .set_checks(tip, vec![failing.clone(), unfinished]);
        world.work.set_outcome(tip, &failing.check, Outcome::Fail);
        let (git, work, loop_ctx, landed) = (
            world.git.clone(),
            world.work.clone(),
            ctx.clone(),
            newer.clone(),
        );
        world.work.before_batches.lock().unwrap().insert(
            "second".to_owned(),
            Arc::new(move || {
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
