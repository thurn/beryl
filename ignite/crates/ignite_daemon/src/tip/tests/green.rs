//! The green branch: a round runs its build checks first and moves `green` once they pass,
//! whatever the rest of the round finds.

use std::sync::mpsc;
use std::time::Duration;

use super::rounds::{connect_machine, running_hub, select};
use super::*;

/// `unit:<text>`, an ordinary check, and `vet:<text>`, a build check. `vet` sorts after `unit`,
/// so without a build step its family would run second.
fn vet(text: &str, hash: char) -> TipCheck {
    let mut check = check(text, hash);
    check.check = name(&format!("vet:{text}"));
    check.key.check = check.check.clone();
    check.family = "vet".to_owned();
    check.kind = TipCheckKind::Build;
    check
}

/// A world whose `vet` family is declared `builds = true`, with `staging` at `bbbb000` listing
/// `vet:a` and `unit:b`.
fn world() -> (World, Commit) {
    let world = World::new();
    let tip = commit("bbbb000");
    world.land(&tip);
    world
        .work
        .set_checks(&tip, vec![vet("a", 'a'), check("b", 'b')]);
    (world, tip)
}

fn green_ctx(world: &World) -> Context {
    green_ctx_for(world, "m-1")
}

fn green_ctx_for(world: &World, machine: &str) -> Context {
    let mut ctx = world.ctx(machine);
    ctx.config.repository.green = Some("green".to_owned());
    ctx
}

/// The checks of each run, in the order they ran.
fn run_order(world: &World) -> Vec<Vec<String>> {
    world
        .work
        .runs()
        .into_iter()
        .map(|run| run.2.iter().map(ToString::to_string).collect())
        .collect()
}

fn branches_pushed(world: &World) -> Vec<(String, Commit)> {
    world
        .git
        .pushes()
        .into_iter()
        .map(|(_, at, branch)| (branch, at))
        .collect()
}

#[test]
fn the_build_checks_run_first_and_green_moves_before_master() {
    let (world, tip) = world();

    let outcome = world.verifier().verify(&green_ctx(&world), &tip).unwrap();

    assert_eq!(outcome, TipOutcome::Verified);
    assert_eq!(run_order(&world), [vec!["vet:a"], vec!["unit:b"]]);
    assert_eq!(
        branches_pushed(&world),
        [
            ("green".to_owned(), tip.clone()),
            ("master".to_owned(), tip.clone())
        ]
    );
}

/// `green` says the tip compiles, and a failing test does not take that back.
#[test]
fn green_moves_when_the_build_passes_although_another_check_fails() {
    let (world, tip) = world();
    world.work.set_outcome(&tip, &name("unit:b"), Outcome::Fail);

    let outcome = world.verifier().verify(&green_ctx(&world), &tip).unwrap();

    assert_eq!(
        outcome,
        TipOutcome::Failed {
            promoted: None,
            checks: vec![name("unit:b")]
        }
    );
    assert_eq!(world.git.tip("green").unwrap(), tip);
    assert_eq!(world.master(), commit("aaaa000"));
}

/// A tip that does not compile moves neither branch, is blamed, and runs nothing past its
/// build checks.
#[test]
fn a_failing_build_check_moves_neither_branch() {
    let (world, tip) = world();
    world.work.add_landing("tk.40", &["bbbb000"], &["src/a"]);
    world.work.set_outcome(&tip, &name("vet:a"), Outcome::Fail);

    let outcome = world.verifier().verify(&green_ctx(&world), &tip).unwrap();

    assert_eq!(
        outcome,
        TipOutcome::Failed {
            promoted: None,
            checks: vec![name("vet:a")]
        }
    );
    assert_eq!(run_order(&world), [vec!["vet:a"]]);
    assert!(branches_pushed(&world).is_empty());
    assert_eq!(world.hub.regressions().len(), 1);
}

#[test]
fn an_incomplete_build_check_keeps_green_where_it_is() {
    let (world, tip) = world();
    world
        .work
        .set_outcome(&tip, &name("vet:a"), Outcome::Incomplete);

    let outcome = world.verifier().verify(&green_ctx(&world), &tip).unwrap();

    assert_eq!(
        outcome,
        TipOutcome::Waiting {
            checks: vec![name("vet:a")],
            unrunnable: Vec::new(),
        }
    );
    assert!(branches_pushed(&world).is_empty());
}

