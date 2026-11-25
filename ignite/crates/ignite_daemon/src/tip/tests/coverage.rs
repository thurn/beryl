//! Turning a whole platform off and then bringing it back to verify the current tip.

use ignite_core::fleet::FleetNotice;
use ignite_store::local;

use super::*;
use crate::pool::Pool;

fn set_notice(world: &World, ctx: &Context, pool: &Pool, notice: FleetNotice) {
    world.hub.set_fleet_notice(ctx.machine.clone(), notice);
    let reply = world.hub.fleet_notice(&ctx.machine).unwrap();
    crate::fleet_notice::store(pool, &reply).unwrap();
}

fn windows_check(hash: char, runnable: bool) -> TipCheck {
    let mut windows = check("windows", hash);
    windows.key.platform = Some(Os::Windows);
    windows.runnable = runnable;
    windows
}

#[test]
fn an_ancestor_uses_its_own_coverage_and_records_a_budget_deferral() {
    let world = World::new();
    let ancestor = commit("bbbb000");
    let tip = commit("cccc000");
    world.work.add_commit("bbbb000", &["docs/x"]);
    world.work.add_commit("cccc000", &["src/a"]);
    world.land(&tip);
    world.work.set_checks(&tip, vec![check("a", 'c')]);
    world.work.set_outcome(&tip, &name("unit:a"), Outcome::Fail);
    let covering = super::rounds::in_family("a", "fast");
    world.store(&covering.key, Outcome::Pass, "m-2");
    world
        .work
        .covered_by
        .lock()
        .unwrap()
        .insert("slow".to_owned(), "fast".to_owned());
    let native = windows_check('b', false);
    world
        .hub
        .put_tip_budget(&ignite_core::tipround::TipBudgetPlan {
            commit: ancestor.clone(),
            platform: Os::Windows,
            after: vec![native.key.clone()],
        })
        .unwrap();
    world.work.set_checks(
        &ancestor,
        vec![covering, super::rounds::in_family("a", "slow"), native],
    );

    assert_eq!(
        world.verifier().verify(&world.ctx("m-1"), &tip).unwrap(),
        TipOutcome::Failed {
            checks: vec![name("unit:a")],
            promoted: Some(ancestor.clone()),
        }
    );
    let coverage = world.hub.tip_coverage().unwrap();
    let windows = &coverage.platforms[&Os::Windows];
    assert_eq!(windows.through, ancestor);
    assert_eq!(
        windows.deferred.as_ref().unwrap().last_good,
        commit("aaaa000")
    );
}

#[test]
fn a_tip_can_claim_its_measurement_after_a_same_platform_peer_stops() {
    let world = World::new();
    let ctx = world.ctx("m-1");
    let mut measurement = windows_check('b', true);
    measurement.machine_relative = true;
    let tip = commit("bbbb000");
    world.land(&tip);
    world.work.set_checks(&tip, vec![measurement]);
    assert!(
        world
            .hub
            .tip_claim(&TipClaim {
                commit: tip.clone(),
                family: "unit@windows@m-2".to_owned(),
                batch: 0,
                machine: MachineId::new("m-2").unwrap(),
            })
            .unwrap()
            .granted
    );
    assert_eq!(
        world.verifier().verify(&ctx, &tip).unwrap(),
        TipOutcome::Verified
    );
    assert!(
        world
            .hub
            .tip_claims()
            .iter()
            .any(|claim| claim.family == "unit@windows@m-1")
    );
    assert_eq!(world.master(), tip);
}

#[test]
fn a_run_cannot_pass_a_tip_listing_for_a_different_calibration() {
    let mut measurement = windows_check('b', true);
    measurement.machine_relative = true;
    let mut run = verdict(&measurement, Outcome::Pass, None);
    run.input_hash = Some(InputHash::new("c".repeat(64)).unwrap());
    let mut outcomes = HashMap::new();
    settle_runs(
        &[measurement],
        vec![run],
        &mut outcomes,
        &mut HashSet::new(),
    );
    assert!(outcomes.is_empty());
}

