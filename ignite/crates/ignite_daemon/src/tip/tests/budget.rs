//! `master` moves once the checks selected by each active platform pass. Checks outside
//! each verifier's budget run afterward.

use std::collections::BTreeMap;

use ignite_core::budget::DEFAULT_TIP_RATE;
use ignite_core::gate::{GateCheck, History};
use ignite_core::score::{AreaRecord, Cost};

use super::*;
use crate::checks::Scoring;

/// A record of `failures` in 20 runs after changes to `src`, at 100 billion a run.
fn scored(text: &str, failures: u32) -> GateCheck {
    GateCheck {
        family_startup: ignite_core::score::Cost::Billions(0.0),
        check: name(text),
        always: false,
        edited: false,
        cached: false,
        history: Some(History {
            areas: BTreeMap::from([("src".to_owned(), AreaRecord { runs: 20, failures })]),
            median_cost: Cost::Billions(100.0),
        }),
        family_rate: 0.01,
        family_median_cost: Cost::Billions(100.0),
        needs: Vec::new(),
        group: None,
        regression: None,
    }
}

fn ran_with_master(world: &World) -> Vec<(Vec<String>, Commit)> {
    world
        .work
        .masters
        .lock()
        .unwrap()
        .iter()
        .map(|(checks, master)| {
            let mut names: Vec<String> = checks.iter().map(ToString::to_string).collect();
            names.sort();
            (names, master.clone())
        })
        .collect()
}

fn named_check(family: &str, text: &str, hash: char) -> TipCheck {
    let mut check = check(text, hash);
    check.family = family.to_owned();
    check.check = name(&format!("{family}:{text}"));
    check.key.check.clone_from(&check.check);
    check
}

#[test]
fn master_moves_once_the_budgeted_checks_pass_and_a_later_failure_holds_it() {
    let world = World::new();
    let mut ctx = world.ctx("m-1");
    // 250 billion at the default rate: two checks of 100 billion fit, a third does not.
    ctx.config.tip.seconds = 250.0 / DEFAULT_TIP_RATE;
    let mut verifier = world.verifier();
    world.work.add_landing("tk.30", &["bbbb000"], &["src/c"]);
    *world.work.scoring.lock().unwrap() = Some(Scoring {
        areas: vec!["src".to_owned()],
        checks: vec![
            scored("unit:a", 9),
            scored("unit:b", 6),
            scored("unit:c", 1),
            scored("unit:d", 0),
        ],
    });
    let tip = commit("bbbb000");
    world.land(&tip);
    let checks = vec![
        check("a", 'a'),
        check("b", 'b'),
        check("c", 'c'),
        check("d", 'd'),
    ];
    world.work.set_checks(&tip, checks.clone());
    world.work.set_outcome(&tip, &name("unit:c"), Outcome::Fail);

    let outcome = verifier.verify(&ctx, &tip).unwrap();

    // The two likeliest checks ran with `master` still behind; once they passed it moved, and
    // the other two ran at the tip.
    assert_eq!(outcome, TipOutcome::Verified);
    assert_eq!(world.master(), tip);
    let old = commit("aaaa000");
    assert_eq!(
        ran_with_master(&world),
        [
            (vec!["unit:a".to_owned(), "unit:b".to_owned()], old.clone()),
            (vec!["unit:c".to_owned(), "unit:d".to_owned()], tip.clone()),
        ]
    );
    // The failure past the budget filed one regression.
    let reports = world.hub.regressions();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].check, name("unit:c"));
    // The round's record says how the budget split the checks.
    let round = world.hub.tip_rounds().pop().unwrap();
    let budget = round.budget.unwrap();
    assert_eq!((budget.within, budget.after, budget.forced), (2, 2, 0));
    assert!((budget.within_billions - 200.0).abs() < 1e-9);
    assert!((budget.after_billions - 200.0).abs() < 1e-9);
    assert!(budget.master_s.is_some());
    // Every check past the budget ran, so the round left none unrun.
    assert!(
        budget.unrun.is_empty() && budget.ended_by.is_none(),
        "{budget:?}"
    );

    // The next landing leaves the failing check's inputs alone: its cached failure is run
    // again before `master` moves, fails, and holds `master` at the last tip.
    let next = commit("cccc000");
    world.land(&next);
    world.work.set_checks(&next, checks);
    world
        .work
        .set_outcome(&next, &name("unit:c"), Outcome::Fail);
    let outcome = verifier.verify(&ctx, &next).unwrap();
    assert_eq!(
        outcome,
        TipOutcome::Failed {
            checks: vec![name("unit:c")]
        }
    );
    assert_eq!(world.master(), tip);
    assert_eq!(world.hub.regressions().len(), 1, "reported once");
}

