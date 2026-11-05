//! The London mulligan, and what each player can see of the other's cards.

use super::{LANDS, MIXED, decide, land_go, play, table};
use crate::state::{Zone, opponent};
use crate::{
    Choice, Decision, DecisionKind, Observation, Profile, Replay, Scratch, State, UNKNOWN, apply,
};

/// Answers mulligan decisions with `mulligan(player, mulligans so far)` and plays first.
fn mulligans_until(
    t: &crate::GameTable,
    s: &mut State,
    mulligan: impl Fn(u8, u8) -> bool,
) -> (Decision, Vec<(DecisionKind, u8)>) {
    let mut log = Vec::new();
    loop {
        let d = decide(t, s);
        let c = match d.kind {
            DecisionKind::PlayOrDraw | DecisionKind::PutOnBottom => Choice(0),
            DecisionKind::KeepOrMulligan => {
                Choice(u8::from(mulligan(d.player, s.player(d.player).mulligans)))
            }
            _ => return (d, log),
        };
        log.push((d.kind, d.player));
        apply(s, t, &mut Scratch::new(), &mut Replay, c);
    }
}

#[test]
fn a_mulligan_puts_one_card_on_the_bottom_that_only_its_owner_knows() {
    let t = table([MIXED, MIXED]);
    let mut s = State::new(&t, 11, Profile::EXACT);
    let (first, log) = mulligans_until(&t, &mut s, |_, n| n == 0);
    let p = s.turn().starting_player;
    let q = opponent(p);
    assert_eq!(first.kind, DecisionKind::Priority);
    // Both declare, both mulligan at once, both bottom a card, then both keep.
    let kinds: Vec<_> = log.iter().map(|&(k, player)| (k, player)).collect();
    assert_eq!(
        kinds[1..],
        [
            (DecisionKind::KeepOrMulligan, p),
            (DecisionKind::KeepOrMulligan, q),
            (DecisionKind::PutOnBottom, p),
            (DecisionKind::PutOnBottom, q),
            (DecisionKind::KeepOrMulligan, p),
            (DecisionKind::KeepOrMulligan, q),
        ]
    );
    for player in [p, q] {
        assert_eq!(s.player(player).mulligans, 1);
        assert_eq!(s.library(player).len(), 34);
        let bottom = s.library(player)[0];
        assert!(s.knows(player, bottom));
        assert!(!s.knows(opponent(player), bottom));
        // Everything else in the library was shuffled away from both players.
        for &slot in &s.library(player)[1..] {
            assert!(!s.knows(0, slot) && !s.knows(1, slot));
        }
    }
    // The starting player skipped their draw, so their hand is the kept six.
    assert_eq!(s.zone(p, Zone::Hand).count(), 6);
}

#[test]
fn a_player_can_mulligan_down_to_nothing() {
    let t = table([LANDS, LANDS]);
    let mut s = State::new(&t, 2, Profile::EXACT);
    let (_, log) = mulligans_until(&t, &mut s, |player, _| player == 0);
    assert_eq!(s.player(0).mulligans, 7);
    assert_eq!(s.player(1).mulligans, 0);
    // The eighth declaration is never asked: the hand would already be empty.
    let asked = log
        .iter()
        .filter(|&&(k, player)| k == DecisionKind::KeepOrMulligan && player == 0)
        .count();
    assert_eq!(asked, 7);
    assert_eq!(
        s.library(0).len() + s.zone(0, Zone::Hand).count() as usize,
        40
    );
    assert!(s.zone(0, Zone::Hand).count() <= 1);
}

#[test]
fn no_player_ever_sees_the_identity_of_a_card_in_the_other_players_hand_or_library() {
    let t = table([MIXED, MIXED]);
    let mut s = State::new(&t, 4, Profile::EXACT);
    let mut checked = 0;
    let agent = |obs: Observation<'_>, d: &Decision| match d.kind {
        // Mulligan once each, to put a known card into each library.
        DecisionKind::KeepOrMulligan if obs.player(d.player).mulligans == 0 => Choice(1),
        _ => land_go(obs, d),
    };
    play(&t, &mut s, agent, |s, _| {
        for viewer in 0..2 {
            let obs = Observation::new(s, viewer);
            let other = opponent(viewer);
            for slot in obs.zone(other, Zone::Hand).iter() {
                assert_eq!(obs.card(slot), UNKNOWN);
                assert_eq!(obs.object(slot).card, UNKNOWN);
            }
            for &slot in obs.library(other) {
                assert_eq!(obs.card(slot), UNKNOWN);
            }
            for slot in obs.zone(viewer, Zone::Hand).iter() {
                assert_eq!(obs.card(slot), s.object(slot).card);
            }
            for &slot in obs.battlefield() {
                assert_eq!(obs.card(slot), s.object(slot).card);
            }
            checked += 1;
        }
    });
    assert!(checked > 50, "{checked}");
}
