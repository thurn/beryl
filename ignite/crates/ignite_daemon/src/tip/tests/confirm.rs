//! How a cached failure affects the shared candidate while required work is still running.

use std::sync::{Barrier, mpsc};
use std::thread;

use ignite_core::protocol::hub::TipCheckFailures;
use ignite_store::hub::{self, tips};

use super::rounds::{connect_machine, in_family, running_hub, select};
use super::*;

#[test]
fn a_confirmed_cached_failure_releases_the_candidate_during_an_unrelated_batch() {
    cached_failure_during_batch(Some(Outcome::Fail));
}

#[test]
fn a_passing_confirmation_keeps_the_candidate_during_an_unrelated_batch() {
    cached_failure_during_batch(Some(Outcome::Pass));
}

#[test]
fn an_established_failure_releases_the_candidate_during_an_unrelated_batch() {
    cached_failure_during_batch(None);
}

fn cached_failure_during_batch(confirmation: Option<Outcome>) {
    let world = World::new();
    let (server, database, ctx) = running_hub(&world, "cached-failure");
    let peer = connect_machine(world.ctx("m-2"), &server, "peer", Os::Macos);
    let tip = commit("cccc000");
    let newer = commit("dddd000");
    let mut cached = in_family("a", "native");
    cached.key.platform = Some(Os::Macos);
    let unrelated = in_family("b", "other");
    let mut verifier = world.verifier();
    let previous_runner = if confirmation.is_some() {
        &peer
    } else {
        verifier.rechecked.insert(cached.key.clone());
        &ctx
    };
    cache_predecessor_failure(&world, previous_runner, &tip, &cached);
    world.land(&tip);
    select(&ctx, &tip);
    world
        .work
        .set_checks(&tip, vec![cached.clone(), unrelated.clone()]);
    if let Some(outcome) = confirmation {
        world.work.set_outcome(&tip, &cached.check, outcome);
    }
    let mut changed = cached.clone();
    changed.key.hash = InputHash::new("d".repeat(64)).unwrap();
    world.work.set_checks(&newer, vec![changed.clone()]);
    world
        .work
        .set_outcome(&newer, &changed.check, Outcome::Incomplete);
    world.git.set_changed(&tip, &newer, &["src/a"]);
    world.land(&newer);
    assert_eq!(
        world.verifier().next_target(&peer).unwrap(),
        Some(tip.clone())
    );

    let (release, entered, held) = hold_batch(&world, &tip, unrelated.family);
    let conn = hub::open(&database).unwrap();
    let (observed, selected, ran, outcome) = thread::scope(|scope| {
        let round = scope.spawn(|| {
            let outcome = verifier.verify(&ctx, &tip);
            let _ = entered.send(false);
            outcome
        });
        assert!(
            held.recv().unwrap(),
            "round finished before the unrelated batch ran"
        );
        let observed = tips::candidate_failures(&conn);
        let selected = world.verifier().next_target(&peer);
        let ran = world.work.runs();
        release.wait();
        (
            observed.unwrap(),
            selected.unwrap(),
            ran,
            round.join().unwrap().unwrap(),
        )
    });

    assert_eq!(
        ran.iter().any(|(_, at, checks, rerun)| at == &tip
            && checks == std::slice::from_ref(&cached.check)
            && *rerun),
        confirmation.is_some()
    );
    if confirmation == Some(Outcome::Pass) {
        assert_eq!(observed, None);
        assert_eq!(selected, Some(tip.clone()));
        assert_eq!(outcome, TipOutcome::Verified);
        assert_eq!(world.master(), tip);
    } else {
        assert_eq!(
            observed,
            Some(TipCheckFailures {
                commit: tip,
                checks: vec![cached.key]
            })
        );
        assert_eq!(selected, Some(newer.clone()));
        assert_eq!(outcome, TipOutcome::Superseded { by: newer.clone() });
        assert_eq!(
            world.verifier().verify(&peer, &newer).unwrap(),
            TipOutcome::Waiting {
                checks: vec![changed.check],
                unrunnable: Vec::new(),
            }
        );
        assert_eq!(world.master(), commit("aaaa000"));
    }
    server.stop();
}

fn cache_predecessor_failure(world: &World, ctx: &Context, tip: &Commit, check: &TipCheck) {
    let predecessor = commit("bbbb000");
    world.land(&predecessor);
    world.work.set_checks(&predecessor, vec![check.clone()]);
    world
        .work
        .set_outcome(&predecessor, &check.check, Outcome::Fail);
    world
        .work
        .run(ctx, &predecessor, std::slice::from_ref(&check.check), false);
    world.git.set_changed(&predecessor, tip, &["src/unrelated"]);
}

fn hold_batch(
    world: &World,
    tip: &Commit,
    family: String,
) -> (Arc<Barrier>, mpsc::Sender<bool>, mpsc::Receiver<bool>) {
    let release = Arc::new(Barrier::new(2));
    let (entered, held) = mpsc::channel();
    let finished = entered.clone();
    let work = Arc::downgrade(&world.work);
    let at = tip.clone();
    let resume = release.clone();
    world.work.before_batches.lock().unwrap().insert(
        family,
        Arc::new(move || {
            let entered = entered.clone();
            let resume = resume.clone();
            work.upgrade().unwrap().on_run.lock().unwrap().insert(
                at.clone(),
                Arc::new(move || {
                    entered.send(true).unwrap();
                    resume.wait();
                }),
            );
        }),
    );
    (release, finished, held)
}