#[test]
fn both_platforms_verify_one_candidate_across_a_landing() {
    let world = World::new();
    let mut mac = world.ctx("m-1");
    mac.config.machine.get_mut("local").unwrap().os = Some(Os::Macos);
    // Each round can afford only one portable check. The second round reuses the first
    // pass and chooses the next check, in a different family with its own batch claim.
    mac.config.tip.seconds = 150.0 / DEFAULT_TIP_RATE;
    let mut windows = world.ctx("m-2");
    windows.config.clone_from(&mac.config);
    let first = check("first", 'a');
    let second = named_check("other", "second", 'b');
    let last = named_check("other", "last", 'c');
    let mut mac_check = named_check("platform", "required", 'd');
    mac_check.key.platform = Some(Os::Macos);
    let mut windows_check = named_check("platform", "required", 'e');
    windows_check.key.platform = Some(Os::Windows);
    let mut required = scored("platform:required", 0);
    required.always = true;
    *world.work.scoring.lock().unwrap() = Some(Scoring {
        areas: vec!["src".to_owned()],
        checks: vec![
            scored("unit:first", 9),
            scored("other:second", 6),
            scored("other:last", 1),
            required,
        ],
    });
    let tip = commit("bbbb000");
    world.land(&tip);
    let previous = world.master();
    let listing = |local| {
        let mut mac_check = mac_check.clone();
        let mut windows_check = windows_check.clone();
        mac_check.runnable = local == Os::Macos;
        windows_check.runnable = local == Os::Windows;
        vec![
            first.clone(),
            second.clone(),
            last.clone(),
            mac_check,
            windows_check,
        ]
    };
    let keys = [mac_check.key.clone(), windows_check.key.clone()];

    world.work.set_checks(&tip, listing(Os::Macos));
    assert_eq!(
        world.verifier().with_host("local").round(&mac, 0).unwrap(),
        Some((
            tip.clone(),
            TipOutcome::Waiting {
                checks: vec![name("platform:required")],
                unrunnable: Vec::new(),
            },
        ))
    );
    assert_eq!(world.master(), previous);
    let cached = world.hub.lookup(&keys).unwrap();
    assert_eq!(cached[0].as_ref().unwrap().outcome, Outcome::Pass);
    assert!(cached[1].is_none());

    // Another landing arrives before the Windows loop chooses its target. Its required
    // check reads different inputs there, so a pass on the new tip could not help this one.
    let newer = commit("cccc000");
    world.land(&newer);
    let mut newer_checks = listing(Os::Windows);
    newer_checks.last_mut().unwrap().key.hash = InputHash::new("f".repeat(64)).unwrap();
    world.work.set_checks(&newer, newer_checks);
    world.work.set_checks(&tip, listing(Os::Windows));
    let mut verifier = world.verifier().with_host("windows");
    assert_eq!(
        verifier.round(&windows, 0).unwrap(),
        Some((tip.clone(), TipOutcome::Verified))
    );
    assert_eq!(world.master(), tip);
    assert!(
        world
            .hub
            .lookup(&keys)
            .unwrap()
            .iter()
            .all(|hit| { hit.as_ref().is_some_and(|hit| hit.outcome == Outcome::Pass) })
    );
    assert_eq!(
        ran_with_master(&world),
        [
            (vec!["platform:required".to_owned()], previous.clone()),
            (vec!["unit:first".to_owned()], previous.clone()),
            (vec!["platform:required".to_owned()], previous.clone()),
            (vec!["other:second".to_owned()], previous),
        ]
    );
    let budget = world.hub.tip_rounds().pop().unwrap().budget.unwrap();
    assert_eq!(budget.unrun, BTreeMap::from([("other".to_owned(), 1)]));
    assert_eq!(budget.ended_by, Some(newer.clone()));
    assert_eq!(
        verifier.round(&windows, 0).unwrap(),
        Some((newer.clone(), TipOutcome::Verified))
    );
    assert_eq!(world.master(), newer);
}

