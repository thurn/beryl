//! Fair deals: a game started from the deal, out of many candidates, that both players are
//! judged equally likely to win.
//!
//! A new game's seed fixes everything about its deal: both libraries' order, so both opening
//! hands and every later draw, the coin flip for who chooses to play first, and the reshuffle
//! after a mulligan. [`fair_seed`] deals [`Fairness::deals`] candidate seeds and plays each one
//! out [`Fairness::playouts`] times with the rollout policy in both seats, on the full state:
//! both players see every card. It keeps the candidate whose first seat won closest to half of
//! its playouts. Mulligans and the choice to play or draw happen inside the playouts like any
//! other decision, so the judgment counts them.
//!
//! The rollout policy alone plays the same game every time from one deal, so the playouts use
//! softmax rollouts ([`Softmax`]), which draw each decision from the policy's scores. Each
//! candidate's playouts draw from a random stream seeded from that candidate alone, so the
//! seed kept depends only on the starting seed and the counts, never on how many threads ran.

use std::thread;

use beryl_engine::rng::SplitMix64;
use beryl_engine::{CardTable, GameTable, Outcome, Profile, Scratch, Seat, State};
use beryl_search::{RolloutPolicy, Softmax, soft_rollout};

use crate::game::with_stops;
use crate::replay::ReplayFile;
use crate::{SessionError, StartGame, perturb};

/// How hard [`fair_seed`] looks for a fair deal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fairness {
    /// Candidate deals judged.
    pub deals: u32,
    /// Playouts per candidate. At 1000, one standard error of a near-even deal's win rate is
    /// about 1.6 points. Fewer let the pick favor deals that only looked even by chance: at
    /// 300, deals picked from 100 rejudged as far as 44% on 1000 fresh playouts.
    pub playouts: u32,
}

impl Fairness {
    /// The Mac's: about 6 s on the project's 18-core Mac, where 100 deals of 300 playouts
    /// took 1.9 s in a release build (2026-11-14).
    pub const DESKTOP: Fairness = Fairness {
        deals: 100,
        playouts: 1000,
    };

    /// The phone's: a twenty-fifth of the Mac's work, for its fewer and slower cores.
    pub const PHONE: Fairness = Fairness {
        deals: 20,
        playouts: 200,
    };

    /// This build's: [`Fairness::PHONE`] on iOS and Android, else [`Fairness::DESKTOP`].
    pub const THIS_DEVICE: Fairness = if cfg!(any(target_os = "ios", target_os = "android")) {
        Fairness::PHONE
    } else {
        Fairness::DESKTOP
    };
}

/// The softmax temperature of the playouts, in the rollout policy's score points: the
/// search's own (`beryl_search::long`).
const TEMPERATURE: f64 = 10.0;

/// The seed, out of `fairness.deals` candidates drawn from `start.seed`, whose deal the first
/// seat wins closest to half the time.
pub fn fair_seed(
    cards: &'static CardTable,
    start: &StartGame,
    fairness: Fairness,
) -> Result<u64, SessionError> {
    let (table, _) = ReplayFile::dealt(start).setup(cards)?;
    let profile = with_stops(Profile::SMART, start.stops, start.human);
    let policy = RolloutPolicy::new(&table);
    let mut stream = SplitMix64::new(start.seed);
    let seeds: Vec<u64> = (0..fairness.deals).map(|_| stream.next_u64()).collect();
    let states = seeds
        .iter()
        .map(|&seed| perturb::new_game(&table, seed, profile))
        .collect::<Result<Vec<_>, _>>()?;
    let rates = win_rates(&table, &policy, &seeds, &states, fairness.playouts);
    let best = rates
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| (*a - 0.5).abs().total_cmp(&(*b - 0.5).abs()))
        .map_or(0, |(i, _)| i);
    Ok(seeds.get(best).copied().unwrap_or(start.seed))
}

