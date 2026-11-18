//! What a round costs: a covered family not run twice, claims sized by recorded cost and cut
//! alike on every machine, and the record of where the round's time went.

use std::collections::BTreeSet;
use std::path::PathBuf;

use ignite_core::protocol::Usage;
use ignite_core::traits::RunResult;

use super::*;

/// A check named `<family>:<text>` reading `src/<text>`, with an input hash that differs per
/// family and name.
fn in_family(text: &str, family: &str) -> TipCheck {
    let mut check = check(text, 'a');
    check.check = name(&format!("{family}:{text}"));
    check.key.check = check.check.clone();
    let seed: u64 = format!("{family}:{text}").bytes().map(u64::from).sum();
    check.key.hash = InputHash::new(format!("{seed:064x}")).unwrap();
    check.family = family.to_owned();
    check
}

fn ran_checks(world: &World) -> BTreeSet<CheckName> {
    world.work.runs().into_iter().flat_map(|r| r.2).collect()
}

fn covered_world() -> (World, Commit) {
    let world = World::new();
    let tip = commit("bbbb000");
    world.land(&tip);
    world
        .work
        .covered_by
        .lock()
        .unwrap()
        .insert("slow".to_owned(), "fast".to_owned());
    (world, tip)
}

/// After a kernel change, tip verification ran both `b3d-interface` and `b3d-every-tick`,
/// although every fault the first finds the second finds too.
#[test]
fn a_covered_check_is_stood_for_by_its_covering_check() {
    let (world, tip) = covered_world();
    let covering = in_family("a", "fast");
    world.store(&covering.key, Outcome::Pass, "m-2");
    // `slow:b` has no counterpart, so it runs.
    let checks = vec![covering, in_family("a", "slow"), in_family("b", "slow")];
    world.work.set_checks(&tip, checks);

    let outcome = world.verifier().verify(&world.ctx("m-1"), &tip).unwrap();

    assert_eq!(outcome, TipOutcome::Verified);
    assert_eq!(world.master(), tip);
    assert_eq!(ran_checks(&world), BTreeSet::from([name("slow:b")]));
}

#[test]
fn a_failing_covering_check_is_reported_once() {
    let (world, tip) = covered_world();
    let covering = in_family("a", "fast");
    world.work.set_outcome(&tip, &covering.check, Outcome::Fail);
    world
        .work
        .set_checks(&tip, vec![covering, in_family("a", "slow")]);

    let outcome = world.verifier().verify(&world.ctx("m-1"), &tip).unwrap();

    assert_eq!(
        outcome,
        TipOutcome::Failed {
            checks: vec![name("fast:a")]
        }
    );
    let reported: Vec<CheckName> = world
        .hub
        .regressions()
        .into_iter()
        .map(|r| r.check)
        .collect();
    assert_eq!(reported, [name("fast:a")]);
    assert!(!ran_checks(&world).contains(&name("slow:a")));
}

/// A covering check that reads less than the covered one could pass on inputs the covered
/// check never saw, so it does not stand for it.
#[test]
fn a_covering_check_that_reads_less_does_not_stand_for_it() {
    let (world, tip) = covered_world();
    let covering = in_family("a", "fast");
    let mut covered = in_family("a", "slow");
    covered.inputs.push("src/more".to_owned());
    world.store(&covering.key, Outcome::Pass, "m-2");
    world.work.set_checks(&tip, vec![covering, covered]);

    world.verifier().verify(&world.ctx("m-1"), &tip).unwrap();

    assert_eq!(ran_checks(&world), BTreeSet::from([name("slow:a")]));
}

/// `count` checks of family `unit`, named so that they sort in order, each recorded at the hub
/// as costing `each` CPU seconds.
fn costed(world: &World, count: usize, each: f64) -> Vec<TipCheck> {
    (0..count)
        .map(|i| {
            let mut c = check(&format!("t{i:05}"), 'c');
            c.key.hash = InputHash::new(format!("{i:064x}")).unwrap();
            world.hub.set_check_cost(c.check.as_str(), each);
            c
        })
        .collect()
}

/// A kernel change made a round launch the program behind 9,153 interface checks 183 times,
/// once per 50 checks, although each check costs milliseconds.
#[test]
fn nine_thousand_cheap_checks_are_one_or_two_claims() {
    let world = World::new();
    let tip = commit("bbbb000");
    world.land(&tip);
    let checks = costed(&world, 9_000, 0.01);
    world.work.set_checks(&tip, checks);

    let outcome = world.verifier().verify(&world.ctx("m-1"), &tip).unwrap();

    assert_eq!(outcome, TipOutcome::Verified);
    let claims = world.hub.tip_claims().len();
    assert!((1..=2).contains(&claims), "{claims} claims");
}

