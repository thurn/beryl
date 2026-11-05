//! The tree search (architecture §19.1): a seed and a budget reproduce a move with root
//! parallelism on, and the hidden search decides from its Observation alone. The strength
//! gates are measured by the CLI's tournaments, too slow for a test.

mod common;

use beryl_engine::rng::SplitMix64;
use beryl_engine::{Decision, GameTable, Profile, Scratch, Slot, State, observe, view};
use beryl_search::{Budget, GreedyAgent, Mcts, Sight};
use common::{mirror, play};

/// A small budget, root-parallel: three trees of two iterations.
const PARALLEL: Budget = Budget {
    iterations: 6,
    trees: 3,
    parallel: true,
};

/// Positions from a greedy mirror game, every fifth decision from turn 12 on, so rollouts are
/// short, each with more than two candidates.
fn positions(t: &GameTable) -> Vec<(State, Decision)> {
    let mut s = State::new(t, 4, Profile::SMART);
    let mut agents = [GreedyAgent::new(t), GreedyAgent::new(t)];
    let mut out = Vec::new();
    let mut n = 0;
    play(t, &mut s, &mut Scratch::new(), &mut agents, |s, d| {
        if s.turn().number >= 12 && d.len() > 2 {
            n += 1;
            if n % 5 == 0 && out.len() < 2 {
                out.push((*s, *d));
            }
        }
    });
    assert_eq!(out.len(), 2, "a game long enough to sample");
    out
}

/// The summed root visit counts of a search from `s`, seen by the decision's player.
fn visits(
    t: &GameTable,
    sight: Sight,
    budget: Budget,
    seed: u64,
    s: &State,
    d: &Decision,
) -> Vec<u32> {
    let mut x = Scratch::new();
    Mcts::new(t, sight, budget, seed).visits(observe(s, t, &mut x, d.player), d)
}

#[test]
fn a_seed_and_a_budget_reproduce_a_move_with_root_parallelism() {
    let t = mirror();
    for (i, (s, d)) in positions(&t).iter().enumerate() {
        for sight in [Sight::Omniscient, Sight::Hidden] {
            let first = visits(&t, sight, PARALLEL, i as u64, s, d);
            assert_eq!(first.len(), d.len());
            assert_eq!(
                first.iter().sum::<u32>(),
                PARALLEL.iterations,
                "every iteration counts"
            );
            assert_eq!(
                visits(&t, sight, PARALLEL, i as u64, s, d),
                first,
                "{sight:?} again"
            );
            let sequential = Budget {
                parallel: false,
                ..PARALLEL
            };
            assert_eq!(
                visits(&t, sight, sequential, i as u64, s, d),
                first,
                "{sight:?} on one thread"
            );
        }
    }
}

/// The hidden search decides the same from the true state and from one with every card it
/// cannot see dealt anew (architecture §13); the omniscient one, which reads them, does not.
#[test]
fn the_hidden_search_decides_the_same_on_a_redeterminized_state() {
    let t = mirror();
    let mut rng = SplitMix64::new(11);
    let mut omniscient_differs = false;
    for (i, (s, d)) in positions(&t).iter().enumerate() {
        let det = observe(s, &t, &mut Scratch::new(), d.player).determinize(&t, &mut rng);
        assert!(
            Slot::all().any(|slot| det.card(slot) != s.card(slot)),
            "position {i}: hidden cards were dealt anew"
        );
        // One tree that plays every candidate and then some, so its counts follow the
        // rollouts' results rather than only the order fresh candidates are tried in.
        let budget = Budget {
            iterations: d.len() as u32 + 6,
            trees: 1,
            parallel: false,
        };
        let seed = i as u64;
        assert_eq!(
            visits(&t, Sight::Hidden, budget, seed, &det, d),
            visits(&t, Sight::Hidden, budget, seed, s, d),
            "position {i}"
        );
        omniscient_differs |= visits(&t, Sight::Omniscient, budget, seed, &det, d)
            != visits(&t, Sight::Omniscient, budget, seed, s, d);
    }
    // The test can catch a search that reads hidden cards.
    assert!(omniscient_differs);
}

/// A search can start at a question asked in the middle of a resolution (§7.3): its tree
/// replays the paused body, and a rollout taking over finishes the body from its record.
#[test]
fn a_search_plays_on_from_a_paused_resolution() {
    let t = mirror();
    let mut s = State::new(&t, 2, Profile::SMART);
    let mut agents = [GreedyAgent::new(&t), GreedyAgent::new(&t)];
    let mut x = Scratch::new();
    let mut paused = Vec::new();
    play(&t, &mut s, &mut Scratch::new(), &mut agents, |s, d| {
        if paused.len() < 2 && !std::ptr::eq(view(s, &t, &mut x), s) {
            paused.push((*s, *d));
        }
    });
    assert_eq!(paused.len(), 2, "a game with paused resolutions");
    for (i, (s, d)) in paused.iter().enumerate() {
        for sight in [Sight::Omniscient, Sight::Hidden] {
            let n = visits(&t, sight, PARALLEL, i as u64, s, d);
            assert_eq!(n.iter().sum::<u32>(), PARALLEL.iterations, "{sight:?}");
        }
    }
}
