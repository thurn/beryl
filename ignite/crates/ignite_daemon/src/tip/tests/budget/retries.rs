//! Shared tip budget decisions across retrying verifiers.

use super::*;

#[test]
#[allow(clippy::too_many_lines)]
fn two_verifiers_keep_the_same_budget_while_verdicts_arrive() {
    let world = World::new();
    let previous = world.master();
    let tip = commit("bbbb000");
    world.land(&tip);
    world.git.set_tip("green", previous.clone());
    let mut mac = world.ctx("m-1");
    mac.config.machine.get_mut("local").unwrap().os = Some(Os::Macos);
    mac.config.repository.green = Some("green".to_owned());
    mac.config.tip.seconds = 250.0 / DEFAULT_TIP_RATE;
    let mut windows = world.ctx("m-2");
    windows.config.clone_from(&mac.config);
    let mut mac_verifier = world.verifier().with_host("local");
    let mut windows_verifier = world.verifier().with_host("windows");

    let first = check("first", 'a');
    let held = named_check("selected", "held", 'b');
    let unrelated = named_check("lower", "cached", 'c');
    let after = named_check("lower", "remaining", 'd');
    let mut build = named_check("compile", "windows", 'e');
    build.key.platform = Some(Os::Windows);
    build.kind = TipCheckKind::Build;
    let list = |runnable| {
        let mut build = build.clone();
        build.runnable = runnable;
        world.work.set_checks(
            &tip,
            vec![
                first.clone(),
                held.clone(),
                unrelated.clone(),
                after.clone(),
                build,
            ],
        );
    };
    *world.work.scoring.lock().unwrap() = Some(Scoring {
        areas: vec!["src".to_owned()],
        checks: vec![
            scored(first.check.as_str(), 9),
            scored(held.check.as_str(), 6),
            scored(unrelated.check.as_str(), 3),
            scored(after.check.as_str(), 1),
        ],
    });
    for (family, owner) in [
        ("selected", &windows.machine),
        ("compile@windows", &mac.machine),
    ] {
        assert!(
            world
                .hub
                .tip_claim(&TipClaim {
                    commit: tip.clone(),
                    family: family.to_owned(),
                    batch: 0,
                    machine: owner.clone(),
                })
                .unwrap()
                .granted
        );
    }
    world
        .work
        .set_outcome(&tip, &held.check, Outcome::Incomplete);
    world
        .work
        .unreasoned
        .lock()
        .unwrap()
        .insert(held.check.clone());
    let waiting = |checks| TipOutcome::Waiting {
        checks,
        unrunnable: Vec::new(),
    };

    list(false);
    let both = waiting(vec![build.check.clone(), held.check.clone()]);
    assert_eq!(mac_verifier.verify(&mac, &tip).unwrap(), both);
    let initial = world.hub.tip_selection(&tip).unwrap().unwrap().portable;
    assert_eq!(initial.after.len(), 2);
    assert!(initial.after.contains(&unrelated.key) && initial.after.contains(&after.key));

    world.store(&unrelated.key, Outcome::Pass, "m-1");
    list(true);
    assert_eq!(windows_verifier.verify(&windows, &tip).unwrap(), both);
    list(false);
    assert_eq!(mac_verifier.verify(&mac, &tip).unwrap(), both);
    assert_eq!(
        world.hub.tip_selection(&tip).unwrap().unwrap().portable,
        initial
    );
    assert_eq!(world.master(), previous);

    world.store(&held.key, Outcome::Pass, "m-2");
    assert_eq!(
        mac_verifier.verify(&mac, &tip).unwrap(),
        waiting(vec![build.check.clone()])
    );
    assert_eq!(world.git.tip("green").unwrap(), previous);
    world.store(&build.key, Outcome::Pass, "m-2");
    assert_eq!(
        mac_verifier.verify(&mac, &tip).unwrap(),
        TipOutcome::Verified
    );
    assert_eq!(world.git.tip("green").unwrap(), tip);
    assert_eq!(world.master(), tip);
    assert_eq!(
        ran_with_master(&world),
        [
            (vec![first.check.to_string()], previous.clone()),
            (vec![held.check.to_string()], previous),
            (vec![after.check.to_string()], tip),
        ]
    );
}

#[test]
fn a_saved_deferral_does_not_excuse_a_failure_or_an_open_regression() {
    for open_regression in [false, true] {
        let world = World::new();
        let mut ctx = world.ctx("m-1");
        ctx.config.tip.seconds = 150.0 / DEFAULT_TIP_RATE;
        let previous = world.master();
        let tip = commit("bbbb000");
        world.land(&tip);
        let selected = check("selected", 'a');
        let deferred = named_check("lower", "deferred", 'b');
        world
            .work
            .set_checks(&tip, vec![selected.clone(), deferred.clone()]);
        *world.work.scoring.lock().unwrap() = Some(Scoring {
            areas: vec!["src".to_owned()],
            checks: vec![
                scored(selected.check.as_str(), 9),
                scored(deferred.check.as_str(), 1),
            ],
        });
        assert!(
            world
                .hub
                .tip_claim(&TipClaim {
                    commit: tip.clone(),
                    family: selected.family.clone(),
                    batch: 0,
                    machine: MachineId::new("m-2").unwrap(),
                })
                .unwrap()
                .granted
        );
        let mut verifier = world.verifier();
        assert!(
            matches!(verifier.verify(&ctx, &tip).unwrap(), TipOutcome::Waiting { checks, .. }
            if checks == [selected.check.clone()])
        );
        assert_eq!(
            world
                .hub
                .tip_selection(&tip)
                .unwrap()
                .unwrap()
                .portable
                .after,
            std::slice::from_ref(&deferred.key)
        );

        world.store(&selected.key, Outcome::Pass, "m-2");
        if open_regression {
            world
                .hub
                .set_open_regressions(vec![ignite_core::protocol::hub::OpenRegression {
                    check: deferred.check.clone(),
                    ticket: ticket("tk.31"),
                    off: false,
                }]);
            world
                .work
                .set_outcome(&tip, &deferred.check, Outcome::Incomplete);
            assert!(
                matches!(verifier.verify(&ctx, &tip).unwrap(), TipOutcome::Waiting { checks, .. }
                if checks == [deferred.check.clone()])
            );
        } else {
            world.store(&deferred.key, Outcome::Fail, "m-2");
            world.work.set_outcome(&tip, &deferred.check, Outcome::Fail);
            assert_eq!(
                verifier.verify(&ctx, &tip).unwrap(),
                TipOutcome::Failed {
                    checks: vec![deferred.check.clone()],
                    promoted: None
                }
            );
        }
        assert_eq!(world.master(), previous);
        assert_eq!(
            ran_with_master(&world),
            [(vec![deferred.check.to_string()], previous)]
        );
    }
}
