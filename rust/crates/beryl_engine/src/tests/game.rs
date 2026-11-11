//! Whole games: turns, priority, land drops, drawing, decking and determinism.

use super::{LANDS, MIXED, fingerprint, land_go, play, table};
use crate::card::Types;
use crate::filter::{Evaluate, Filter};
use crate::ids::Seat;
use crate::profile::Limits;
use crate::state::{Zone, ZoneKey};
use crate::{Choice, DecisionKind, Decklist, Opt, Outcome, Profile, StallReason, State};

/// Plays a land-and-pass game, returning the outcome, the final state and the state hash at
/// every decision.
fn land_and_pass(decks: [&Decklist; 2], seed: u64, profile: Profile) -> (Outcome, State, Vec<u64>) {
    let t = table(decks);
    let mut s = State::new(&t, seed, profile);
    let mut hashes = Vec::new();
    let outcome = play(&t, &mut s, land_go, |s, _| hashes.push(fingerprint(s)));
    (outcome, s, hashes)
}

#[test]
fn land_and_pass_decks_the_player_on_the_draw_first() {
    for profile in [Profile::EXACT, Profile::SMART] {
        for seed in 0..4 {
            let (outcome, s, hashes) = land_and_pass([LANDS, LANDS], seed, profile);
            let starting = s.turn().starting_player;
            // Each library holds 33 cards after the opening hands. The starting player skips
            // their first draw, so the other player is the first to draw from an empty library,
            // on their 34th turn: turn 68.
            assert_eq!(outcome, Outcome::Win(starting), "seed {seed}");
            assert_eq!(s.turn().number, 68);
            assert!(s.player(starting.opponent()).drew_from_empty);
            assert_eq!(s.library(starting).len(), 0);
            // One land a turn: the starting player played 34, the other 33.
            assert_eq!(s.battlefield().len(), 67);
            assert!(!hashes.is_empty());
        }
    }
}

#[test]
fn one_seed_reproduces_the_game_hash_for_hash() {
    for profile in [Profile::EXACT, Profile::SMART] {
        let (a_outcome, a, a_hashes) = land_and_pass([MIXED, LANDS], 7, profile);
        let (b_outcome, b, b_hashes) = land_and_pass([MIXED, LANDS], 7, profile);
        assert_eq!(a_outcome, b_outcome);
        assert_eq!(a_hashes, b_hashes);
        assert_eq!(fingerprint(&a), fingerprint(&b));
        let (_, _, other_seed) = land_and_pass([MIXED, LANDS], 8, profile);
        assert_ne!(a_hashes, other_seed);
    }
}

#[test]
fn choosing_to_draw_makes_the_other_player_start() {
    let t = table([LANDS, LANDS]);
    let mut s = State::new(&t, 1, Profile::EXACT);
    let mut chooser = None;
    let outcome = play(
        &t,
        &mut s,
        |obs, d| {
            if d.kind == DecisionKind::PlayOrDraw {
                chooser = Some(obs.viewer());
                assert_eq!(d.candidates.option(Choice(1)), Some(Opt::Draw));
                return Choice(1);
            }
            land_go(obs, d)
        },
        |_, _| {},
    );
    let starting = chooser.unwrap().opponent();
    assert_eq!(s.turn().starting_player, starting);
    assert_eq!(outcome, Outcome::Win(starting));
}

#[test]
fn only_the_active_player_plays_one_land_in_a_main_phase() {
    let t = table([MIXED, MIXED]);
    let mut s = State::new(&t, 3, Profile::EXACT);
    let mut land_offers = 0;
    play(&t, &mut s, land_go, |s, d| {
        if d.kind != DecisionKind::Priority {
            return;
        }
        let turn = s.turn();
        assert_eq!(d.player, turn.priority);
        // With nothing else to do, priority is a decision only when a land can be played.
        assert_eq!(d.len(), 2);
        assert_eq!(d.player, turn.active);
        assert!(matches!(
            turn.step,
            crate::state::TurnStep::PrecombatMain | crate::state::TurnStep::PostcombatMain
        ));
        assert_eq!(s.player(d.player).lands_played, 0);
        let Some(Opt::PlayLand(slot)) = d.candidates.option(Choice(1)) else {
            panic!("expected a land play");
        };
        assert!(
            Filter::Types(Types::LAND).matches(s, &t, slot, d.player)
                && s.zone(ZoneKey::Of(d.player, Zone::Hand)).contains(slot)
        );
        land_offers += 1;
    });
    assert!(land_offers > 20);
}

#[test]
fn cleanup_discards_down_to_seven() {
    let spells: &Decklist = &[(20, "Test Spell A"), (20, "Test Spell B")];
    let t = table([spells, spells]);
    let mut s = State::new(&t, 5, Profile::SMART);
    let mut discards = 0;
    let outcome = play(&t, &mut s, land_go, |s, d| {
        let active = s.turn().active;
        // The player who isn't active ended their own turn at seven cards or fewer.
        assert!(s.zone(ZoneKey::Of(active.opponent(), Zone::Hand)).count() <= 7);
        if d.kind == DecisionKind::Discard {
            assert_eq!(d.player, active);
            assert!(s.zone(ZoneKey::Of(active, Zone::Hand)).count() > 7);
            discards += 1;
        }
    });
    assert_eq!(outcome, Outcome::Win(s.turn().starting_player));
    assert!(discards > 5);
    for p in Seat::BOTH {
        assert_eq!(
            s.graveyard(p).len() + s.zone(ZoneKey::Of(p, Zone::Hand)).count() as usize,
            40
        );
    }
}

#[test]
fn a_game_past_its_turn_budget_stalls() {
    let t = table([LANDS, LANDS]);
    let profile = Profile {
        limits: Limits {
            turns: 10,
            ..Limits::DEFAULT
        },
        ..Profile::EXACT
    };
    let mut s = State::new(&t, 0, profile);
    let outcome = play(&t, &mut s, land_go, |_, _| {});
    assert_eq!(outcome, Outcome::Stalled(StallReason::Turns));
    assert_eq!(s.turn().number, 10);
}

#[test]
fn search_keeps_the_default_budgets_until_a_player_game_passes_them() {
    let d = Limits::DEFAULT;
    assert_eq!(Limits::search(0, 0), d);
    assert_eq!(Limits::search(199, 499), d);
    let past = Limits::search(250, 600);
    assert_eq!(past.turns, 450);
    assert_eq!(past.decisions_per_turn, 1_100);
    assert_eq!(past.state_based_iterations, d.state_based_iterations);
    assert_eq!(
        Limits::search(u16::MAX, u16::MAX),
        Limits {
            state_based_iterations: d.state_based_iterations,
            ..Limits::PLAYER
        }
    );
    assert_eq!(
        Limits::PLAYER.state_based_iterations,
        d.state_based_iterations
    );
}
