//! Culprit searches over commits the hub recorded no landing for.

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
