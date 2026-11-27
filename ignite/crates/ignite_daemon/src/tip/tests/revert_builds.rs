//! An automatic revert must leave a tree that builds. Tip verification once blamed a commit
//! that added a function for a failing test suite and reverted only that commit, while a later
//! commit already called the function: the revert passed the tests, but `staging` no longer
//! compiled, and `master` stayed stuck until someone reverted the later commit by hand.
//!
//! Each test here has three landings on `staging`: `tk.40` adds a function, `tk.41` calls it,
//! and `tk.42` changes something unrelated. The test suite `unit:a` fails at the tip and is
//! blamed on `tk.40` once `master` has stalled, and the build check `vet:all` reads every file.

use ignite_core::traits::RevertOutcome;

use super::*;

/// The commit that undoes `tk.40` alone, and the one that undoes `tk.40` and `tk.41`.
const ALONE: &str = "eeee000";
const WITH_CALLER: &str = "ffff000";

/// The build check every landing's files are inputs of.
fn build() -> TipCheck {
    let mut check = check("all", 'f');
    check.check = name("vet:all");
    check.key.check = check.check.clone();
    check.family = "vet".to_owned();
    check.inputs = vec!["src".to_owned()];
    check.kind = TipCheckKind::Build;
    check
}

/// The world described at the top of this file, with `master` stalled long enough for a revert
/// of a check outside the build group, and the reverts scripted as [`ALONE`] then
/// [`WITH_CALLER`]. `calls` says whether `tk.41`'s added lines call the function `tk.40` added.
fn three_landings(calls: bool) -> (World, Context, Commit, TipCheck) {
    let world = World::new();
    let tip = commit("dddd000");
    world.land(&tip);
    for (id, sha, path, subject, added) in [
        (
            "tk.40",
            "bbbb000",
            "src/a/lib.rs",
            "refactor: add short filter builders",
            "pub fn nonland() -> Filter {",
        ),
        (
            "tk.41",
            "cccc000",
            "src/b/card.rs",
            "feat: a card that uses the builders",
            if calls {
                "    .filter(nonland())"
            } else {
                "    .filter(land())"
            },
        ),
        (
            "tk.42",
            "dddd000",
            "src/c/card.rs",
            "feat: an unrelated card",
            "    .filter(land())",
        ),
    ] {
        world.work.add_landing(id, &[sha], &[path]);
        world.git.set_subject(&commit(sha), subject);
        world.git.set_line_changes(&[commit(sha)], &[added], &[]);
    }
    // The tree before `tk.40` already had `land` and `Filter`, but not `nonland`.
    world
        .git
        .set_words_before(&commit("bbbb000"), &["land", "Filter", "pub"]);
    let failing = check("a", 'a');
    world.work.set_checks(&tip, vec![failing.clone(), build()]);
    world.work.set_outcome(&tip, &failing.check, Outcome::Fail);
    // Without `tk.40`, `tk.41`'s call does not compile.
    world
        .work
        .set_outcome(&commit(ALONE), &build().check, Outcome::Fail);
    for sha in [ALONE, WITH_CALLER] {
        world.git.script_revert(RevertOutcome::Clean {
            commit: commit(sha),
        });
    }
    let mut ctx = world.ctx("m-1");
    ctx.config.repository.green = Some("green".to_owned());
    ctx.config.revert.stall_minutes = Some(120);
    world.clock.advance(121.0 * 60.0);
    (world, ctx, tip, failing)
}

fn staging_pushes(world: &World) -> Vec<Commit> {
    world
        .git
        .pushes()
        .into_iter()
        .filter(|(_, _, branch)| branch == "staging")
        .map(|(_, at, _)| at)
        .collect()
}

fn verify(world: &World, ctx: &Context, tip: &Commit) {
    world
        .verifier()
        .pausing_with(Arc::new(|_| {}))
        .verify(ctx, tip)
        .unwrap();
}