#[test]
fn a_failed_or_incomplete_native_check_holds_the_shared_candidate() {
    for outcome in [Outcome::Fail, Outcome::Incomplete] {
        let world = World::new();
        let previous = world.master();
        let mac = world.ctx("m-1");
        let windows = world.ctx("m-2");
        let tip = commit("bbbb000");
        let newer = commit("cccc000");
        let mut mac_check = check("mac", 'a');
        mac_check.key.platform = Some(Os::Macos);
        let mut windows_check = check("windows", 'b');
        windows_check.key.platform = Some(Os::Windows);
        windows_check.runnable = false;

        world.land(&tip);
        world
            .work
            .set_checks(&tip, vec![mac_check.clone(), windows_check.clone()]);
        let mut mac_verifier = world.verifier();
        assert_eq!(
            mac_verifier.round(&mac, 0).unwrap(),
            Some((
                tip.clone(),
                TipOutcome::Waiting {
                    checks: vec![windows_check.check.clone()],
                    unrunnable: Vec::new(),
                },
            ))
        );

        world.land(&newer);
        mac_check.runnable = false;
        windows_check.runnable = true;
        world
            .work
            .set_checks(&tip, vec![mac_check.clone(), windows_check.clone()]);
        world.work.set_outcome(&tip, &windows_check.check, outcome);
        let mut windows_verifier = world.verifier();
        let checks = vec![windows_check.check.clone()];
        let expected = if outcome == Outcome::Fail {
            TipOutcome::Failed { checks }
        } else {
            TipOutcome::Waiting {
                checks,
                unrunnable: Vec::new(),
            }
        };
        assert_eq!(
            windows_verifier.round(&windows, 0).unwrap(),
            Some((tip, expected))
        );
        assert_eq!(world.master(), previous);

        // A fix changes the Windows inputs. The next candidate still needs its own pass.
        windows_check.key.hash = InputHash::new("c".repeat(64)).unwrap();
        world
            .work
            .set_checks(&newer, vec![mac_check, windows_check]);
        assert_eq!(
            windows_verifier.round(&windows, 0).unwrap(),
            Some((newer.clone(), TipOutcome::Verified))
        );
        assert_eq!(world.master(), newer);
    }
}

#[test]
fn both_verifiers_switch_to_the_replacement_after_a_required_failure() {
    let world = World::new();
    let previous = world.master();
    let mac = world.ctx("m-1");
    let windows = world.ctx("m-2");
    let tip = commit("bbbb000");
    let newer = commit("cccc000");
    world.land(&tip);

    let native = |text, hash, platform| {
        let mut check = named_check("native", text, hash);
        check.key.platform = Some(platform);
        check
    };
    let mac_check = native("mac", 'a', Os::Macos);
    let windows_check = native("windows", 'b', Os::Windows);
    let fixed_mac = native("mac", 'c', Os::Macos);
    let new_windows = native("windows", 'd', Os::Windows);
    let first = check("first", 'e');
    let second = check("second", 'f');
    world.store(&windows_check.key, Outcome::Pass, "m-2");
    for check in [&first, &second] {
        world.hub.set_check_cost(check.check.as_str(), 60.0);
    }
    let elsewhere = |mut check: TipCheck| {
        check.runnable = false;
        check
    };
    let mac_work = Arc::new(FakeWork::default());
    for (at, mac_check, windows_check) in [
        (&tip, &mac_check, &windows_check),
        (&newer, &fixed_mac, &new_windows),
    ] {
        mac_work.set_checks(
            at,
            vec![mac_check.clone(), elsewhere(windows_check.clone())],
        );
        world.work.set_checks(
            at,
            vec![
                elsewhere(mac_check.clone()),
                windows_check.clone(),
                first.clone(),
                second.clone(),
            ],
        );
    }
    mac_work.set_outcome(&tip, &mac_check.check, Outcome::Fail);

    // Windows has its native result. While its first portable batch runs, macOS verifies
    // the same candidate, records a required failure, and selects the landing.
    world.work.on_run.lock().unwrap().insert(tip.clone(), {
        let (ctx, work, git) = (mac.clone(), mac_work.clone(), world.git.clone());
        let (tip, newer) = (tip.clone(), newer.clone());
        Arc::new(move || {
            let mut verifier = TipVerifier::new(work.clone(), "test");
            git.set_tip("staging", newer.clone());
            assert_eq!(
                verifier.round(&ctx, 0).unwrap(),
                Some((
                    tip.clone(),
                    TipOutcome::Failed {
                        checks: vec![mac_check.check.clone()]
                    }
                ))
            );
            assert_eq!(verifier.next_target(&ctx).unwrap(), Some(newer.clone()));
        })
    });

    let mut verifier = world.verifier();
    assert_eq!(
        verifier.round(&windows, 0).unwrap(),
        Some((tip.clone(), TipOutcome::Superseded { by: newer.clone() }))
    );
    assert_eq!(world.master(), previous);
    let old_batches: Vec<_> = world
        .hub
        .tip_claim_asks()
        .into_iter()
        .filter(|claim| claim.commit == tip && claim.family == "unit")
        .map(|claim| claim.batch)
        .collect();
    assert_eq!(old_batches, [0]);

    let (at, outcome) = verifier.round(&windows, 0).unwrap().unwrap();
    assert_eq!(at, newer);
    assert!(matches!(outcome, TipOutcome::Waiting { checks, .. } if checks == [fixed_mac.check]));
    assert_eq!(world.master(), previous);
    let replacement_runs: Vec<_> = world
        .work
        .runs()
        .into_iter()
        .filter(|run| run.1 == newer)
        .flat_map(|run| run.2)
        .collect();
    assert_eq!(replacement_runs, [new_windows.check, second.check]);
    assert_eq!(
        TipVerifier::new(mac_work, "test").round(&mac, 0).unwrap(),
        Some((newer.clone(), TipOutcome::Verified))
    );
    assert_eq!(world.master(), newer);
}