/// Each deal's first-seat win rate over `playouts` softmax rollouts, its candidates split
/// among the machine's cores. A draw, or a playout the engine gave up on, counts as half.
fn win_rates(
    table: &GameTable,
    policy: &RolloutPolicy,
    seeds: &[u64],
    states: &[State],
    playouts: u32,
) -> Vec<f64> {
    let threads = thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let chunk = states.len().div_ceil(threads).max(1);
    thread::scope(|scope| {
        let workers: Vec<_> = seeds
            .chunks(chunk)
            .zip(states.chunks(chunk))
            .map(|(seeds, states)| {
                scope.spawn(move || {
                    seeds
                        .iter()
                        .zip(states)
                        .map(|(&seed, state)| win_rate(table, policy, seed, state, playouts))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().expect("a playout thread finishes"))
            .collect()
    })
}

/// The first seat's win rate in `state`'s deal over `playouts` softmax rollouts, which draw
/// from a stream seeded by the deal's `seed`.
fn win_rate(
    table: &GameTable,
    policy: &RolloutPolicy,
    seed: u64,
    state: &State,
    playouts: u32,
) -> f64 {
    // A stream apart from the deal's own: the deal's seed also drives its shuffles.
    let mut rng = SplitMix64::new(SplitMix64::new(seed).next_u64());
    let mut x = Scratch::new();
    let mut points = 0u32;
    for _ in 0..playouts {
        let mut s = *state;
        let mut soft = Softmax {
            rng: &mut rng,
            temperature: TEMPERATURE,
        };
        points += match soft_rollout(&mut s, table, &mut x, policy, &mut soft) {
            Outcome::Win(Seat::P0) => 2,
            Outcome::Win(_) => 0,
            Outcome::Draw | Outcome::Stalled(_) => 1,
        };
    }
    f64::from(points) / f64::from(2 * playouts.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::StopsSetting;

    fn start(seed: u64) -> StartGame {
        let deck = beryl_card_names::decks::default_deck();
        StartGame {
            seed,
            decks: [deck.clone(), deck],
            human: Seat::P0,
            opponent: "greedy".to_owned(),
            stops: StopsSetting::SMART,
        }
    }

    const SMALL: Fairness = Fairness {
        deals: 6,
        playouts: 8,
    };

    #[test]
    fn the_same_seed_picks_the_same_deal() {
        let a = fair_seed(&beryl_cards::CARDS, &start(7), SMALL).unwrap();
        assert_eq!(a, fair_seed(&beryl_cards::CARDS, &start(7), SMALL).unwrap());
    }

    #[test]
    fn the_pick_is_the_candidate_nearest_even() {
        let start = start(11);
        let (table, _) = ReplayFile::dealt(&start)
            .setup(&beryl_cards::CARDS)
            .unwrap();
        let profile = with_stops(Profile::SMART, start.stops, start.human);
        let policy = RolloutPolicy::new(&table);
        let mut stream = SplitMix64::new(start.seed);
        let seeds: Vec<u64> = (0..SMALL.deals).map(|_| stream.next_u64()).collect();
        let states: Vec<State> = seeds
            .iter()
            .map(|&s| perturb::new_game(&table, s, profile).unwrap())
            .collect();
        let rates = win_rates(&table, &policy, &seeds, &states, SMALL.playouts);
        // Judged one at a time, on one thread, each deal gets the same rate.
        for (i, (&seed, state)) in seeds.iter().zip(&states).enumerate() {
            assert_eq!(
                rates[i].to_bits(),
                win_rate(&table, &policy, seed, state, SMALL.playouts).to_bits()
            );
        }
        let picked = fair_seed(&beryl_cards::CARDS, &start, SMALL).unwrap();
        let at = seeds.iter().position(|&s| s == picked).unwrap();
        let gap = |r: f64| (r - 0.5).abs();
        assert!(rates.iter().all(|&r| gap(rates[at]) <= gap(r)));
    }
}
