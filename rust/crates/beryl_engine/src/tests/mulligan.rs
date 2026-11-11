//! The London mulligan, and what each player can see of the other's cards.

use super::{LANDS, MIXED, decide, land_go, play, table};
use crate::card::Purpose;
use crate::ids::Seat;
use crate::state::{Zone, ZoneKey};
use crate::two_player::TwoPlayerSeat as _;
use crate::{Choice, Decision, DecisionKind, Observation, Profile, Replay, Scratch, State, apply};

/// Answers mulligan decisions with `mulligan(player, mulligans so far)` and plays first.
fn mulligans_until(
    t: &crate::GameTable,
    s: &mut State,
    mulligan: impl Fn(Seat, u8) -> bool,
) -> (Decision, Vec<(Purpose, Seat)>) {
    let mut log = Vec::new();
    loop {
        let d = decide(t, s);
        let c = match d.prompt.purpose {
            Purpose::PlayOrDraw | Purpose::PutOnBottom => Choice(0),
            Purpose::Mulligan => Choice(u16::from(mulligan(
                d.player,
                s.player(d.player).mulligans(),
            ))),
            _ => return (d, log),
        };
        log.push((d.prompt.purpose, d.player));
        apply(s, t, &mut Scratch::new(), &mut Replay, c);
    }
}

#[test]
fn a_mulligan_puts_one_card_on_the_bottom_that_only_its_owner_knows() {
    let t = table([MIXED, MIXED]);
    let mut s = State::new(&t, 11, Profile::EXACT).unwrap();
    let (first, log) = mulligans_until(&t, &mut s, |_, n| n == 0);
    let p = s.turn().starting_player();
    let q = p.opponent();
    assert_eq!(first.kind, DecisionKind::Priority);
    // Both declare and mulligan at once; then each keeps their new seven and only then puts a
    // card on the bottom.
    assert_eq!(
        log[1..],
        [
            (Purpose::Mulligan, p),
            (Purpose::Mulligan, q),
            (Purpose::Mulligan, p),
            (Purpose::PutOnBottom, p),
            (Purpose::Mulligan, q),
            (Purpose::PutOnBottom, q),
        ]
    );
    for player in [p, q] {
        assert_eq!(s.player(player).mulligans(), 1);
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
    assert_eq!(s.zone(ZoneKey::Of(p, Zone::Hand)).count(), 6);
}

#[test]
fn a_player_decides_on_a_full_new_seven_before_putting_cards_on_the_bottom() {
    let t = table([MIXED, MIXED]);
    let mut s = State::new(&t, 11, Profile::EXACT).unwrap();
    // Mulligan twice, keeping the third seven.
    let (_, log) = mulligans_until(&t, &mut s, |player, n| player == Seat::P0 && n < 2);
    let asked: Vec<_> = log
        .iter()
        .filter(|&&(_, player)| player == Seat::P0)
        .map(|&(k, _)| k)
        .skip_while(|&k| k == Purpose::PlayOrDraw)
        .collect();
    assert_eq!(
        asked,
        [
            Purpose::Mulligan,
            Purpose::Mulligan,
            Purpose::Mulligan,
            Purpose::PutOnBottom,
            Purpose::PutOnBottom,
        ]
    );
    assert_eq!(s.player(Seat::P0).mulligans(), 2);
    assert_eq!(s.library(Seat::P0).len(), 35);
}

#[test]
fn a_player_can_mulligan_down_to_nothing() {
    let t = table([LANDS, LANDS]);
    let mut s = State::new(&t, 2, Profile::EXACT).unwrap();
    let (_, log) = mulligans_until(&t, &mut s, |player, _| player == Seat::P0);
    assert_eq!(s.player(Seat::P0).mulligans(), 7);
    assert_eq!(s.player(Seat::P1).mulligans(), 0);
    // The eighth declaration is never asked: the hand would already be empty.
    let asked = log
        .iter()
        .filter(|&&(k, player)| k == Purpose::Mulligan && player == Seat::P0)
        .count();
    assert_eq!(asked, 7);
    assert_eq!(
        s.library(Seat::P0).len() + s.zone(ZoneKey::Of(Seat::P0, Zone::Hand)).count() as usize,
        40
    );
    assert!(s.zone(ZoneKey::Of(Seat::P0, Zone::Hand)).count() <= 1);
}

#[test]
fn no_player_ever_sees_the_identity_of_a_card_in_the_other_players_hand_or_library() {
    let t = table([MIXED, MIXED]);
    let mut s = State::new(&t, 4, Profile::EXACT).unwrap();
    let mut checked = 0;
    let agent = |obs: Observation<'_>, d: &Decision| match d.kind {
        // Mulligan once each, to put a known card into each library.
        DecisionKind::KeepOrMulligan if obs.player(d.player).mulligans() == 0 => Choice(1),
        _ => land_go(obs, d),
    };
    play(&t, &mut s, agent, |s, _| {
        for viewer in Seat::BOTH {
            let obs = Observation::new(s, viewer);
            let other = viewer.opponent();
            let hidden = obs.zone(ZoneKey::Of(other, Zone::Hand));
            assert!(hidden.all(|slot| obs.card(slot).is_none()));
            for &slot in obs.library(other) {
                assert_eq!(obs.card(slot), None);
            }
            let own = obs.zone(ZoneKey::Of(viewer, Zone::Hand));
            assert!(own.all(|slot| obs.card(slot) == s.card_behind(slot)));
            for &slot in obs.battlefield() {
                assert_eq!(obs.card(slot), s.card_behind(slot));
            }
            checked += 1;
        }
    });
    assert!(checked > 50, "{checked}");
}
