//! Fair deals: the seed [`fair_seed`] picks for a start request.

use beryl_cards::CARDS;
use beryl_engine::rng::SplitMix64;
use beryl_session::fair::{Fairness, fair_seed};
use beryl_session::{Seat, StartGame, StopsSetting};

fn start(seed: u64) -> StartGame {
    let deck = beryl_card_names::decks::default_deck();
    StartGame {
        format: beryl_session::Format::CONSTRUCTED,
        seed,
        decks: [deck.clone(), deck],
        human: Seat::P0,
        opponent: "greedy".to_owned(),
        stops: StopsSetting::SMART,
        sideboards: Default::default(),
    }
}

const SMALL: Fairness = Fairness {
    deals: 6,
    playouts: 8,
};

#[test]
fn the_same_seed_picks_the_same_deal() {
    let a = fair_seed(&CARDS, &start(7), SMALL).unwrap();
    assert_eq!(a, fair_seed(&CARDS, &start(7), SMALL).unwrap());
}

/// The candidates are the first `deals` numbers of the starting seed's random stream, so the
/// pick is one of them, and with one deal to judge it is the first.
#[test]
fn the_pick_is_one_of_the_candidates_drawn_from_the_starting_seed() {
    let start = start(11);
    let mut stream = SplitMix64::new(start.seed);
    let seeds: Vec<u64> = (0..SMALL.deals).map(|_| stream.next_u64()).collect();
    let picked = fair_seed(&CARDS, &start, SMALL).unwrap();
    assert!(seeds.contains(&picked));
    let one = Fairness {
        deals: 1,
        playouts: 4,
    };
    assert_eq!(fair_seed(&CARDS, &start, one).unwrap(), seeds[0]);
}
