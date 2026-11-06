//! A generic board evaluation (docs/ai-strategy.md §4): the chance the first seat wins a
//! position, from what a player has, read off card data and the card values of a
//! [`crate::Knowledge`] tier, never card names. It is a logistic function of the difference
//! between the two players' [`Features`], with weights fitted by `beryl_cli eval-fit` on
//! seeded games, so it reads as a probability.
//!
//! It reads a full state: a determinization's or the true one. Search calls it at a leaf,
//! where the state is a world it sampled, so reading hands there is no cheating.

use beryl_engine::Layout;
use beryl_engine::card::Types;
use beryl_engine::{Seat, State, Zone};

use crate::values::CardValues;

/// What one player has, as the evaluation counts it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Features(pub [f64; FEATURES]);

/// How many features there are.
pub const FEATURES: usize = 8;

/// The features' names, in order, for a fit's report.
pub const NAMES: [&str; FEATURES] = [
    "life",
    "cards in hand",
    "hand value",
    "lands",
    "permanent value",
    "creature power",
    "loyalty",
    "library short",
];

/// The fitted weights, per unit of each feature's difference, first seat less second:
/// `beryl_cli eval-fit --games 6000 --seed 1`, 2026-11-06, over 145k positions of the frozen
/// expert against greedy and of greedy against itself in the cb8d5d5f mirror, each turn's
/// first decision labeled with the winner. Held out (every fifth game): log loss 0.539
/// against a coin's 0.693, 70.6% of winners called.
pub const WEIGHTS: [f64; FEATURES] = [
    0.0239, 0.1790, 0.0013, 0.1502, 0.0021, 0.0607, 0.1465, -0.3084,
];

/// A library this short or shorter starts to count against its owner: a player who must
/// draw from an empty one loses
/// ([CR 704.5b](https://yawgatog.com/resources/magic-rules/#R7045b)).
const LIBRARY_SHORT: usize = 5;

/// Player `p`'s features in `s`.
pub fn features<L: Layout>(s: &State<L>, values: &CardValues, p: Seat) -> Features {
    let mut f = [0.0; FEATURES];
    f[0] = f64::from(s.player(p).life.clamp(-10, 40));
    let hand = s.zone(p, Zone::Hand);
    f[1] = f64::from(hand.count());
    f[2] = hand
        .iter()
        .map(|slot| f64::from(values.get(s.card(slot)).value))
        .sum();
    for &slot in s.battlefield() {
        if s.controller(slot) != p {
            continue;
        }
        let d = s.derived(slot);
        if d.types.intersects(Types::LAND) {
            f[3] += 1.0;
            continue;
        }
        f[4] += f64::from(values.get(s.card(slot)).value);
        if d.types.intersects(Types::CREATURE) {
            f[5] += f64::from(d.power.max(0));
        }
        if d.types.intersects(Types::PLANESWALKER) {
            f[6] += f64::from(s.object(slot).counters.loyalty);
        }
    }
    f[7] = LIBRARY_SHORT.saturating_sub(s.library(p).len()) as f64;
    Features(f)
}

/// The weighted difference of the two players' features, first seat less second: the
/// log-odds the first seat wins.
pub fn logit<L: Layout>(s: &State<L>, values: &CardValues, weights: &[f64; FEATURES]) -> f64 {
    let [a, b] = Seat::BOTH.map(|p| features(s, values, p).0);
    a.iter()
        .zip(&b)
        .zip(weights)
        .map(|((x, y), w)| w * (x - y))
        .sum()
}

/// The chance the first seat wins `s`.
pub fn evaluate<L: Layout>(s: &State<L>, values: &CardValues) -> f64 {
    sigmoid(logit(s, values, &WEIGHTS))
}

pub fn sigmoid(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sigmoid_is_a_probability() {
        assert!((sigmoid(0.0) - 0.5).abs() < 1e-12);
        assert!(sigmoid(10.0) > 0.99 && sigmoid(-10.0) < 0.01);
    }
}