/// Without a green branch configured, a build family is an ordinary family: no build step, the
/// families run in their usual order, and only `master` moves.
#[test]
fn without_green_a_round_runs_as_before() {
    let (world, tip) = world();

    let outcome = world.verifier().verify(&world.ctx("m-1"), &tip).unwrap();

    assert_eq!(outcome, TipOutcome::Verified);
    assert_eq!(run_order(&world), [vec!["unit:b"], vec!["vet:a"]]);
    assert_eq!(branches_pushed(&world), [("master".to_owned(), tip)]);
}

/// A landing that arrives once `green` has moved leaves it at the tip the round built, and a
/// newer tip that does not compile does not move it back or forward.
#[test]
fn a_newer_tip_does_not_undo_green() {
    let (world, tip) = world();
    let newer = commit("cccc000");
    let git = world.git.clone();
    let landed = newer.clone();
    world.work.before_batches.lock().unwrap().insert(
        "unit".to_owned(),
        Arc::new(move || git.set_tip("staging", landed.clone())),
    );
    let ctx = green_ctx(&world);
    let mut verifier = world.verifier();

    assert_eq!(verifier.verify(&ctx, &tip).unwrap(), TipOutcome::Verified);
    assert_eq!(world.git.tip("green").unwrap(), tip);

    world.work.before_batches.lock().unwrap().clear();
    world
        .work
        .set_checks(&newer, vec![vet("a", 'c'), check("b", 'b')]);
    world.work.add_landing("tk.41", &["cccc000"], &["src/a"]);
    world
        .work
        .set_outcome(&newer, &name("vet:a"), Outcome::Fail);
    let outcome = verifier.verify(&ctx, &newer).unwrap();

    assert_eq!(
        outcome,
        TipOutcome::Failed {
            promoted: None,
            checks: vec![name("vet:a")]
        }
    );
    assert_eq!(world.git.tip("green").unwrap(), tip);
    assert_eq!(world.master(), tip);
}

fn hold_ordinary_listing(work: &FakeWork) -> (mpsc::Receiver<ListingPhase>, mpsc::Sender<()>) {
    let (started, phases) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    *work.before_listing.lock().unwrap() = Some(Box::new(move |phase| {
        started.send(phase).unwrap();
        if matches!(phase, ListingPhase::All | ListingPhase::Other) {
            resume.recv().unwrap();
        }
    }));
    (phases, release)
}

fn wait_for_ordinary_listing(phases: &mpsc::Receiver<ListingPhase>) {
    for expected in [
        ListingPhase::Build,
        ListingPhase::PeerBuild,
        ListingPhase::Other,
    ] {
        assert_eq!(
            phases.recv_timeout(Duration::from_secs(10)).unwrap(),
            expected
        );
    }
}