#[test]
fn a_later_landing_that_calls_what_the_culprit_added_is_reverted_with_it() {
    let (world, ctx, tip, failing) = three_landings(true);

    verify(&world, &ctx, &tip);

    assert!(world.hub.regressions().is_empty());
    assert_eq!(staging_pushes(&world), vec![commit(WITH_CALLER)]);
    // The test suite passed at the first revert, the build failed there, and both passed at
    // the revert that also undid the caller.
    let runs: Vec<(Commit, Vec<CheckName>)> = world
        .work
        .runs()
        .into_iter()
        .filter(|run| run.1 != tip)
        .map(|run| (run.1, run.2))
        .collect();
    assert_eq!(
        runs,
        vec![
            (commit(ALONE), vec![failing.check.clone()]),
            (commit(ALONE), vec![build().check]),
            (commit(WITH_CALLER), vec![failing.check.clone()]),
            (commit(WITH_CALLER), vec![build().check]),
        ]
    );
    let reverted = world.hub.reverted_landings();
    let tickets: Vec<Option<TicketId>> = reverted.iter().map(|r| r.ticket.clone()).collect();
    assert_eq!(tickets, vec![Some(ticket("tk.40")), Some(ticket("tk.41"))]);
    assert!(reverted.iter().all(|r| r.revert == commit(WITH_CALLER)));
    assert_eq!(reverted[1].landed, vec![commit("cccc000")]);
    assert!(
        reverted[1].output.contains("undone together with tk.40"),
        "{}",
        reverted[1].output
    );
}

#[test]
fn a_revert_that_still_does_not_build_is_reported_not_pushed() {
    let (world, ctx, tip, failing) = three_landings(true);
    world
        .work
        .set_outcome(&commit(WITH_CALLER), &build().check, Outcome::Fail);

    verify(&world, &ctx, &tip);

    assert!(staging_pushes(&world).is_empty());
    assert!(world.hub.reverted_landings().is_empty());
    let regressions = world.hub.regressions();
    assert_eq!(regressions.len(), 1);
    let report = &regressions[0];
    assert_eq!(report.check, failing.check);
    assert_eq!(report.caused_by, Some(ticket("tk.40")));
    assert!(
        report.output.starts_with(
            "Ignite did not revert tk.40 (bbbb000) automatically. The check unit:a passes with \
             it undone, but vet:all then fails at the revert ffff000. That revert also undid \
             these later landings, which use what it added: tk.41 (cccc000)."
        ),
        "{}",
        report.output
    );
    assert!(!report.output.contains("tk.42"));
}

#[test]
fn a_revert_that_does_not_build_with_no_later_caller_is_reported() {
    let (world, ctx, tip, _) = three_landings(false);

    verify(&world, &ctx, &tip);

    assert!(staging_pushes(&world).is_empty());
    let regressions = world.hub.regressions();
    assert_eq!(regressions.len(), 1);
    assert!(
        regressions[0]
            .output
            .contains("vet:all then fails at the revert eeee000. No later landing uses"),
        "{}",
        regressions[0].output
    );
}

#[test]
fn a_pushed_revert_advances_a_pre_revert_candidate_to_the_revert() {
    use ignite_core::tipround::{TipCandidate, TipCandidateUpdate};

    let (world, ctx, tip, _) = three_landings(true);
    world.git.set_not_ancestor(&tip, &world.master());
    let pre_revert = commit("deed000");
    // While the round is running its checks at `tip`, another machine selects a pre-revert
    // commit as the shared candidate.
    let hub = world.hub.clone();
    let selected = pre_revert.clone();
    world.work.before_batches.lock().unwrap().insert(
        "unit".to_owned(),
        Arc::new(move || {
            let expected = hub.tip_candidate().unwrap();
            assert!(
                hub.update_tip_candidate(&TipCandidateUpdate {
                    expected,
                    replacement: TipCandidate {
                        commit: selected.clone(),
                        retry_on_new_tip: false,
                    },
                })
                .unwrap()
            );
        }),
    );

    verify(&world, &ctx, &tip);

    assert_eq!(
        world.hub.tip_candidate().unwrap(),
        Some(TipCandidate {
            commit: commit(WITH_CALLER),
            retry_on_new_tip: false,
        })
    );
    assert_eq!(
        world.verifier().next_target(&ctx).unwrap(),
        Some(commit(WITH_CALLER))
    );
}

