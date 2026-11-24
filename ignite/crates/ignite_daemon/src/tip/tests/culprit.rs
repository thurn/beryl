//! Culprit searches over commits the hub recorded no landing for, and the revert of a landing
//! that broke the build or held back master.

use std::sync::Mutex;

use ignite_core::protocol::hub::Reverted;
use ignite_core::protocol::tickets::{LeaseOp, LeaseReply};
use ignite_core::traits::RevertOutcome;

use super::*;

/// The first regression tip verification filed named the tip as its first bad commit: none of
/// the 60 commits since the last verified commit had a landing recorded at the hub, so they
/// were grouped into one landing, the only candidate, and blamed with nothing run. The culprit
/// was one of several commits in the middle that changed the check's inputs.
#[test]
fn commits_no_landing_recorded_are_searched_one_by_one() {
    let world = World::new();
    let ctx = world.ctx("m-1");
    for (sha, paths) in [
        ("bbbb000", &["docs/x"][..]),
        ("cccc000", &["src/a/lib.rs"]),
        ("dddd000", &["docs/y"]),
        ("eeee000", &["src/a/lib.rs"]),
        ("ffff000", &["docs/z"]),
    ] {
        world.work.add_commit(sha, paths);
    }
    let tip = commit("ffff000");
    world.land(&tip);
    let failing = check("a", 'a');
    world.work.set_checks(&tip, vec![failing.clone()]);
    world.work.set_outcome(&tip, &failing.check, Outcome::Fail);
    world
        .work
        .set_outcome(&commit("cccc000"), &failing.check, Outcome::Pass);
    world
        .work
        .set_outcome(&commit("eeee000"), &failing.check, Outcome::Fail);

    world.verifier().verify(&ctx, &tip).unwrap();

    let report = &world.hub.regressions()[0];
    assert_eq!(report.first_bad, commit("eeee000"));
    assert_eq!(report.last_good, commit("dddd000"));
    assert_eq!(report.caused_by, None);
    assert_eq!(report.search, Some(CulpritSearch::Bisected { runs: 1 }));
    // The tip's own run, then one run of the search at the first candidate.
    let runs: Vec<Commit> = world.work.runs().into_iter().map(|run| run.1).collect();
    assert_eq!(runs, vec![tip, commit("cccc000")]);
}

/// A recorded landing keeps its commits together, and unrecorded commits beside it stand
/// alone.
#[test]
fn recorded_landings_stay_whole_beside_unrecorded_commits() {
    let world = World::new();
    let ctx = world.ctx("m-1");
    world.record_landing("tk.1", &["cccc000", "dddd000"]);
    for sha in ["bbbb000", "cccc000", "dddd000", "eeee000", "ffff000"] {
        world.work.add_commit(sha, &["docs/x"]);
    }

    let landings = world
        .work
        .landings(&ctx, &commit("aaaa000"), &commit("ffff000"))
        .unwrap();

    let grouped: Vec<(Option<TicketId>, Vec<Commit>)> = landings
        .into_iter()
        .map(|landing| (landing.ticket, landing.commits))
        .collect();
    assert_eq!(
        grouped,
        vec![
            (None, vec![commit("bbbb000")]),
            (
                Some(ticket("tk.1")),
                vec![commit("cccc000"), commit("dddd000")]
            ),
            (None, vec![commit("eeee000")]),
            (None, vec![commit("ffff000")]),
        ]
    );
}

/// A build check, `vet:<text>`, reading `src/<text>`, in the family `vet` that [`broken`]
/// declares `builds = true`.
fn vet(text: &str, hash: char) -> TipCheck {
    let mut check = check(text, hash);
    check.check = name(&format!("vet:{text}"));
    check.key.check = check.check.clone();
    check.family = "vet".to_owned();
    check.kind = TipCheckKind::Build;
    check
}