#[test]
fn an_off_platform_defers_its_checks_then_verifies_the_tip_already_on_master() {
    let world = World::new();
    let path = ignite_core::test_files::temp_dir().join("macos.db");
    let pool = Pool::new(local::open(&path).unwrap());
    let mut mac = world.ctx("m-1");
    // The verifier runs as a Mac even when this test runs on a Windows host.
    mac.config
        .machine
        .entry("jarvis".to_owned())
        .or_default()
        .os = Some(Os::Macos);
    let windows = world.ctx("m-2");
    let mut verifier = world.verifier().with_memory(local::open(&path).unwrap());
    let mut macos_check = check("macos", 'a');
    macos_check.key.platform = Some(Os::Macos);
    let first = commit("bbbb000");
    world.land(&first);
    world
        .work
        .set_checks(&first, vec![macos_check.clone(), windows_check('b', false)]);
    assert!(matches!(
        verifier.round_at(&mac, &first, Seq(0)).unwrap(),
        TipOutcome::Waiting { .. }
    ));
    assert_eq!(world.master(), commit("aaaa000"));
    assert!(!verifier.due(&mac, &first));

    set_notice(
        &world,
        &mac,
        &pool,
        FleetNotice {
            version: 1,
            off_platforms: [Os::Windows].into(),
            ..FleetNotice::default()
        },
    );
    assert!(verifier.due(&mac, &first));
    let expected = TipOutcome::VerifiedWithDeferrals {
        checks: [(Os::Windows, vec![name("unit:windows")])].into(),
    };
    assert_eq!(verifier.round_at(&mac, &first, Seq(0)).unwrap(), expected);
    assert_eq!(world.master(), first);
    assert!(world.hub.tips_waiting().is_empty());

    let current = commit("cccc000");
    world.land(&current);
    macos_check.key.hash = InputHash::new("d".repeat(64)).unwrap();
    world
        .work
        .set_checks(&current, vec![macos_check, windows_check('c', false)]);
    assert_eq!(verifier.round_at(&mac, &current, Seq(0)).unwrap(), expected);
    assert_eq!(world.master(), current);
    let coverage = world.hub.tip_coverage().unwrap();
    let verification = &coverage.platforms[&Os::Windows];
    assert_eq!(verification.through, current);
    let gap = verification.deferred.as_ref().unwrap();
    assert_eq!(gap.since, first);
    assert_eq!(gap.last_good, commit("aaaa000"));

    world.work.set_checks(
        &current,
        vec![check("portable", 'f'), windows_check('c', true)],
    );
    let mut returning = world.verifier().with_host("windows");
    assert_eq!(
        returning.round(&windows, 0).unwrap(),
        Some((current.clone(), TipOutcome::Verified))
    );
    let runs: Vec<_> = world
        .work
        .runs()
        .into_iter()
        .filter(|run| run.0 == windows.machine)
        .collect();
    assert_eq!(
        runs,
        [(windows.machine, current, vec![name("unit:windows")], false)]
    );
    assert!(
        world
            .hub
            .tip_coverage()
            .unwrap()
            .warnings()
            .next()
            .is_none()
    );
}

#[test]
fn a_returning_platform_blames_from_before_the_first_deferral() {
    use ignite_core::tipround::{DeferredPlatform, PlatformVerification, TipCoverage};

    let world = World::new();
    let missed = commit("bbbb000");
    let current = commit("cccc000");
    world.land(&current);
    world.git.set_tip("master", current.clone());
    world.hub.set_tip_coverage(TipCoverage {
        platforms: [(
            Os::Windows,
            PlatformVerification {
                through: current.clone(),
                deferred: Some(DeferredPlatform {
                    since: missed.clone(),
                    last_good: commit("aaaa000"),
                }),
            },
        )]
        .into(),
        ..TipCoverage::default()
    });
    let failing = windows_check('c', true);
    world
        .work
        .add_landing("first", &[missed.as_str()], &["src/windows"]);
    world
        .work
        .add_landing("next", &[current.as_str()], &["src/portable"]);
    world
        .work
        .set_outcome(&current, &failing.check, Outcome::Fail);
    world.work.set_checks(&current, vec![failing]);

    assert!(matches!(
        world
            .verifier()
            .with_host("windows")
            .verify(&world.ctx("m-2"), &current)
            .unwrap(),
        TipOutcome::Failed { .. }
    ));
    assert_eq!(world.master(), current);
    let reports = world.hub.regressions();
    assert_eq!(reports[0].first_bad, missed);
    assert_eq!(reports[0].last_good, commit("aaaa000"));
    assert!(
        world.hub.tip_coverage().unwrap().platforms[&Os::Windows]
            .deferred
            .is_some()
    );
}

/// A platform catching up from `master` itself has no promoted landing in its gap, so a
/// failure it finds during a stall reverts its culprit like any other. Refusing every
/// catching-up platform the revert held `master` behind one Windows lint for hours.
#[test]
fn a_platform_catching_up_from_master_reverts_during_a_stall() {
    use ignite_core::tipround::{DeferredPlatform, PlatformVerification, TipCoverage};
    use ignite_core::traits::RevertOutcome;

    let world = World::new();
    let master = world.master();
    let tip = commit("bbbb000");
    world.land(&tip);
    world.hub.set_tip_coverage(TipCoverage {
        platforms: [(
            Os::Windows,
            PlatformVerification {
                through: master.clone(),
                deferred: Some(DeferredPlatform {
                    since: tip.clone(),
                    last_good: master.clone(),
                }),
            },
        )]
        .into(),
        ..TipCoverage::default()
    });
    let failing = windows_check('b', true);
    world
        .work
        .add_landing("tk.40", &[tip.as_str()], &["src/windows/lib.rs"]);
    world.work.set_outcome(&tip, &failing.check, Outcome::Fail);
    world.work.set_checks(&tip, vec![failing]);
    world.git.script_revert(RevertOutcome::Clean {
        commit: commit("cccc000"),
    });
    let mut ctx = world.ctx("m-2");
    ctx.config.revert.stall_minutes = Some(120);
    world.clock.advance(121.0 * 60.0);

    world
        .verifier()
        .with_host("windows")
        .pausing_with(Arc::new(|_| {}))
        .verify(&ctx, &tip)
        .unwrap();

    let reverts = world.hub.reverted_landings();
    assert_eq!(reverts.len(), 1, "{:?}", world.hub.regressions());
    assert_eq!(reverts[0].revert, commit("cccc000"));
    assert_eq!(reverts[0].ticket, Some(ticket("tk.40")));
}

