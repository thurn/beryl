//! A round robin between named agents in the WU Control mirror: N games per pairing, seats
//! alternated, games in parallel across the cores, each from a seed derived from the
//! tournament's, so a seed reproduces every game. Results come with Wilson 95% intervals, as
//! a table and as JSON.

use std::fmt::Write;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use beryl_engine::rng::SplitMix64;
use beryl_engine::{Outcome, Seat};
use beryl_search::Budget;
use serde::Serialize;

use crate::game::{Played, mirror, play};

/// One pairing's results, from its first agent's side.
#[derive(Debug, Default, Serialize)]
pub struct Pairing {
    pub a: String,
    pub b: String,
    pub games: u32,
    pub a_wins: u32,
    pub b_wins: u32,
    pub draws: u32,
    pub stalls: u32,
    /// `a`'s share of the games won, and its Wilson 95% interval.
    pub a_win_rate: f64,
    pub a_win_interval: [f64; 2],
    pub stall_rate: f64,
    pub mean_turns: f64,
    pub mean_decisions: f64,
}

#[derive(Debug, Serialize)]
pub struct Results {
    pub seed: u64,
    pub games_per_pairing: u32,
    pub pairings: Vec<Pairing>,
}

/// The seed of game `game` of pairing `pairing`. The tournament seed is hashed before the
/// game's index is mixed in: combined by a bare XOR, nearby seeds would replay the same games,
/// only reordered (seeds 1 and 3 would be one tournament).
pub fn game_seed(seed: u64, pairing: usize, game: u32) -> u64 {
    let base = SplitMix64::new(seed).next_u64();
    SplitMix64::new(base ^ ((pairing as u64) << 32) ^ u64::from(game)).next_u64()
}

/// Every pairing of distinct agents, or an agent against itself when only one is named.
fn pairings(agents: &[String]) -> Vec<(usize, usize)> {
    if agents.len() == 1 {
        return vec![(0, 0)];
    }
    let mut out = Vec::new();
    for i in 0..agents.len() {
        for j in i + 1..agents.len() {
            out.push((i, j));
        }
    }
    out
}

/// Plays the tournament, searching agents with `budget`'s iterations and trees. The first
/// invariant violation fails it.
pub fn run(
    agents: &[String],
    games: u32,
    seed: u64,
    budget: Budget,
    check: bool,
) -> Result<Results, String> {
    // Games run in parallel, one thread each, so a search grows its trees one after another.
    let budget = Budget {
        parallel: false,
        ..budget
    };
    let t = mirror();
    let pairs = pairings(agents);
    let jobs: Vec<(usize, u32)> = (0..pairs.len())
        .flat_map(|p| (0..games).map(move |g| (p, g)))
        .collect();
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<(usize, u32, Played)>> = Mutex::new(Vec::with_capacity(jobs.len()));
    let failure: Mutex<Option<String>> = Mutex::new(None);
    let workers = thread::available_parallelism().map_or(1, usize::from);
    thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                while let Some(&(p, g)) = jobs.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let (a, b) = pairs[p];
                    // Seats alternate: `a` takes the first seat in even games.
                    let names = if g % 2 == 0 {
                        [agents[a].as_str(), agents[b].as_str()]
                    } else {
                        [agents[b].as_str(), agents[a].as_str()]
                    };
                    match play(&t, game_seed(seed, p, g), names, budget, check, None) {
                        Ok(played) => results.lock().unwrap().push((p, g, played)),
                        Err(e) => {
                            failure.lock().unwrap().get_or_insert(e);
                            next.store(jobs.len(), Ordering::Relaxed);
                        }
                    }
                }
            });
        }
    });
    if let Some(e) = failure.into_inner().unwrap() {
        return Err(e);
    }
    let mut tallies: Vec<Pairing> = pairs
        .iter()
        .map(|&(a, b)| Pairing {
            a: agents[a].clone(),
            b: agents[b].clone(),
            ..Pairing::default()
        })
        .collect();
    let mut turns = vec![0u64; pairs.len()];
    let mut decisions = vec![0u64; pairs.len()];
    for (p, g, played) in results.into_inner().unwrap() {
        let tally = &mut tallies[p];
        tally.games += 1;
        turns[p] += u64::from(played.turns);
        decisions[p] += u64::from(played.decisions);
        let a_seat = if g % 2 == 0 { Seat::P0 } else { Seat::P1 };
        match played.outcome {
            Outcome::Win(w) if w == a_seat => tally.a_wins += 1,
            Outcome::Win(_) => tally.b_wins += 1,
            Outcome::Draw => tally.draws += 1,
            Outcome::Stalled(_) => tally.stalls += 1,
        }
    }
    for (p, tally) in tallies.iter_mut().enumerate() {
        let n = f64::from(tally.games.max(1));
        tally.a_win_rate = f64::from(tally.a_wins) / n;
        tally.a_win_interval = wilson(tally.a_wins, tally.games);
        tally.stall_rate = f64::from(tally.stalls) / n;
        tally.mean_turns = turns[p] as f64 / n;
        tally.mean_decisions = decisions[p] as f64 / n;
    }
    Ok(Results {
        seed,
        games_per_pairing: games,
        pairings: tallies,
    })
}