/// A check with an open regression ticket runs before `master` moves whatever its score, so a
/// fix that has not landed keeps holding `master`.
#[test]
fn an_open_regression_runs_before_master_whatever_the_budget() {
    let world = World::new();
    let mut ctx = world.ctx("m-1");
    ctx.config.tip.seconds = 150.0 / DEFAULT_TIP_RATE;
    let mut broken = scored("unit:c", 0);
    broken.regression = Some(ticket("tk.31"));
    *world.work.scoring.lock().unwrap() = Some(Scoring {
        areas: vec!["src".to_owned()],
        checks: vec![scored("unit:a", 9), broken],
    });
    world.work.add_landing("tk.32", &["bbbb000"], &["src/c"]);
    let tip = commit("bbbb000");
    world.land(&tip);
    world
        .work
        .set_checks(&tip, vec![check("a", 'a'), check("c", 'c')]);
    world.work.set_outcome(&tip, &name("unit:c"), Outcome::Fail);

    let outcome = world.verifier().verify(&ctx, &tip).unwrap();

    assert_eq!(
        outcome,
        TipOutcome::Failed {
            checks: vec![name("unit:c")]
        }
    );
    assert_eq!(world.master(), commit("aaaa000"));
}

/// Once `master` has moved, a newer `staging` tip ends the checks past the budget at the next
/// batch, even while their family is under way. The tip loop waits for the round, so a round
/// that ran every check past the budget kept every newer tip from being verified for hours.
#[test]
fn a_newer_tip_ends_the_checks_past_the_budget_at_the_next_batch() {
    let world = World::new();
    let mut ctx = world.ctx("m-1");
    ctx.config.tip.seconds = 150.0 / DEFAULT_TIP_RATE;
    *world.work.scoring.lock().unwrap() = Some(Scoring {
        areas: vec!["src".to_owned()],
        checks: vec![scored("unit:a", 9), scored("unit:b", 0)],
    });
    world.work.add_landing("tk.33", &["bbbb000"], &["src/b"]);
    let tip = commit("bbbb000");
    world.land(&tip);
    world
        .work
        .set_checks(&tip, vec![check("a", 'a'), check("b", 'b')]);
    // A landing arrives once `master` has moved, while the family past the budget is being
    // built and listed, before its first batch is claimed.
    let git = world.git.clone();
    let moved = tip.clone();
    world.work.before_batches.lock().unwrap().insert(
        "unit".to_owned(),
        Arc::new(move || {
            if git.tip("master").unwrap() == moved {
                git.set_tip("staging", commit("cccc000"));
            }
        }),
    );

    let outcome = world.verifier().verify(&ctx, &tip).unwrap();

    assert_eq!(outcome, TipOutcome::Verified);
    assert_eq!(world.master(), tip);
    // Only the check within the budget ran; the one past it is left to the newer tip.
    assert_eq!(
        ran_with_master(&world),
        [(vec!["unit:a".to_owned()], commit("aaaa000"))]
    );
    // The round's record names the check it left and the tip that ended it, which the hub
    // reads when a landing later finds that check failing.
    let budget = world.hub.tip_rounds().pop().unwrap().budget.unwrap();
    assert_eq!(budget.unrun, BTreeMap::from([("unit".to_owned(), 1)]));
    assert_eq!(budget.ended_by, Some(commit("cccc000")));
}