#[test]
fn checks_of_thirty_seconds_are_claimed_two_at_a_time() {
    let world = World::new();
    let tip = commit("bbbb000");
    world.land(&tip);
    let checks = costed(&world, 6, 30.0);
    world.work.set_checks(&tip, checks);

    world.verifier().verify(&world.ctx("m-1"), &tip).unwrap();

    let sizes: Vec<usize> = world.work.runs().iter().map(|r| r.2.len()).collect();
    assert_eq!(sizes, [2, 2, 2]);
    assert_eq!(world.hub.tip_claims().len(), 3);
}

/// Batches are claimed by number, so two machines must cut the same ones even when the
/// recorded costs change between their rounds and one has already run some of the checks.
#[test]
fn two_daemons_given_the_same_tip_cut_identical_batches() {
    let world = World::new();
    let tip = commit("bbbb000");
    world.land(&tip);
    let checks = costed(&world, 6, 30.0);
    let names: Vec<CheckName> = checks.iter().map(|c| c.check.clone()).collect();
    world.work.set_checks(&tip, checks);
    // m-2 holds the first batch.
    let first = TipClaim {
        commit: tip.clone(),
        family: "unit".to_owned(),
        batch: 0,
        machine: MachineId::new("m-2").unwrap(),
    };
    assert!(world.hub.tip_claim(&first).unwrap().granted);
    let one = world.verifier().verify(&world.ctx("m-1"), &tip).unwrap();
    assert!(matches!(one, TipOutcome::Waiting { .. }), "{one:?}");
    // Costs recorded since would put every check in one batch.
    for name in &names {
        world.hub.set_check_cost(name.as_str(), 1.0);
    }

    let two = world.verifier().verify(&world.ctx("m-2"), &tip).unwrap();

    assert_eq!(two, TipOutcome::Verified);
    let batches: Vec<(String, Vec<CheckName>)> = world
        .work
        .runs()
        .into_iter()
        .map(|r| (r.0.to_string(), r.2))
        .collect();
    assert_eq!(
        batches,
        [
            ("m-1".to_owned(), names[2..4].to_vec()),
            ("m-1".to_owned(), names[4..6].to_vec()),
            ("m-2".to_owned(), names[0..2].to_vec()),
        ]
    );
}

fn took(wall_s: f64, queued_s: f64) -> RunResult {
    RunResult {
        exit: Some(0),
        output: PathBuf::new(),
        usage: Usage {
            queued_s,
            wall_s,
            ..Usage::default()
        },
        hung: false,
    }
}

/// A 137-minute round spent 21 minutes listing families, and nothing said so.
#[test]
fn a_round_records_where_its_time_went_and_a_slow_one_files_one_ticket() {
    let world = World::new();
    *world.work.clock.lock().unwrap() = Some(world.clock.clone());
    world.launcher.script(&["list"], took(180.0, 0.0));
    world.launcher.script(&["run"], took(400.0, 20.0));
    let ctx = world.ctx("m-1");
    let tip = commit("bbbb000");
    world.land(&tip);
    let cached = check("cached", 'a');
    world.store(&cached.key, Outcome::Pass, "m-2");
    world.work.set_checks(&tip, vec![cached, check("new", 'b')]);
    let mut verifier = world.verifier();

    assert_eq!(verifier.verify(&ctx, &tip).unwrap(), TipOutcome::Verified);

    let rounds = world.hub.tip_rounds();
    assert_eq!(rounds.len(), 1);
    let round = &rounds[0];
    assert_eq!((round.commit.clone(), round.ran, round.cached), (tip, 1, 1));
    assert!((round.wall_s - 600.0).abs() < 1e-6, "{round:?}");
    assert!((round.listing_s - 180.0).abs() < 1e-6, "{round:?}");
    assert!((round.checks_s - 400.0).abs() < 1e-6, "{round:?}");
    assert!((round.waiting_s - 20.0).abs() < 1e-6, "{round:?}");
    assert!(round.building_s.abs() < 1e-6, "{round:?}");
    assert_eq!(world.hub.round_tickets_filed(), 1);

    // A second slow round while that ticket is open files nothing.
    let newer = commit("cccc000");
    world.land(&newer);
    world.work.set_checks(&newer, vec![check("newer", 'c')]);
    assert_eq!(verifier.verify(&ctx, &newer).unwrap(), TipOutcome::Verified);
    assert_eq!(world.hub.tip_rounds().len(), 2);
    assert_eq!(world.hub.round_tickets_filed(), 1);
}
