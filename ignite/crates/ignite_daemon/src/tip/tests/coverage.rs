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