#[test]
#[allow(clippy::too_many_lines)]
fn family_and_checks_only_deferrals_promote_before_windows_catches_up() {
    use ignite_core::fleet::MachineState;
    use ignite_store::hub::{self, fleet_settings};

    use super::rounds::{connect_machine, running_hub, select};

    for checks_only in [false, true] {
        let world = World::new();
        let tip = commit("bbbb000");
        world.land(&tip);
        let scenario = if checks_only {
            "checks-only-deferral"
        } else {
            "family-deferral"
        };
        let (server, database, mut mac) = running_hub(&world, scenario);
        mac.config.machine.get_mut("local").unwrap().os = Some(Os::Macos);
        let windows = connect_machine(world.ctx("m-2"), &server, "windows", Os::Windows);
        select(&mac, &tip);
        let mut mac_verifier = world.verifier().with_host("local");
        let mut win_verifier = world.verifier().with_host("windows");
        let win = windows_check('b', true);
        let portable = check("portable", 'c');
        let mut native_mac = check("macos", 'd');
        native_mac.key.platform = Some(Os::Macos);
        if checks_only {
            let windows_path = database.with_file_name("windows.db");
            let conn = local::open(&windows_path).unwrap();
            let mut hub = hub::open(&database).unwrap();
            fleet_settings::set_machine_state(
                &mut hub,
                windows.machine.as_str(),
                MachineState::ChecksOnly,
                "operator",
                Timestamp::EPOCH,
            )
            .unwrap();
            let notice = FleetNotice {
                version: fleet_settings::settings(&hub).unwrap().version,
                checks_only_platforms: [Os::Windows].into(),
                ..FleetNotice::default()
            };
            crate::fleet_notice::write(
                &conn,
                &FleetNotice {
                    state: MachineState::ChecksOnly,
                    ..notice.clone()
                },
            )
            .unwrap();
            win_verifier = win_verifier.with_memory(conn);
            native_mac.runnable = false;
            world.work.set_checks(
                &tip,
                vec![win.clone(), portable.clone(), native_mac.clone()],
            );
            assert!(matches!(
                win_verifier.round_at(&windows, &tip, Seq(0)).unwrap(),
                TipOutcome::Waiting { .. }
            ));
            assert_eq!(
                world.work.runs(),
                [(
                    windows.machine.clone(),
                    tip.clone(),
                    vec![portable.check.clone()],
                    false
                )]
            );
            assert_ne!(world.master(), tip);
            let conn = local::open(&database.with_file_name("macos.db")).unwrap();
            crate::fleet_notice::write(&conn, &notice).unwrap();
            mac_verifier = mac_verifier.with_memory(conn);
        } else {
            world
                .work
                .defer_platforms
                .lock()
                .unwrap()
                .insert("unit".to_owned(), vec![Os::Windows]);
        }

        // macOS cannot list this Windows-only family at the candidate.
        native_mac.runnable = true;
        world.work.set_checks(&tip, vec![portable, native_mac]);
        let listing = name("unit:listing");
        world.work.unlisted.lock().unwrap().insert(
            tip.clone(),
            vec![Unlisted {
                name: listing.clone(),
                family: "unit".to_owned(),
                message: "waiting for the windows listing of unit".to_owned(),
                reportable: false,
                platforms: vec![Os::Windows],
            }],
        );
        assert_eq!(
            mac_verifier.verify(&mac, &tip).unwrap(),
            TipOutcome::VerifiedWithDeferrals {
                checks: [(Os::Windows, vec![listing])].into(),
            }
        );
        assert_eq!(world.master(), tip);
        assert!(
            mac.hub.tip_coverage().unwrap().platforms[&Os::Windows]
                .deferred
                .is_some()
        );

        world.work.unlisted.lock().unwrap().clear();
        world.work.set_checks(&tip, vec![win.clone()]);
        assert!(win_verifier.due(&windows, &tip));
        assert_eq!(
            win_verifier.round_at(&windows, &tip, Seq(0)).unwrap(),
            TipOutcome::Verified
        );
        let windows_coverage = &mac.hub.tip_coverage().unwrap().platforms[&Os::Windows];
        assert_eq!(windows_coverage.through, tip);
        assert!(windows_coverage.deferred.is_none());
        assert_eq!(
            mac.hub.lookup(&[win.key]).unwrap()[0]
                .as_ref()
                .unwrap()
                .outcome,
            Outcome::Pass
        );
        server.stop();
    }
}
