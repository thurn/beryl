//! The London mulligan, and what each player can see of the other's cards.

use super::{LANDS, MIXED, decide, land_go, play, table};
use crate::ids::Seat;
use crate::state::Zone;
use crate::{Choice, Decision, DecisionKind, Observation, Profile, Replay, Scratch, State, apply};

/// Answers mulligan decisions with `mulligan(player, mulligans so far)` and plays first.
fn mulligans_until(
    t: &crate::GameTable,
    s: &mut State,
    mulligan: impl Fn(Seat, u8) -> bool,
) -> (Decision, Vec<(DecisionKind, Seat)>) {
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
    let q = p.opponent();
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
        assert!(!s.knows(player.opponent(), bottom));
        // Everything else in the library was shuffled away from both players.
        for &slot in &s.library(player)[1..] {
            assert!(!s.knows(Seat::P0, slot) && !s.knows(Seat::P1, slot));
        }
    }
    // The starting player skipped their draw, so their hand is the kept six.
    assert_eq!(s.zone(p, Zone::Hand).count(), 6);
}

#[test]
fn a_player_can_mulligan_down_to_nothing() {
    let t = table([LANDS, LANDS]);
    let mut s = State::new(&t, 2, Profile::EXACT);
    let (_, log) = mulligans_until(&t, &mut s, |player, _| player == Seat::P0);
    assert_eq!(s.player(Seat::P0).mulligans, 7);
    assert_eq!(s.player(Seat::P1).mulligans, 0);
    // The eighth declaration is never asked: the hand would already be empty.
    let asked = log
        .iter()
        .filter(|&&(k, player)| k == DecisionKind::KeepOrMulligan && player == Seat::P0)
        .count();
    assert_eq!(asked, 7);
    assert_eq!(
        s.library(Seat::P0).len() + s.zone(Seat::P0, Zone::Hand).count() as usize,
        40
    );
    assert!(s.zone(Seat::P0, Zone::Hand).count() <= 1);
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
        for viewer in Seat::BOTH {
            let obs = Observation::new(s, viewer);
            let other = viewer.opponent();
            for slot in obs.zone(other, Zone::Hand).iter() {
                assert_eq!(obs.card(slot), None);
            }
            for &slot in obs.library(other) {
                assert_eq!(obs.card(slot), None);
            }
            for slot in obs.zone(viewer, Zone::Hand).iter() {
                assert_eq!(obs.card(slot), Some(s.card(slot)));
            }
            for &slot in obs.battlefield() {
                assert_eq!(obs.card(slot), Some(s.card(slot)));
            }
            checked += 1;
        }
    });
    assert!(checked > 50, "{checked}");
}
