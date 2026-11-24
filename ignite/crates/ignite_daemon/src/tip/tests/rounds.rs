//! What a round costs: a covered family not run twice, claims sized by recorded cost and cut
//! alike on every machine, and the record of where the round's time went.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Barrier, mpsc};
use std::thread;

use ignite_core::protocol::Usage;
use ignite_core::protocol::hub::{MachineRegister, TipCheckFailures, TipFailure, TipVerified};
use ignite_core::tipround::{TipCandidate, TipCandidateUpdate};
use ignite_core::traits::RunResult;
use ignite_hub::server::{HubServer, RunningHub};
use ignite_store::hub::{self, tips};

use super::*;

/// A check named `<family>:<text>` reading `src/<text>`, with an input hash that differs per
/// family and name.
pub(super) fn in_family(text: &str, family: &str) -> TipCheck {
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
fn a_deferred_family_cannot_cover_a_required_check() {
    let (world, tip) = covered_world();
    let mut ctx = world.ctx("m-1");
    ctx.config.machine.get_mut("local").unwrap().os = Some(Os::Macos);
    let mut covering = in_family("a", "fast");
    let mut covered = in_family("a", "slow");
    covering.key.platform = Some(Os::Macos);
    covered.key.platform = Some(Os::Macos);
    world
        .work
        .defer_platforms
        .lock()
        .unwrap()
        .insert("fast".to_owned(), vec![Os::Macos]);
    world
        .work
        .set_checks(&tip, vec![covering.clone(), covered.clone()]);

    let mut verifier = world.verifier().with_host("local");
    assert_eq!(
        verifier.round_at(&ctx, &tip, Seq(0)).unwrap(),
        TipOutcome::VerifiedWithDeferrals {
            checks: [(Os::Macos, vec![covering.check.clone()])].into(),
        }
    );
    assert_eq!(ran_checks(&world), BTreeSet::from([covered.check.clone()]));
    assert_eq!(world.master(), tip);
    assert!(verifier.due(&ctx, &tip));
    assert_eq!(
        verifier.round_at(&ctx, &tip, Seq(0)).unwrap(),
        TipOutcome::Verified
    );
    assert_eq!(
        ran_checks(&world),
        BTreeSet::from([covered.check, covering.check])
    );
}

#[test]
fn a_covered_check_with_an_open_regression_reports_its_own_pass() {
    let (world, tip) = covered_world();
    let covering = in_family("a", "fast");
    let regressed = in_family("a", "slow");
    world
        .hub
        .set_open_regressions(vec![ignite_core::protocol::hub::OpenRegression {
            check: regressed.check.clone(),
            ticket: ticket("tk.1"),
        }]);
    world.store(&covering.key, Outcome::Pass, "m-2");
    world
        .work
        .set_checks(&tip, vec![covering, regressed.clone()]);

    assert_eq!(
        world.verifier().verify(&world.ctx("m-1"), &tip).unwrap(),
        TipOutcome::Verified
    );
    assert_eq!(ran_checks(&world), BTreeSet::from([regressed.check]));
    let states = world.hub.tip_states();
    assert_eq!(
        states.last().unwrap().regression_passes.as_deref(),
        Some(&[regressed.key][..])
    );
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
            promoted: None,
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

/// A check switched off as flaky after one machine cut its batches must not shift the
/// batches of a machine that lists the family afterwards: its batch 0 would then take in a
/// check of the first machine's batch 1, which would run on both machines at once.
#[test]
fn a_check_switched_off_mid_round_leaves_the_batches_alone() {
    let world = World::new();
    let tip = commit("bbbb000");
    world.land(&tip);
    let checks = costed(&world, 6, 30.0);
    let names: Vec<CheckName> = checks.iter().map(|c| c.check.clone()).collect();
    world.work.set_checks(&tip, checks);
    // m-1 cut the family into three batches of two and holds batches 1 and 2, which it has
    // not run yet.
    for batch in [1, 2] {
        let claim = TipClaim {
            commit: tip.clone(),
            family: "unit".to_owned(),
            batch,
            machine: MachineId::new("m-1").unwrap(),
        };
        assert!(world.hub.tip_claim(&claim).unwrap().granted);
    }
    // The hub switches the second check off before m-2 lists the family.
    world.work.switch_off(&tip, &names[1]);

    let outcome = world.verifier().verify(&world.ctx("m-2"), &tip).unwrap();

    assert!(matches!(outcome, TipOutcome::Waiting { .. }), "{outcome:?}");
    let runs: Vec<Vec<CheckName>> = world.work.runs().into_iter().map(|r| r.2).collect();
    assert_eq!(runs, [names[0..1].to_vec()]);
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
    world.launcher.advance_clock(world.clock.clone());
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
    // Nothing else ran beside the listing, so the round waited on all of it.
    let alone = round.listing_alone_s.unwrap();
    assert!((alone - 180.0).abs() < 1e-6, "{round:?}");
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

pub(super) fn running_hub(world: &World, name: &str) -> (RunningHub, PathBuf, Context) {
    let ctx = world.ctx("m-1");
    let database = ignite_core::test_files::temp_dir().join(format!("tip-{name}.sqlite"));
    let hub = HubServer {
        config: ctx.config.clone(),
        listen: "127.0.0.1:0".to_owned(),
        database: database.clone(),
        wordmark: None,
    }
    .start()
    .unwrap();
    let ctx = connect_machine(ctx, &hub, "local", Os::Macos);
    (hub, database, ctx)
}

pub(super) fn connect_machine(mut ctx: Context, hub: &RunningHub, name: &str, os: Os) -> Context {
    ctx.config.hub.url = format!("http://{}", hub.address());
    let register = crate::hubclient::HubClient::new(&ctx.config.hub, "", "tip-test");
    let machine = register
        .register(&MachineRegister {
            machine: ctx.machine.clone(),
            name: name.to_owned(),
            os,
            cores: 1,
            memory_mb: 1024,
            families: Vec::new(),
            version: "test".to_owned(),
        })
        .unwrap();
    ctx.hub = Arc::new(crate::hubclient::HubClient::new(
        &ctx.config.hub,
        &machine.token,
        "tip-test",
    ));
    ctx
}

pub(super) fn select(ctx: &Context, tip: &Commit) {
    let expected = ctx.hub.tip_candidate().unwrap();
    assert!(
        ctx.hub
            .update_tip_candidate(&TipCandidateUpdate {
                expected,
                replacement: TipCandidate {
                    commit: tip.clone(),
                    retry_on_new_tip: false,
                },
            })
            .unwrap()
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn both_platforms_start_the_successor_while_the_failed_round_is_running() {
    let world = World::new();
    let (server, database, ctx) = running_hub(&world, "while-running");
    let windows = connect_machine(world.ctx("m-2"), &server, "windows", Os::Windows);
    let tip = commit("bbbb000");
    let newer = commit("cccc000");
    world.land(&tip);
    select(&ctx, &tip);
    let mut failed = in_family("a", "first");
    failed.key.platform = Some(Os::Macos);
    let unfinished = in_family("b", "second");
    world
        .work
        .set_checks(&tip, vec![failed.clone(), unfinished.clone()]);
    world.work.set_outcome(&tip, &failed.check, Outcome::Fail);
    world
        .work
        .set_outcome(&tip, &unfinished.check, Outcome::Incomplete);
    let mut mac_check = failed.clone();
    mac_check.key.hash = InputHash::new("c".repeat(64)).unwrap();
    let mut windows_check = in_family("c", "native");
    windows_check.key.platform = Some(Os::Windows);
    windows_check.runnable = false;
    world
        .work
        .set_checks(&newer, vec![mac_check.clone(), windows_check.clone()]);
    world.git.set_changed(&tip, &newer, &["src/a"]);
    world.land(&newer);
    let previous = world.master();
    for machine in [&ctx, &windows] {
        assert_eq!(
            world.verifier().next_target(machine).unwrap(),
            Some(tip.clone())
        );
    }
    let resume = Arc::new(Barrier::new(2));
    let (ready, held) = mpsc::channel();
    let release = resume.clone();
    let finished = ready.clone();
    super::culprit::on_second_run(
        &world,
        &tip,
        Arc::new(move || {
            ready.send(true).unwrap();
            release.wait();
        }),
    );
    let conn = hub::open(&database).unwrap();

    let (observed, mac, master_after_mac, windows, outcome) = thread::scope(|scope| {
        let round = scope.spawn(|| {
            let outcome = world.verifier().verify(&ctx, &tip);
            let _ = finished.send(false);
            outcome
        });
        assert!(
            held.recv().unwrap(),
            "round finished before the second batch ran"
        );
        let hub = &ctx.config.hub;
        let observed = ureq::get(format!("{}/{}/api/tips/failures", hub.url, hub.project))
            .call()
            .and_then(|mut reply| reply.body_mut().read_to_vec());
        let mac = world.verifier().round(&ctx, 0);
        let master_after_mac = world.master();
        mac_check.runnable = false;
        windows_check.runnable = true;
        world
            .work
            .set_checks(&newer, vec![mac_check, windows_check.clone()]);
        let windows = world.verifier().round(&windows, 0);
        resume.wait();
        (
            observed,
            mac,
            master_after_mac,
            windows,
            round.join().unwrap(),
        )
    });
    server.stop();

    let observed: Option<TipCheckFailures> = serde_json::from_slice(&observed.unwrap()).unwrap();
    assert_eq!(
        observed,
        Some(TipCheckFailures {
            commit: tip.clone(),
            checks: vec![failed.key.clone()],
        })
    );
    assert_eq!(
        mac.unwrap(),
        Some((
            newer.clone(),
            TipOutcome::Waiting {
                checks: vec![windows_check.check],
                unrunnable: Vec::new(),
            },
        ))
    );
    assert_eq!(master_after_mac, previous);
    assert_eq!(
        windows.unwrap(),
        Some((newer.clone(), TipOutcome::Verified))
    );
    assert_eq!(world.master(), newer);
    assert_eq!(outcome.unwrap(), TipOutcome::Superseded { by: newer },);
    assert_eq!(tips::candidate_failures(&conn).unwrap(), None);
    assert_eq!(
        tips::latest_failure(&conn, Timestamp::EPOCH).unwrap(),
        Some((
            TipCheckFailures {
                commit: tip,
                checks: vec![failed.key],
            },
            false,
        ))
    );
}

fn store_pass(ctx: &Context, key: &ResultKey) {
    ctx.hub
        .store(&[NewResult {
            key: key.clone(),
            outcome: Outcome::Pass,
            machine: ctx.machine.clone(),
            record: RecordId(2),
            output: None,
            cost: ignite_core::protocol::hub::ResultCost::default(),
            areas: Vec::new(),
        }])
        .unwrap();
}

#[test]
fn a_settled_failure_retries_the_same_tip_once_all_exact_keys_pass() {
    let world = World::new();
    let (server, database, ctx) = running_hub(&world, "recovered-tip");
    let peer = connect_machine(world.ctx("m-2"), &server, "peer", Os::Macos);
    let tip = commit("bbbb000");
    world.land(&tip);
    let mut first = check("first", 'a');
    first.key.platform = Some(Os::Macos);
    let second = check("second", 'b');
    let checks = vec![first.clone(), second.clone()];
    for check in &checks {
        world.work.set_outcome(&tip, &check.check, Outcome::Fail);
    }
    world.work.set_checks(&tip, checks);
    let local = database.with_file_name("daemon.db");
    let remembering = || {
        world
            .verifier()
            .with_memory(ignite_store::local::open(&local).unwrap())
    };

    assert_eq!(
        remembering().round(&ctx, 0).unwrap(),
        Some((
            tip.clone(),
            TipOutcome::Failed {
                promoted: None,
                checks: vec![first.check.clone(), second.check.clone()],
            }
        ))
    );
    let failed_runs = world.work.runs();
    let regressions = ctx.hub.open_regressions().unwrap();
    assert_eq!(regressions.len(), 2);

    let mut wrong_platform = first.key.clone();
    wrong_platform.platform = Some(Os::Windows);
    let mut wrong_hash = first.key.clone();
    wrong_hash.hash = InputHash::new("c".repeat(64)).unwrap();
    for key in [&second.key, &wrong_platform, &wrong_hash] {
        store_pass(&peer, key);
    }
    assert_eq!(remembering().round(&ctx, 0).unwrap(), None);
    assert_eq!(remembering().round(&ctx, 0).unwrap(), None);
    assert_eq!(world.master(), commit("aaaa000"));
    assert_eq!(world.work.runs(), failed_runs);
    assert_eq!(ctx.hub.open_regressions().unwrap(), regressions);

    store_pass(&peer, &first.key);
    assert_eq!(
        remembering().round(&ctx, 0).unwrap(),
        Some((tip.clone(), TipOutcome::Verified))
    );
    assert_eq!(world.master(), tip);
    assert_eq!(world.work.runs(), failed_runs);
    assert_eq!(remembering().round(&ctx, 0).unwrap(), None);

    // A peer that was slow to report could still be holding the old failed verdict.
    peer.hub
        .tip_verified(&TipVerified {
            commit: tip.clone(),
            machine: peer.machine.clone(),
            state: TipState::Failed,
            regression_passes: None,
            failed_descendant: None,
            failure: Some(TipFailure {
                checks: vec![first.key.clone()],
                unverified_commits: Vec::new(),
            }),
        })
        .unwrap();
    let conn = hub::open(&database).unwrap();
    let state: String = conn
        .query_row(
            "SELECT state FROM tips WHERE commit_sha = ?1",
            [tip.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(state, "verified");
    assert_eq!(tips::candidate_failures(&conn).unwrap(), None);
    assert_eq!(
        tips::latest_failure(&conn, Timestamp::EPOCH).unwrap(),
        Some((
            TipCheckFailures {
                commit: tip,
                checks: vec![first.key, second.key],
            },
            true,
        ))
    );
    server.stop();
}

#[test]
fn a_superseded_failure_cannot_blame_a_new_candidate_or_replace_a_missing_result() {
    let world = World::new();
    let (server, database, ctx) = running_hub(&world, "superseded");
    let tip = commit("bbbb000");
    let newer = commit("cccc000");
    world.land(&tip);
    select(&ctx, &tip);
    let failed = check("a", 'a');
    world.work.set_checks(&tip, vec![failed.clone()]);
    world.work.set_outcome(&tip, &failed.check, Outcome::Fail);
    world.git.set_changed(&tip, &newer, &["src/a"]);
    world.land(&newer);

    let mut verifier = world.verifier();
    assert_eq!(
        verifier.verify(&ctx, &tip).unwrap(),
        TipOutcome::Failed {
            promoted: None,
            checks: vec![failed.check.clone()],
        }
    );
    assert!(ctx.hub.open_regressions().unwrap().is_empty());

    select(&ctx, &newer);
    ctx.hub
        .tip_check_failures(&TipCheckFailures {
            commit: tip,
            checks: vec![failed.key],
        })
        .unwrap();
    let missing = check("b", 'b');
    world
        .work
        .set_checks(&newer, vec![check("a", 'c'), missing.clone()]);
    world
        .work
        .set_outcome(&newer, &missing.check, Outcome::Incomplete);
    let outcome = verifier.verify(&ctx, &newer);
    let regressions = ctx.hub.open_regressions();
    server.stop();

    let conn = hub::open(&database).unwrap();
    assert_eq!(tips::candidate_failures(&conn).unwrap(), None);
    assert!(regressions.unwrap().is_empty());
    assert_eq!(world.master(), commit("aaaa000"));
    assert!(
        matches!(outcome.unwrap(), TipOutcome::Waiting { checks, .. } if checks == [missing.check])
    );
}

#[test]
fn the_successor_reports_a_failure_on_unchanged_inputs() {
    let world = World::new();
    let (server, _, ctx) = running_hub(&world, "unchanged-inputs");
    let tip = commit("bbbb000");
    let newer = commit("cccc000");
    let failed = check("a", 'a');
    world.land(&tip);
    select(&ctx, &tip);
    world.work.set_checks(&tip, vec![failed.clone()]);
    world.work.set_outcome(&tip, &failed.check, Outcome::Fail);
    // The old round stores its result and reports the failed batch before it can start blame.
    world
        .work
        .run(&ctx, &tip, std::slice::from_ref(&failed.check), false);
    ctx.hub
        .tip_check_failures(&TipCheckFailures {
            commit: tip.clone(),
            checks: vec![failed.key.clone()],
        })
        .unwrap();

    world.land(&newer);
    world.git.set_changed(&tip, &newer, &["src/unrelated"]);
    world.work.set_checks(&newer, vec![failed.clone()]);
    world.work.set_outcome(&newer, &failed.check, Outcome::Fail);
    world.work.add_commit(tip.as_str(), &["src/a"]);

    let outcome = world.verifier().round(&ctx, 0).unwrap();
    assert_eq!(
        outcome,
        Some((
            newer,
            TipOutcome::Failed {
                promoted: None,
                checks: vec![failed.check.clone()],
            },
        ))
    );
    assert_eq!(world.master(), commit("aaaa000"));
    let regressions = ctx.hub.open_regressions().unwrap();
    assert_eq!(regressions.len(), 1);
    assert_eq!(regressions[0].check, failed.check);
    server.stop();
}

#[test]
fn a_failed_tip_promotes_the_newest_fully_cached_landing_before_the_culprit() {
    let world = World::new();
    let tip = commit("ffff000");
    world.land(&tip);
    world.work.set_checks(&tip, vec![check("a", 'f')]);
    world.work.set_outcome(&tip, &name("unit:a"), Outcome::Fail);
    for (sha, hash) in [("bbbb000", 'a'), ("cccc000", 'b'), ("dddd000", 'c')] {
        world.work.add_commit(sha, &["docs/x"]);
        let changed = check("a", hash);
        let extra = check("b", hash);
        world.store(&changed.key, Outcome::Flaky, "m-2");
        if sha != "dddd000" {
            world.store(&extra.key, Outcome::Pass, "m-2");
        }
        world.work.set_checks(&commit(sha), vec![changed, extra]);
    }
    // Even a cached pass at the blamed landing cannot move master past its start.
    world.work.add_commit("eeee000", &["src/a"]);
    world
        .work
        .set_checks(&commit("eeee000"), vec![check("a", 'b')]);
    world.work.add_commit("ffff000", &["docs/x"]);

    let promoted = commit("cccc000");
    assert_eq!(
        world.verifier().verify(&world.ctx("m-1"), &tip).unwrap(),
        TipOutcome::Failed {
            checks: vec![name("unit:a")],
            promoted: Some(promoted.clone()),
        }
    );
    assert_eq!(world.master(), promoted);
    let runs = world.work.runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].1, tip);
    assert_eq!(world.hub.regressions()[0].first_bad, commit("eeee000"));
    let states = world.hub.tip_states();
    assert_eq!(states[0].state, TipState::Failed);
    assert_eq!(states[1].state, TipState::Verified);
}

#[test]
fn a_failed_tip_cannot_promote_an_ancestor_missing_a_required_result() {
    let world = World::new();
    let ancestor = commit("bbbb000");
    let tip = commit("cccc000");
    world.work.add_commit("bbbb000", &["docs/x"]);
    world.work.add_commit("cccc000", &["src/a"]);
    world.land(&tip);
    world.work.set_checks(&tip, vec![check("a", 'c')]);
    world.work.set_outcome(&tip, &name("unit:a"), Outcome::Fail);
    let cached = check("a", 'b');
    world.store(&cached.key, Outcome::Pass, "m-2");
    world.store(&check("b", 'e').key, Outcome::Pass, "m-2");
    world
        .work
        .set_checks(&ancestor, vec![cached, check("b", 'd')]);

    assert_eq!(
        world.verifier().verify(&world.ctx("m-1"), &tip).unwrap(),
        TipOutcome::Failed {
            checks: vec![name("unit:a")],
            promoted: None
        }
    );
    assert_eq!(world.master(), commit("aaaa000"));
    assert_eq!(world.work.runs().len(), 1);
}