/// A world with `staging` at `bbbb000` and `[revert] broken_build` set as given, where the
/// landing of `tk.40` (`bbbb000`, changing `paths`) makes `failing` fail at the tip.
fn broken(failing: &TipCheck, paths: &[&str], revert: bool) -> (World, Context, Commit) {
    let world = World::new();
    let tip = commit("bbbb000");
    world.land(&tip);
    world.work.add_landing("tk.40", &["bbbb000"], paths);
    world.work.set_checks(&tip, vec![failing.clone()]);
    world.work.set_outcome(&tip, &failing.check, Outcome::Fail);
    world
        .git
        .set_subject(&tip, "feat: a change that does not compile");
    let mut ctx = world.ctx("m-1");
    ctx.config.repository.green = Some("green".to_owned());
    ctx.config.revert.broken_build = revert;
    (world, ctx, tip)
}

/// Makes the next revert clean, as `cccc000`. Without one, the fake's revert is the tip itself,
/// where the check fails.
fn clean_revert(world: &World) {
    world.git.script_revert(RevertOutcome::Clean {
        commit: commit("cccc000"),
    });
}

fn reverting(world: &World) -> TipVerifier {
    world.verifier().pausing_with(Arc::new(|_| {}))
}

/// The commits pushed to `staging`.
fn staging_pushes(world: &World) -> Vec<Commit> {
    world
        .git
        .pushes()
        .into_iter()
        .filter(|(_, _, branch)| branch == "staging")
        .map(|(_, at, _)| at)
        .collect()
}

/// Asserts the round reported the failure as a regression and reverted nothing.
fn reported_not_reverted(world: &World) {
    assert_eq!(world.hub.regressions().len(), 1);
    assert!(staging_pushes(world).is_empty());
    assert!(world.hub.reverted_landings().is_empty());
}

#[test]
fn a_build_check_broken_by_one_landing_is_reverted() {
    let failing = vet("a", 'a');
    let (world, ctx, tip) = broken(&failing, &["src/a/lib.rs"], true);
    clean_revert(&world);

    let outcome = reverting(&world).verify(&ctx, &tip).unwrap();

    assert_eq!(
        outcome,
        TipOutcome::Failed {
            promoted: None,
            checks: vec![failing.check.clone()]
        }
    );
    assert!(world.hub.regressions().is_empty());
    assert_eq!(staging_pushes(&world), vec![commit("cccc000")]);
    // The check ran at the tip again, ignoring the cache, then at the revert before it was
    // pushed.
    let runs: Vec<(Commit, bool)> = world
        .work
        .runs()
        .into_iter()
        .map(|run| (run.1, run.3))
        .collect();
    assert_eq!(
        runs,
        vec![
            (tip.clone(), false),
            (tip, true),
            (commit("cccc000"), false)
        ]
    );
    let reverted = world.hub.reverted_landings();
    assert_eq!(reverted.len(), 1);
    assert_eq!(reverted[0].ticket, Some(ticket("tk.40")));
    assert_eq!(reverted[0].landed, vec![commit("bbbb000")]);
    assert_eq!(reverted[0].revert, commit("cccc000"));
    assert_eq!(reverted[0].checks, vec![failing.check]);
    assert_eq!(reverted[0].search, CulpritSearch::OneCandidate);
    assert_eq!(reverted[0].machine, "jarvis");
    // The lease was given back after the push.
    let other = LeaseOp::Acquire {
        holder: "m-2:tk.41".to_owned(),
        ttl_s: 60,
        payload: None,
        no_queue: true,
    };
    assert!(matches!(
        world.hub.lease("staging", other).unwrap(),
        LeaseReply::Held(_)
    ));
}

