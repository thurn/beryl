//! The green branch: a round runs its build checks first and moves `green` once they pass,
//! whatever the rest of the round finds.

use super::*;

/// `unit:<text>`, an ordinary check, and `vet:<text>`, a build check. `vet` sorts after `unit`,
/// so without a build step its family would run second.
fn vet(text: &str, hash: char) -> TipCheck {
    let mut check = check(text, hash);
    check.check = name(&format!("vet:{text}"));
    check.key.check = check.check.clone();
    check.family = "vet".to_owned();
    check
}

/// A world whose `vet` family is declared `builds = true`, with `staging` at `bbbb000` listing
/// `vet:a` and `unit:b`.
fn world() -> (World, Commit) {
    let world = World::new();
    world.work.builds.lock().unwrap().insert("vet".to_owned());
    let tip = commit("bbbb000");
    world.land(&tip);
    world
        .work
        .set_checks(&tip, vec![vet("a", 'a'), check("b", 'b')]);
    (world, tip)
}

fn green_ctx(world: &World) -> Context {
    let mut ctx = world.ctx("m-1");
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
            checks: vec![name("vet:a")]
        }
    );
    assert_eq!(world.git.tip("green").unwrap(), tip);
    assert_eq!(world.master(), tip);
}