#[test]
#[allow(clippy::too_many_lines)]
fn an_active_peer_finishes_its_budget_before_master_and_catches_up_afterward() {
    let world = World::new();
    let previous = world.master();
    let tip = commit("bbbb000");
    world.land(&tip);
    world.git.set_tip("green", previous.clone());

    let (server, _, mut mac) = super::rounds::running_hub(&world, "native-budget");
    mac.config.machine.get_mut("local").unwrap().os = Some(Os::Macos);
    mac.config.repository.green = Some("green".to_owned());
    mac.config.tip.seconds = 150.0 / DEFAULT_TIP_RATE;
    let mut windows =
        super::rounds::connect_machine(world.ctx("m-2"), &server, "windows", Os::Windows);
    windows.config.clone_from(&mac.config);
    super::rounds::select(&mac, &tip);
    let mut mac_verifier = world.verifier().with_host("local");
    let mut windows_verifier = world.verifier().with_host("windows");

    let native = |family, check, hash, platform| {
        let mut native = named_check(family, check, hash);
        native.key.platform = Some(platform);
        native
    };
    let mac_before = native("mac-before", "a", 'a', Os::Macos);
    let mac_after = native("mac-after", "a", 'b', Os::Macos);
    let win_before = native("win-before", "a", 'c', Os::Windows);
    let win_after = native("win-after", "a", 'd', Os::Windows);
    let win_always = native("win-always", "a", 'e', Os::Windows);
    let mut win_build = native("win-build", "a", 'f', Os::Windows);
    win_build.kind = TipCheckKind::Build;
    let set_listing = |platform| {
        let mut checks = vec![
            mac_before.clone(),
            mac_after.clone(),
            win_before.clone(),
            win_after.clone(),
            win_always.clone(),
            win_build.clone(),
        ];
        for check in &mut checks {
            check.runnable = check.key.platform == Some(platform);
        }
        world.work.set_checks(&tip, checks);
    };
    let mut always = scored(win_always.check.as_str(), 0);
    always.always = true;
    *world.work.scoring.lock().unwrap() = Some(Scoring {
        areas: vec!["src".to_owned()],
        checks: vec![
            scored(mac_before.check.as_str(), 9),
            scored(mac_after.check.as_str(), 1),
            scored(win_before.check.as_str(), 9),
            scored(win_after.check.as_str(), 1),
            always,
        ],
    });

    set_listing(Os::Macos);
    assert_eq!(
        mac_verifier.round_at(&mac, &tip, Seq(0)).unwrap(),
        TipOutcome::Waiting {
            checks: vec![
                win_after.check.clone(),
                win_always.check.clone(),
                win_before.check.clone(),
                win_build.check.clone(),
            ],
            unrunnable: Vec::new(),
        }
    );
    assert_eq!(world.master(), previous);

    set_listing(Os::Windows);
    assert_eq!(
        windows_verifier.round_at(&windows, &tip, Seq(0)).unwrap(),
        TipOutcome::VerifiedWithDeferrals {
            checks: [(Os::Macos, vec![mac_after.check.clone()])].into(),
        }
    );
    assert_eq!(world.master(), tip);
    assert_eq!(world.git.tip("green").unwrap(), tip);
    assert!(mac_verifier.due(&mac, &tip));

    set_listing(Os::Macos);
    assert_eq!(
        mac_verifier.round_at(&mac, &tip, Seq(0)).unwrap(),
        TipOutcome::Verified
    );
    assert!(mac.hub.tip_coverage().unwrap().warnings().next().is_none());
    assert_eq!(
        ran_with_master(&world),
        [
            (vec![mac_before.check.to_string()], previous.clone()),
            (vec![win_build.check.to_string()], previous.clone()),
            (vec![win_always.check.to_string()], previous.clone()),
            (vec![win_before.check.to_string()], previous),
            (vec![win_after.check.to_string()], tip.clone()),
            (vec![mac_after.check.to_string()], tip),
        ]
    );
    server.stop();
}