/// The revert is pushed only under the `staging` lease: while a landing holds it, the round
/// waits, then reports the regression instead.
#[test]
fn a_lease_another_landing_keeps_reports_a_regression() {
    let failing = vet("a", 'a');
    let (world, ctx, tip) = broken(&failing, &["src/a/lib.rs"], true);
    clean_revert(&world);
    let landing = LeaseOp::Acquire {
        holder: "m-2:tk.41".to_owned(),
        ttl_s: 60,
        payload: None,
        no_queue: true,
    };
    world.hub.lease("staging", landing).unwrap();
    let waits = Arc::new(Mutex::new(0));
    let counted = waits.clone();

    world
        .verifier()
        .pausing_with(Arc::new(move |_| *counted.lock().unwrap() += 1))
        .verify(&ctx, &tip)
        .unwrap();

    reported_not_reverted(&world);
    assert!(*waits.lock().unwrap() > 0);
}

#[test]
fn a_failing_check_outside_the_build_group_is_reported() {
    let failing = check("a", 'a');
    let (world, ctx, tip) = broken(&failing, &["src/a/lib.rs"], true);
    clean_revert(&world);

    reverting(&world).verify(&ctx, &tip).unwrap();

    reported_not_reverted(&world);
    assert_eq!(world.hub.regressions()[0].caused_by, Some(ticket("tk.40")));
}

#[test]
fn a_required_failure_can_revert_at_the_stall_threshold_after_being_reported() {
    let failing = check("a", 'a');
    let (world, mut ctx, tip) = broken(&failing, &["src/a/lib.rs"], false);
    ctx.config.revert.stall_minutes = Some(120);
    // Master has been quiet for longer than two hours, but the landing has waited 119 minutes.
    world
        .git
        .set_committed_at(&tip, Timestamp::EPOCH.plus_seconds(3_600.0));
    world.clock.advance(3_600.0 + 119.0 * 60.0);
    clean_revert(&world);
    let mut verifier = reverting(&world);

    verifier.round_at(&ctx, &tip, Seq(0)).unwrap();

    reported_not_reverted(&world);
    assert_eq!(world.hub.regressions()[0].caused_by, Some(ticket("tk.40")));
    assert!(!verifier.due(&ctx, &tip));

    world.clock.advance(60.0);
    assert!(verifier.due(&ctx, &tip));
    verifier.round_at(&ctx, &tip, Seq(0)).unwrap();

    assert_eq!(staging_pushes(&world), [commit("cccc000")]);
    let reports = world.hub.reverted_landings();
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].ticket, Some(ticket("tk.40")));
    assert_eq!(reports[0].checks, std::slice::from_ref(&failing.check));
    assert_eq!(reports[0].search, CulpritSearch::OneCandidate);
    assert_eq!(world.hub.regressions().len(), 1, "reported once");
    assert!(!verifier.due(&ctx, &tip));
    let runs = world.work.runs();
    let (_, at, checks, rerun) = &runs[runs.len() - 2];
    assert_eq!(at, &tip);
    assert_eq!(checks, &[failing.check]);
    assert!(*rerun);
    assert_eq!(runs.last().unwrap().1, commit("cccc000"));
}

#[test]
fn a_required_failure_with_multiple_suspects_is_reported_during_a_stall() {
    let failing = check("a", 'a');
    let (world, mut ctx, tip) = broken(&failing, &["src/a/lib.rs"], false);
    ctx.config.revert.stall_minutes = Some(120);
    world.clock.advance(121.0 * 60.0);
    clean_revert(&world);
    world.work.landings.lock().unwrap().insert(
        0,
        TipLanding {
            ticket: Some(ticket("tk.39")),
            commits: vec![commit("abab000")],
            paths: vec!["src/a/main.rs".to_owned()],
        },
    );
    world
        .work
        .set_outcome(&commit("abab000"), &failing.check, Outcome::Incomplete);

    reverting(&world).verify(&ctx, &tip).unwrap();

    reported_not_reverted(&world);
    let reports = world.hub.regressions();
    assert_eq!(reports[0].suspects, [ticket("tk.39"), ticket("tk.40")]);
    assert!(matches!(
        reports[0].search,
        Some(CulpritSearch::EndedEarly { .. })
    ));
}