#[test]
fn a_proven_fix_on_staging_advances_an_older_candidate_to_that_tip() {
    use ignite_core::tipround::{TipCandidate, TipCandidateUpdate};

    let world = World::new();
    let ctx = world.ctx("m-1");
    let tip = commit("bbbb000");
    let intermediate = commit("cccc000");
    let fixed = commit("dddd000");
    world.land(&tip);
    world.git.set_not_ancestor(&tip, &world.master());
    let failing = check("a", 'a');
    world.work.set_checks(&tip, vec![failing.clone()]);
    world.work.set_outcome(&tip, &failing.check, Outcome::Fail);
    world.git.set_changed(&tip, &fixed, &["src/a"]);
    world
        .work
        .set_outcome(&fixed, &failing.check, Outcome::Pass);
    // While the round is running at `tip`, another machine selects `intermediate` and a
    // newer landing `fixed` arrives on `staging`.
    let hub = world.hub.clone();
    let git = world.git.clone();
    let selected = intermediate.clone();
    let newest = fixed.clone();
    world.work.before_batches.lock().unwrap().insert(
        "unit".to_owned(),
        Arc::new(move || {
            git.set_tip("staging", newest.clone());
            let expected = hub.tip_candidate().unwrap();
            assert!(
                hub.update_tip_candidate(&TipCandidateUpdate {
                    expected,
                    replacement: TipCandidate {
                        commit: selected.clone(),
                        retry_on_new_tip: false,
                    },
                })
                .unwrap()
            );
        }),
    );

    verify(&world, &ctx, &tip);

    assert!(world.hub.regressions().is_empty());
    assert_eq!(
        world.hub.tip_candidate().unwrap(),
        Some(TipCandidate {
            commit: fixed,
            retry_on_new_tip: false,
        })
    );
}

#[test]
fn advance_to_keeps_a_candidate_at_or_ahead_of_the_target() {
    use ignite_core::tipround::{TipCandidate, TipCandidateUpdate};

    let world = World::new();
    let ctx = world.ctx("m-1");
    let target = commit("bbbb000");
    let ahead = commit("cccc000");

    // Already at `target` with `retry_on_new_tip = true`: do not clear the retry flag.
    world
        .hub
        .update_tip_candidate(&TipCandidateUpdate {
            expected: None,
            replacement: TipCandidate {
                commit: target.clone(),
                retry_on_new_tip: true,
            },
        })
        .unwrap();
    super::super::candidate::advance_to(&ctx, &target).unwrap();
    assert_eq!(
        world.hub.tip_candidate().unwrap(),
        Some(TipCandidate {
            commit: target.clone(),
            retry_on_new_tip: true,
        })
    );

    // Already past `target` (not an ancestor of `target`): do not step backwards.
    world
        .hub
        .update_tip_candidate(&TipCandidateUpdate {
            expected: Some(TipCandidate {
                commit: target.clone(),
                retry_on_new_tip: true,
            }),
            replacement: TipCandidate {
                commit: ahead.clone(),
                retry_on_new_tip: false,
            },
        })
        .unwrap();
    world.git.set_not_ancestor(&ahead, &target);
    super::super::candidate::advance_to(&ctx, &target).unwrap();
    assert_eq!(
        world.hub.tip_candidate().unwrap(),
        Some(TipCandidate {
            commit: ahead,
            retry_on_new_tip: false,
        })
    );
}
