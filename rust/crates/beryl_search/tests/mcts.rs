//! The tree search (architecture §19.1): a seed and a budget reproduce a move with root
//! parallelism on, the hidden search decides from its Observation alone, and what cuts a
//! decision's cost (the exact stop, subtree reuse) leaves the move a seed makes as it was.
//! The strength gates are measured by the CLI's tournaments, too slow for a test.

mod common;

use beryl_engine::rng::SplitMix64;
use beryl_engine::{
    Candidates, Choice, Decision, DecisionKind, GameTable, Opt, Profile, Replay, Scratch, Seat,
    Slot, State, Step, apply, next, observe, view,
};
use beryl_search::{Agent, AgentDecision, Budget, GreedyAgent, Mcts, Sight};
use common::{mirror, play};

/// A small budget, root-parallel: three trees of two iterations.
const PARALLEL: Budget = Budget {
    iterations: 6,
    trees: 3,
    parallel: true,
};

/// Two positions from a greedy mirror game, each with more than two candidates, `from_end`
/// such decisions before the game's last. Late positions make short rollouts, the cost of
/// these tests.
fn positions(t: &GameTable, from_end: [usize; 2]) -> Vec<(State, Decision)> {
    let mut s = State::new(t, 4, Profile::SMART).unwrap();
    let mut agents = [GreedyAgent::new(t), GreedyAgent::new(t)];
    let mut all = Vec::new();
    play(t, &mut s, &mut Scratch::new(), &mut agents, |s, d| {
        if d.len() > 2 {
            all.push((*s, *d));
        }
    });
    from_end.map(|n| all[all.len() - n]).to_vec()
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
    for (i, (s, d)) in positions(&t, [10, 5]).iter().enumerate() {
        for sight in [Sight::Omniscient, Sight::Hidden] {
            let first = visits(&t, sight, PARALLEL, i as u64, s, d);
            assert_eq!(first.len(), d.len());
            assert_eq!(
                first.iter().sum::<u32>(),
                PARALLEL.iterations,
                "every iteration counts"
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
    // The first position is one where what the omniscient search reads changes its rollouts'
    // results.
    for (i, (s, d)) in positions(&t, [24, 5]).iter().enumerate() {
        let det = observe(s, &t, &mut Scratch::new(), d.player).determinize(&t, &mut rng);
        assert!(
            Slot::all().any(|slot| det.card_behind(slot) != s.card_behind(slot)),
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
/// replays the paused body, and a rollout taking over finishes the body from its record. Each
/// position is a paused question the game follows straight with another, so the tree's first
/// move leaves a resolution paused for the rollout: the game's last two, for short rollouts.
#[test]
fn a_search_plays_on_from_a_paused_resolution() {
    let t = mirror();
    let mut s = State::new(&t, 2, Profile::SMART).unwrap();
    let mut agents = [GreedyAgent::new(&t), GreedyAgent::new(&t)];
    let mut x = Scratch::new();
    let mut previous: Option<(State, Decision)> = None;
    let mut paused = Vec::new();
    play(&t, &mut s, &mut Scratch::new(), &mut agents, |s, d| {
        let pausing = !std::ptr::eq(view(s, &t, &mut x), s);
        if pausing && let Some(p) = previous {
            paused.push(p);
        }
        previous = pausing.then_some((*s, *d));
    });
    assert!(paused.len() >= 2, "a game with bodies asking twice");
    for (i, (s, d)) in paused[paused.len() - 2..].iter().enumerate() {
        for sight in [Sight::Omniscient, Sight::Hidden] {
            let n = visits(&t, sight, PARALLEL, i as u64, s, d);
            assert_eq!(n.iter().sum::<u32>(), PARALLEL.iterations, "{sight:?}");
        }
    }
}

/// Every decision with more than one candidate of seeded greedy mirror games, but priority
/// with the player's own object on top of the stack, which the search passes unsearched:
/// `count` of them, spread evenly over the games' decisions from `from_end` before each
/// game's last on.
fn searched_positions(t: &GameTable, count: usize, from_end: usize) -> Vec<(State, Decision)> {
    let mut all = Vec::new();
    for seed in 0..8 {
        let mut s = State::new(t, seed, Profile::SMART).unwrap();
        let mut agents = [GreedyAgent::new(t), GreedyAgent::new(t)];
        let mut game = Vec::new();
        play(t, &mut s, &mut Scratch::new(), &mut agents, |s, d| {
            let own_on_top = d.kind == DecisionKind::Priority
                && s.stack().last().is_some_and(|e| e.controller == d.player);
            if d.len() > 1 && !own_on_top {
                game.push((*s, *d));
            }
        });
        all.extend(game.split_off(game.len().saturating_sub(from_end)));
    }
    assert!(all.len() >= count, "{} positions", all.len());
    let step = all.len() / count;
    all.into_iter().step_by(step).take(count).collect()
}

/// The exact stop chooses as the full budget on `count` decisions, and stops early on some.
fn exact_stop_chooses_as_the_full_budget(iterations: u32, count: usize, from_end: usize) {
    let t = mirror();
    let budget = Budget {
        iterations,
        trees: 4,
        parallel: true,
    };
    let mut x = Scratch::new();
    let mut spent = 0;
    for (i, (s, d)) in searched_positions(&t, count, from_end).iter().enumerate() {
        let obs = observe(s, &t, &mut x, d.player);
        let mut agent = Mcts::new(&t, Sight::Hidden, budget, i as u64);
        let full = agent.visits(obs, d);
        let mut most = 0;
        for (j, &n) in full.iter().enumerate() {
            if n > full[most] {
                most = j;
            }
        }
        assert_eq!(
            agent.choose(obs, d),
            Choice(most as u16),
            "decision {i}: {full:?}"
        );
        spent += agent.last_search().expect("a search").rollouts;
    }
    assert!(
        spent < iterations * count as u32,
        "the stop never came early"
    );
}

#[test]
fn the_exact_stop_chooses_as_the_full_budget() {
    exact_stop_chooses_as_the_full_budget(48, 24, 6);
}

/// One of the searching agent's decisions in a game, as it was asked and answered.
struct Asked {
    state: State,
    decision: Decision,
    choice: Choice,
    /// The game's decisions before this one, either player's.
    index: usize,
    /// Where an agent rebuilt before this decision had to start, and what it searched.
    resume_point: AgentDecision,
    chain: Option<AgentDecision>,
}

/// A small budget on one thread, enough for the chosen child to have a subtree of its own.
const REUSING: Budget = Budget {
    iterations: 40,
    trees: 2,
    parallel: false,
};

/// The end of seeded greedy mirror games played on by the hidden search in seat 0 against
/// the greedy agent, from `from_end` decisions before the greedy game's last: every decision
/// of the search's, and the game indices of the greedy agent's actions, its decisions but
/// passing priority. Late positions make short rollouts.
fn searched_game(t: &GameTable, seed: u64, from_end: usize) -> (Vec<Asked>, Vec<usize>) {
    let mut starts = Vec::new();
    let mut s = State::new(t, seed, Profile::SMART).unwrap();
    play(
        t,
        &mut s,
        &mut Scratch::new(),
        &mut [GreedyAgent::new(t), GreedyAgent::new(t)],
        |s, _| starts.push(*s),
    );
    let mut s = starts[starts.len() - from_end];
    let mut search = Mcts::new(t, Sight::Hidden, REUSING, seed);
    let mut greedy = GreedyAgent::new(t);
    let mut x = Scratch::new();
    let mut asked = Vec::new();
    let mut acted = Vec::new();
    for index in 0.. {
        let Step::Decide(d) = next(&mut s, t, &mut x, &mut Replay) else {
            break;
        };
        let obs = observe(&s, t, &mut x, d.player);
        let c = if d.player == Seat::P0 {
            let resume_point = search.resume_point().expect("a search resumes");
            let c = search.choose(obs, &d);
            asked.push(Asked {
                state: s,
                decision: d,
                choice: c,
                index,
                resume_point,
                chain: search.last_search().map(|l| l.chain),
            });
            c
        } else {
            let c = greedy.choose(obs, &d);
            if !(d.kind == DecisionKind::Priority && d.candidates.option(c) == Some(Opt::Pass)) {
                acted.push(index);
            }
            c
        };
        apply(&mut s, t, &mut x, &mut Replay, c);
    }
    (asked, acted)
}

/// The slot a choice names, which it may move.
fn named(d: &Decision, c: Choice) -> Option<Slot> {
    match d.candidates {
        Candidates::Objects(_) => d.candidates.object(c),
        Candidates::Options(_) => match d.candidates.option(c)? {
            Opt::Cast { slot, .. }
            | Opt::PlayLand(slot)
            | Opt::Activate { slot, .. }
            | Opt::Object(slot) => Some(slot),
            _ => None,
        },
    }
}

/// The searching agent's games for the reuse tests: (seed, decisions before the end).
const REUSE_GAMES: [(u64, usize); 1] = [(1, 24)];

/// A decision reuses the trees of the one before only when no other player acted between,
/// no card but the one the choice named changed zones, and nothing hidden was learned (CR
/// 400.7: a zone change makes a new object, so a new incarnation). Another player's pass
/// may come between: whether they could have done more is theirs to know.
#[test]
fn subtree_reuse_stays_within_one_action() {
    let t = mirror();
    let mut reused = 0;
    for (seed, from_end) in REUSE_GAMES {
        let (asked, acted) = searched_game(&t, seed, from_end);
        for (k, pair) in (1..).zip(asked.windows(2)) {
            let [before, after] = pair else {
                unreachable!()
            };
            let Some(chain) = after.chain else { continue };
            let mut x = Scratch::new();
            let was = observe(&before.state, &t, &mut x, Seat::P0);
            let mut y = Scratch::new();
            let now = observe(&after.state, &t, &mut y, Seat::P0);
            let moves = named(&before.decision, before.choice);
            let straight = !acted
                .iter()
                .any(|&i| (before.index..after.index).contains(&i))
                && Slot::all().all(|slot| {
                    was.knows(slot) == now.knows(slot)
                        && (was.object(slot).incarnation() == now.object(slot).incarnation()
                            || moves == Some(slot))
                });
            if chain < AgentDecision(k) {
                reused += 1;
                assert!(
                    straight,
                    "seed {seed}: reused at game decision {}",
                    after.index
                );
            }
        }
    }
    assert!(reused > 0, "the games reuse a subtree");
}

/// An agent rebuilt from its seed in the middle of a game chooses as the uninterrupted one,
/// asked first the decision its resume point names: the next, or inside a reuse chain the
/// decision the chain started at, which it searches again.
#[test]
fn a_rebuilt_agent_plays_on_as_the_uninterrupted_one() {
    let t = mirror();
    let mut inside_chain = false;
    for (seed, from_end) in REUSE_GAMES {
        let (asked, _) = searched_game(&t, seed, from_end);
        for (k, a) in asked.iter().enumerate() {
            let r = a.resume_point.0 as usize;
            let chain_ends = asked
                .get(k + 1)
                .is_none_or(|b| b.resume_point != a.resume_point);
            // Rebuild at the last decision of each reuse chain, and at every fourth outside
            // one, for the test's time.
            if r < k && chain_ends {
                inside_chain = true;
            } else if r < k || k % 4 != 0 {
                continue;
            }
            let mut rebuilt = Mcts::resumed(&t, Sight::Hidden, REUSING, seed, a.resume_point);
            let mut x = Scratch::new();
            for b in &asked[r..=k] {
                let obs = observe(&b.state, &t, &mut x, Seat::P0);
                assert_eq!(
                    rebuilt.choose(obs, &b.decision),
                    b.choice,
                    "seed {seed}: rebuilt before decision {r}, at {k}"
                );
            }
        }
    }
    assert!(inside_chain, "a rebuild inside a reuse chain");
}

/// An agent rewound to one of its own decisions, as a game taken back is, searches it again
/// from the seed it had there: a decision searched fresh the first time gets the same move.
#[test]
fn a_rewound_agent_searches_a_decision_again_as_it_did() {
    let t = mirror();
    let (seed, from_end) = REUSE_GAMES[0];
    let (asked, _) = searched_game(&t, seed, from_end);
    let fresh: Vec<usize> = (0..asked.len())
        .filter(|&k| asked[k].resume_point == AgentDecision(k as u32))
        .collect();
    assert!(fresh.len() > 1, "fresh searches to go back to");
    let mut agent = Mcts::new(&t, Sight::Hidden, REUSING, seed);
    let mut x = Scratch::new();
    for (k, a) in asked.iter().enumerate() {
        agent.choose(observe(&a.state, &t, &mut x, Seat::P0), &a.decision);
        assert_eq!(agent.next_decision(), Some(AgentDecision(k as u32 + 1)));
    }
    for &k in fresh.iter().rev() {
        agent.rewind(AgentDecision(k as u32));
        let a = &asked[k];
        assert_eq!(
            agent.choose(observe(&a.state, &t, &mut x, Seat::P0), &a.decision),
            a.choice,
            "rewound to decision {k}"
        );
    }
}