#[test]
fn an_incomplete_rerun_cannot_confirm_a_revert_during_a_stall() {
    let failing = check("a", 'a');
    let (world, mut ctx, tip) = broken(&failing, &["src/a/lib.rs"], false);
    ctx.config.revert.stall_minutes = Some(120);
    world.clock.advance(121.0 * 60.0);
    clean_revert(&world);
    let work = world.work.clone();
    let (at, check) = (tip.clone(), failing.check.clone());
    on_second_run(
        &world,
        &tip,
        Arc::new(move || work.set_outcome(&at, &check, Outcome::Incomplete)),
    );

    reverting(&world).verify(&ctx, &tip).unwrap();

    reported_not_reverted(&world);
}

#[test]
fn a_landing_that_changed_ignites_source_is_reported() {
    let failing = vet("a", 'a');
    let paths = ["src/a/lib.rs", "ignite/crates/ignite_core/src/lib.rs"];
    for failing in [failing, check("a", 'a')] {
        let (world, mut ctx, tip) = broken(&failing, &paths, true);
        ctx.config.revert.stall_minutes = Some(120);
        world.clock.advance(121.0 * 60.0);
        clean_revert(&world);

        reverting(&world).verify(&ctx, &tip).unwrap();

        reported_not_reverted(&world);
    }
}

#[test]
fn a_revert_that_conflicts_is_reported() {
    let failing = vet("a", 'a');
    let (world, ctx, tip) = broken(&failing, &["src/a/lib.rs"], true);
    world.git.script_revert(RevertOutcome::Conflict {
        files: vec!["src/a/lib.rs".to_owned()],
    });

    reverting(&world).verify(&ctx, &tip).unwrap();

    reported_not_reverted(&world);
}

#[test]
fn a_build_check_that_still_fails_at_the_revert_is_reported() {
    let failing = vet("a", 'a');
    let (world, ctx, tip) = broken(&failing, &["src/a/lib.rs"], true);
    clean_revert(&world);
    world
        .work
        .set_outcome(&commit("cccc000"), &failing.check, Outcome::Fail);

    reverting(&world).verify(&ctx, &tip).unwrap();

    reported_not_reverted(&world);
}

/// A search that ran out of answers names a range of landings, and no one of them is known to
/// be the culprit.
#[test]
fn a_search_that_ends_at_a_range_is_reported() {
    let failing = vet("a", 'a');
    let (world, ctx, tip) = broken(&failing, &["src/a/lib.rs"], true);
    clean_revert(&world);
    world.work.landings.lock().unwrap().insert(
        0,
        TipLanding {
            ticket: Some(ticket("tk.39")),
            commits: vec![commit("abab000")],
            paths: vec!["src/a/main.rs".to_owned()],
        },
    );
    world
        .work
        .set_outcome(&commit("abab000"), &failing.check, Outcome::Incomplete);

    reverting(&world).verify(&ctx, &tip).unwrap();

    reported_not_reverted(&world);
    assert!(matches!(
        world.hub.regressions()[0].search,
        Some(CulpritSearch::EndedEarly { .. })
    ));
}

#[test]
fn a_landing_of_round_reverts_is_never_reverted() {
    let failing = vet("a", 'a');
    let (world, ctx, tip) = broken(&failing, &["src/a/lib.rs"], true);
    clean_revert(&world);
    world.work.landings.lock().unwrap()[0].ticket = None;
    world
        .git
        .set_subject(&tip, "revert: feat: a change that does not compile");

    reverting(&world).verify(&ctx, &tip).unwrap();

    reported_not_reverted(&world);
}

