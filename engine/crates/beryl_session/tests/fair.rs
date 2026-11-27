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

fn shared_start(seed: u64) -> StartGame {
    beryl_session::start_game("forgetful-fish", "greedy", seed).unwrap()
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

#[test]
fn a_shared_deck_start_picks_the_same_candidate_for_a_seed() {
    let start = shared_start(7);
    let mut stream = SplitMix64::new(start.seed);
    let seeds: Vec<u64> = (0..SMALL.deals).map(|_| stream.next_u64()).collect();
    let picked = fair_seed(&CARDS, &start, SMALL).unwrap();
    assert!(seeds.contains(&picked));
    assert_eq!(picked, fair_seed(&CARDS, &shared_start(7), SMALL).unwrap());
    // Confirm the judge actually distinguishes deals rather than tying every candidate on stalls.
    let varied = (0..6u64).any(|seed| {
        let first = SplitMix64::new(seed).next_u64();
        fair_seed(&CARDS, &shared_start(seed), SMALL).unwrap() != first
    });
    assert!(varied, "expected shared-deck playouts to distinguish deals");
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

/// Whether `view` names a card in its opponent's hand that the opponent has not revealed. The
/// only reveal before the first turn is the shared-deck format's free mulligan, which shows the
/// hand to the other player so they can see that it qualifies.
fn names_hidden_card(view: &beryl_session::PlayerView) -> bool {
    use beryl_session::protocol::{Declaration, ZoneName};
    let revealed = view.players.opponent.mulligan == Declaration::WillFreeMulligan;
    !revealed
        && view.objects.iter().any(|o| {
            o.zone == ZoneName::Hand
                && o.owner == beryl_session::protocol::Side::Opponent
                && o.name.is_some()
        })
}

/// Plays the opening of `deck`'s game of `seed` through the session, each seat keeping or
/// taking the first candidate, and returns whether either seat's view before the first turn named
/// a card in the other seat's hand that it had not revealed, and whether the opponent of either
/// seat took a free mulligan.
fn opening_views(deck: &str, seed: u64) -> (bool, bool) {
    use std::sync::mpsc::channel;
    use std::time::Duration;

    use beryl_session::{CandidateIndex, Sessions, Status};

    let (sent, updates) = channel();
    let (sent_across, across) = channel();
    let sessions = Sessions::new(&CARDS, None, move |u| sent.send(u).unwrap())
        .with_across(move |u| sent_across.send(u).unwrap());
    let start = beryl_session::start_game(deck, "greedy", seed).unwrap();
    sessions.start_game(&start, None).unwrap();
    let (mut leaked, mut free) = (false, false);
    loop {
        let update = updates
            .recv_timeout(Duration::from_secs(120))
            .expect("the session emitted no view within the hang limit");
        let views = std::iter::once(update.view).chain(across.try_iter().map(|u| u.view));
        let mut decision = None;
        for view in views {
            leaked |= names_hidden_card(&view);
            free |= view.players.opponent.mulligan
                == beryl_session::protocol::Declaration::WillFreeMulligan;
            if view.turn.number > 0 {
                return (leaked, free);
            }
            if let Status::Deciding { decision: d } = &view.status {
                decision = decision.or(Some(d.id));
            }
        }
        if let Some(id) = decision {
            sessions.choose(id, CandidateIndex(0)).unwrap();
        }
    }
}

/// A fair deal only picks the seed ([`fair_seed`]), so a fair game's opening is an ordinary
/// seeded game's. Neither seat's view of the opening names a card in the other seat's hand,
/// in a shared-deck game or a game with two libraries, except the hand of a player who took a
/// free mulligan, which the shared-deck format reveals. The shared-deck seeds include one
/// (13311631893818503597, the deal of an in-app report that took the reveal for a leak) where
/// the starting player's hand has no land and the free mulligan is taken.
#[test]
fn no_opening_view_names_a_card_the_other_seat_has_not_revealed() {
    let mut any_free = false;
    for seed in [13_311_631_893_818_503_597, 1, 2, 3] {
        let (leaked, free) = opening_views("forgetful-fish", seed);
        assert!(
            !leaked,
            "forgetful-fish seed {seed}: a hidden hand card was named"
        );
        any_free |= free;
    }
    assert!(
        any_free,
        "expected a shared-deck seed whose opening has a free mulligan"
    );
    for seed in [1, 2, 3] {
        let (leaked, free) = opening_views("wu-control", seed);
        assert!(
            !leaked,
            "wu-control seed {seed}: a hidden hand card was named"
        );
        assert!(!free, "a two-library game offers no free mulligan");
    }
}
