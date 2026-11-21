//! Whole games: turns, priority, land drops, drawing, decking and determinism.

use super::{LANDS, MIXED, fingerprint, land_go, play, table};
use crate::card::Types;
use crate::filter::{Evaluate, Filter};
use crate::profile::Limits;
use crate::state::{Zone, ZoneKey};
use crate::{Choice, DecisionKind, Decklist, Opt, Outcome, Profile, StallReason, State};
use beryl_kernel::two_player::TwoPlayerSeat as _;

/// Plays a land-and-pass game, returning the outcome, the final state and the state hash at
/// every decision.
fn land_and_pass(decks: [&Decklist; 2], seed: u64, profile: Profile) -> (Outcome, State, Vec<u64>) {
    let t = table(decks);
    let mut s = State::new(&t, seed, profile).unwrap();
    let mut hashes = Vec::new();
    let outcome = play(&t, &mut s, land_go, |s, _| hashes.push(fingerprint(s)));
    (outcome, s, hashes)
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
    let mut s = State::new(&t, 1, Profile::EXACT).unwrap();
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
    assert_eq!(s.turn().starting_player(), starting);
    assert_eq!(outcome, Outcome::Win(starting));
}

#[test]
fn only_the_active_player_plays_one_land_in_a_main_phase() {
    let t = table([MIXED, MIXED]);
    let mut s = State::new(&t, 3, Profile::EXACT).unwrap();
    let mut land_offers = 0;
    play(&t, &mut s, land_go, |s, d| {
        if d.kind != DecisionKind::Priority {
            return;
        }
        let turn = s.turn();
        assert_eq!(d.player, turn.priority());
        // With nothing else to do, priority is a decision only when a land can be played.
        assert_eq!(d.len(), 2);
        assert_eq!(d.player, turn.active());
        assert!(matches!(
            turn.step(),
            crate::state::TurnStep::PrecombatMain | crate::state::TurnStep::PostcombatMain
        ));
        assert_eq!(s.player(d.player).lands_played(), 0);
        assert!(crate::turn::may_play_land(s, d.player));
        assert!(!crate::turn::may_play_land(s, d.player.opponent()));
        let Some(Opt::PlayLand(slot)) = d.candidates.option(Choice(1)) else {
            panic!("expected a land play");
        };
        assert!(
            Filter::Types(Types::LAND).matches(s, &t, slot, d.player, &crate::card::Words::NONE)
                && s.zone(ZoneKey::Of(d.player, Zone::Hand)).contains(slot)
        );
        land_offers += 1;
    });
    assert!(land_offers > 20);
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
    let mut s = State::new(&t, 0, profile).unwrap();
    let outcome = play(&t, &mut s, land_go, |_, _| {});
    assert_eq!(outcome, Outcome::Stalled(StallReason::Turns));
    assert_eq!(s.turn().number(), 10);
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