/// Four build checks, each broken by a landing of its own: the round reverts three, and
/// reports the fourth.
#[test]
fn the_fourth_culprit_of_a_round_is_reported() {
    let world = World::new();
    let tip = commit("bbbb004");
    world.land(&tip);
    let mut checks = Vec::new();
    for (i, text) in ["a", "b", "c", "d"].into_iter().enumerate() {
        let failing = vet(text, char::from(b'a' + u8::try_from(i).unwrap()));
        let sha = format!("bbbb00{}", i + 1);
        world.work.add_landing(
            &format!("tk.{}", 40 + i),
            &[sha.as_str()],
            &[format!("src/{text}/lib.rs").as_str()],
        );
        world.work.set_outcome(&tip, &failing.check, Outcome::Fail);
        world.git.script_revert(RevertOutcome::Clean {
            commit: commit(&format!("cccc00{}", i + 1)),
        });
        checks.push(failing);
    }
    world.work.set_checks(&tip, checks);
    let mut ctx = world.ctx("m-1");
    ctx.config.repository.green = Some("green".to_owned());
    ctx.config.revert.broken_build = true;

    reverting(&world).verify(&ctx, &tip).unwrap();

    assert_eq!(
        staging_pushes(&world),
        vec![commit("cccc001"), commit("cccc002"), commit("cccc003")]
    );
    assert_eq!(world.hub.reverted_landings().len(), 3);
    let regressions = world.hub.regressions();
    assert_eq!(regressions.len(), 1);
    assert_eq!(regressions[0].check, name("vet:d"));
}

/// A landing that moves the tip while the revert's check runs: the revert is built again on
/// the new tip, and that one is pushed.
#[test]
fn a_tip_that_moves_before_the_push_gets_a_rebuilt_revert() {
    let failing = vet("a", 'a');
    let (world, ctx, tip) = broken(&failing, &["src/a/lib.rs"], true);
    clean_revert(&world);
    world.git.script_revert(RevertOutcome::Clean {
        commit: commit("eeee000"),
    });
    let git = world.git.clone();
    let moved: fake::BeforeBatches = Arc::new(move || git.set_tip("staging", commit("dddd000")));
    world
        .work
        .on_run
        .lock()
        .unwrap()
        .insert(commit("cccc000"), moved);

    reverting(&world).verify(&ctx, &tip).unwrap();

    assert!(world.hub.regressions().is_empty());
    assert_eq!(staging_pushes(&world), vec![commit("eeee000")]);
    assert_eq!(world.hub.reverted_landings()[0].revert, commit("eeee000"));
}

#[test]
fn without_broken_build_a_broken_build_is_reported() {
    let failing = vet("a", 'a');
    let (world, ctx, tip) = broken(&failing, &["src/a/lib.rs"], false);
    clean_revert(&world);

    reverting(&world).verify(&ctx, &tip).unwrap();

    reported_not_reverted(&world);
    assert_eq!(world.hub.regressions()[0].caused_by, Some(ticket("tk.40")));
}

/// A revert the hub did not take is told again at the start of the next round.
#[test]
fn a_revert_the_hub_missed_is_sent_with_the_next_round() {
    let failing = vet("a", 'a');
    let (world, ctx, tip) = broken(&failing, &["src/a/lib.rs"], false);
    let missed = Reverted {
        ticket: Some(ticket("tk.39")),
        landed: vec![commit("abab000")],
        revert: commit("acac000"),
        checks: vec![failing.check.clone()],
        output: String::new(),
        search: CulpritSearch::OneCandidate,
        machine: "jarvis".to_owned(),
    };
    let mut verifier = reverting(&world);
    verifier.unsent_reverts.push(missed.clone());

    verifier.verify(&ctx, &tip).unwrap();

    assert_eq!(world.hub.reverted_landings(), vec![missed]);
    assert!(verifier.unsent_reverts.is_empty());
}

/// Arms `hook` during the first run at `at` so it fires when the second run starts.
pub(super) fn on_second_run(world: &World, at: &Commit, hook: fake::BeforeBatches) {
    let work = world.work.clone();
    let commit = at.clone();
    let arm: fake::BeforeBatches = Arc::new(move || {
        work.on_run
            .lock()
            .unwrap()
            .insert(commit.clone(), hook.clone());
    });
    world.work.on_run.lock().unwrap().insert(at.clone(), arm);
}