/// The Wilson score interval at 95% for `wins` of `n`.
pub fn wilson(wins: u32, n: u32) -> [f64; 2] {
    if n == 0 {
        return [0.0, 1.0];
    }
    let z = 1.959_964;
    let n = f64::from(n);
    let p = f64::from(wins) / n;
    let z2 = z * z;
    let centre = (p + z2 / (2.0 * n)) / (1.0 + z2 / n);
    let half = z * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt() / (1.0 + z2 / n);
    [(centre - half).max(0.0), (centre + half).min(1.0)]
}

/// The results as a table.
pub fn table(r: &Results) -> String {
    let mut out = format!(
        "{:<24} {:>6} {:>6} {:>6} {:>5} {:>6}  {:<17} {:>6} {:>6}\n",
        "pairing",
        "games",
        "a wins",
        "b wins",
        "draws",
        "stalls",
        "a win rate (95%)",
        "turns",
        "decs"
    );
    for p in &r.pairings {
        let _ = writeln!(
            out,
            "{:<24} {:>6} {:>6} {:>6} {:>5} {:>6}  {:>5.1}% [{:.1}, {:.1}] {:>6.1} {:>6.0}",
            format!("{} v {}", p.a, p.b),
            p.games,
            p.a_wins,
            p.b_wins,
            p.draws,
            p.stalls,
            100.0 * p.a_win_rate,
            100.0 * p.a_win_interval[0],
            100.0 * p.a_win_interval[1],
            p.mean_turns,
            p.mean_decisions,
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wilson_intervals_match_known_values() {
        let [lo, hi] = wilson(90, 100);
        assert!(
            (lo - 0.8256).abs() < 1e-3 && (hi - 0.9448).abs() < 1e-3,
            "{lo} {hi}"
        );
        let [lo, hi] = wilson(0, 0);
        assert!(lo.abs() < 1e-9 && (hi - 1.0).abs() < 1e-9);
        let [lo, hi] = wilson(10, 10);
        assert!(
            (lo - 0.7225).abs() < 1e-3 && (hi - 1.0).abs() < 1e-9,
            "{lo} {hi}"
        );
    }

    #[test]
    fn different_seeds_play_different_games() {
        let games = |seed| {
            (0..200)
                .map(|g| game_seed(seed, 0, g))
                .collect::<std::collections::HashSet<_>>()
        };
        for (a, b) in [(1, 3), (0, 2), (7, 11)] {
            assert!(games(a).is_disjoint(&games(b)), "seeds {a} and {b}");
        }
    }

    /// Seats alternate and each win is credited to the agent that won it, whichever seat it
    /// held: greedy beats random from both.
    #[test]
    fn wins_go_to_the_agent_not_the_seat() {
        let agents = ["greedy".to_owned(), "random".to_owned()];
        let r = run(&agents, 8, 3, Budget::DEFAULT, false).unwrap();
        let p = &r.pairings[0];
        assert_eq!(
            (p.a.as_str(), p.b.as_str(), p.games),
            ("greedy", "random", 8)
        );
        assert!(p.a_wins >= 6, "{p:?}");
    }

    #[test]
    fn every_pair_of_distinct_agents_meets_once() {
        let names = |v: &[&str]| v.iter().map(|&s| s.to_owned()).collect::<Vec<_>>();
        assert_eq!(pairings(&names(&["a"])), [(0, 0)]);
        assert_eq!(pairings(&names(&["a", "b", "c"])), [(0, 1), (0, 2), (1, 2)]);
    }
}