/// Both verifiers share the result cache. Once both native builds pass, the second verifier
/// can publish green while the first is still listing ordinary checks. Master still needs
/// the ordinary listing and its results from both platforms.
#[test]
fn native_builds_publish_green_while_another_listing_is_held() {
    let (world, tip) = world();
    let (server, _, mut mac) = running_hub(&world, "green-before-listing");
    mac.config.repository.green = Some("green".to_owned());
    let windows = connect_machine(
        green_ctx_for(&world, "m-2"),
        &server,
        "windows",
        Os::Windows,
    );
    select(&mac, &tip);
    let windows_work = Arc::new(FakeWork::default());
    let previous = world.master();
    let for_platform = |mut check: TipCheck, os| {
        check.key.platform = Some(os);
        check
    };
    let mac_build = for_platform(vet("a", 'a'), Os::Macos);
    let windows_build = for_platform(vet("a", 'b'), Os::Windows);
    let mac_test = for_platform(check("b", 'c'), Os::Macos);
    let windows_test = for_platform(check("b", 'd'), Os::Windows);
    let checks = |os| {
        [
            mac_build.clone(),
            windows_build.clone(),
            mac_test.clone(),
            windows_test.clone(),
        ]
        .into_iter()
        .map(|mut check| {
            check.runnable = check.key.platform == Some(os);
            check
        })
        .collect()
    };
    world.work.set_checks(&tip, checks(Os::Macos));
    windows_work.set_checks(&tip, checks(Os::Windows));
    let outdated = for_platform(vet("a", 'e'), Os::Windows);
    windows
        .hub
        .store(&[NewResult {
            key: outdated.key,
            outcome: Outcome::Pass,
            machine: windows.machine.clone(),
            record: RecordId(1),
            output: None,
            cost: ignite_core::protocol::hub::ResultCost::default(),
            areas: Vec::new(),
        }])
        .unwrap();

    std::thread::scope(|scope| {
        let (mac_phases, release_mac) = hold_ordinary_listing(&world.work);
        let mac_round = scope.spawn(|| world.verifier().verify(&mac, &tip).unwrap());
        wait_for_ordinary_listing(&mac_phases);
        assert_eq!(run_order(&world), [vec!["vet:a"]]);
        assert!(
            branches_pushed(&world).is_empty(),
            "the Windows build has no exact pass"
        );

        let (windows_phases, release_windows) = hold_ordinary_listing(&windows_work);
        let windows_round = scope.spawn(|| {
            TipVerifier::new(windows_work.clone(), "test")
                .verify(&windows, &tip)
                .unwrap()
        });
        wait_for_ordinary_listing(&windows_phases);
        assert_eq!(world.git.tip("green").unwrap(), tip);
        assert_eq!(world.master(), previous);
        assert_eq!(windows_work.runs()[0].2, [name("vet:a")]);

        release_mac.send(()).unwrap();
        assert_eq!(
            mac_round.join().unwrap(),
            TipOutcome::Waiting {
                checks: vec![name("unit:b")],
                unrunnable: Vec::new()
            }
        );
        assert_eq!(world.master(), previous);

        release_windows.send(()).unwrap();
        assert_eq!(windows_round.join().unwrap(), TipOutcome::Verified);
        assert_eq!(world.master(), tip);
        assert!(mac_phases.try_recv().is_err());
        assert!(windows_phases.try_recv().is_err());
    });

    let keys = [
        mac_build.key,
        windows_build.key,
        mac_test.key,
        windows_test.key,
    ];
    assert!(
        mac.hub
            .lookup(&keys)
            .unwrap()
            .iter()
            .all(|hit| { hit.as_ref().is_some_and(|hit| hit.outcome == Outcome::Pass) })
    );
    assert_eq!(run_order(&world), [vec!["vet:a"], vec!["unit:b"]]);
    assert_eq!(windows_work.runs().len(), 2);
    server.stop();
}

#[test]
fn master_still_honors_coverage_between_build_and_other_families() {
    let (world, tip) = world();
    let (server, database, mut ctx) = running_hub(&world, "green-covered");
    ctx.config.repository.green = Some("green".to_owned());
    select(&ctx, &tip);
    world
        .work
        .set_checks(&tip, vec![vet("a", 'a'), check("a", 'b')]);
    world.work.set_outcome(&tip, &name("vet:a"), Outcome::Fail);
    world
        .work
        .covered_by
        .lock()
        .unwrap()
        .insert("vet".to_owned(), "unit".to_owned());

    assert_eq!(
        world.verifier().verify(&ctx, &tip).unwrap(),
        TipOutcome::Verified
    );
    assert_eq!(branches_pushed(&world), [("master".to_owned(), tip)]);
    assert_eq!(run_order(&world), [vec!["vet:a"], vec!["unit:a"]]);
    assert!(ctx.hub.open_regressions().unwrap().is_empty());
    server.stop();
    let db = ignite_store::hub::open(&database).unwrap();
    assert_eq!(
        ignite_store::hub::tips::latest_failure(&db, Timestamp::EPOCH).unwrap(),
        None
    );
}

#[test]
fn a_build_run_must_pass_for_the_inputs_the_round_listed() {
    let build = vet("a", 'a');
    let mut pass = verdict(&build, Outcome::Pass, None);
    pass.input_hash = Some(vet("a", 'b').key.hash);
    let mut outcomes = HashMap::new();

    settle_runs(&[build], vec![pass], &mut outcomes, &mut HashSet::new());

    assert!(outcomes.is_empty());
}