/// A failure that does not happen again when the check runs at the tip once more was a fault
/// of that run, and reopens no ticket.
#[test]
fn a_build_failure_that_does_not_repeat_is_reported() {
    let failing = vet("a", 'a');
    let (world, ctx, tip) = broken(&failing, &["src/a/lib.rs"], true);
    clean_revert(&world);
    let work = world.work.clone();
    let (at, check) = (tip.clone(), failing.check.clone());
    on_second_run(
        &world,
        &tip,
        Arc::new(move || work.set_outcome(&at, &check, Outcome::Pass)),
    );

    reverting(&world).verify(&ctx, &tip).unwrap();

    reported_not_reverted(&world);
}

/// A tip that moved during the search with a change to what the check reads may already hold
/// a fix, or another machine's revert: nothing is reverted, and the next round runs the check
/// at the new tip first.
#[test]
fn a_tip_that_changed_the_checks_inputs_is_left_to_the_next_round() {
    let failing = vet("a", 'a');
    let (world, ctx, tip) = broken(&failing, &["src/a/lib.rs"], true);
    clean_revert(&world);
    let newer = commit("dddd000");
    world.git.set_changed(&tip, &newer, &["src/a/lib.rs"]);
    let git = world.git.clone();
    let moved = newer.clone();
    let land: fake::BeforeBatches = Arc::new(move || git.set_tip("staging", moved.clone()));
    // A fix lands after the search, while the check runs at the tip a second time.
    on_second_run(&world, &tip, land);

    reverting(&world).verify(&ctx, &tip).unwrap();

    assert!(world.hub.regressions().is_empty());
    assert!(staging_pushes(&world).is_empty());
    assert!(world.hub.reverted_landings().is_empty());
}

/// On a busy day every round's tip is overtaken, with a change to what a failing check reads,
/// before the round ends. Leaving each such failure to the next round never reported it, and
/// `master` did not move for hours. The second round that finds the check failing reports it.
#[test]
fn a_check_left_to_the_next_round_is_reported_when_it_fails_again() {
    let world = World::new();
    let ctx = world.ctx("m-1");
    let failing = check("a", 'a');
    let mut verifier = world.verifier();
    for (at, newer) in [("bbbb000", "cccc000"), ("cccc000", "dddd000")] {
        world.work.add_commit(newer, &["src/a/lib.rs"]);
        let (at, newer) = (commit(at), commit(newer));
        world.git.set_changed(&at, &newer, &["src/a/lib.rs"]);
        world.land(&newer);
        world.work.set_checks(&at, vec![failing.clone()]);
        world.work.set_outcome(&at, &failing.check, Outcome::Fail);

        verifier.verify(&ctx, &at).unwrap();
    }

    let regressions = world.hub.regressions();
    assert_eq!(regressions.len(), 1);
    assert_eq!(regressions[0].check, failing.check);
}

/// Two build checks blamed on one landing share its revert, but only when each passes there.
#[test]
fn a_second_check_on_a_reverted_landing_must_pass_at_the_revert() {
    let failing = vet("a", 'a');
    let (world, ctx, tip) = broken(&failing, &["src/a/lib.rs", "src/b/lib.rs"], true);
    clean_revert(&world);
    let other = vet("b", 'b');
    world
        .work
        .set_checks(&tip, vec![failing.clone(), other.clone()]);
    world.work.set_outcome(&tip, &other.check, Outcome::Fail);
    world
        .work
        .set_outcome(&commit("cccc000"), &other.check, Outcome::Fail);

    reverting(&world).verify(&ctx, &tip).unwrap();

    assert_eq!(staging_pushes(&world), vec![commit("cccc000")]);
    assert_eq!(world.hub.reverted_landings()[0].checks, vec![failing.check]);
    let regressions = world.hub.regressions();
    assert_eq!(regressions.len(), 1);
    assert_eq!(regressions[0].check, other.check);
}
