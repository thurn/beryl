//! The London mulligan, and what each player can see of the other's cards.

use super::{CARDS, LANDS, MIXED, decide, fixture_slots, land_go, play, table};
use crate::card::{Purpose, Types};
use crate::ids::Seat;
use crate::replacement::event::Event;
use crate::state::{Zone, ZoneKey};
use crate::{
    Choice, Decision, DecisionKind, Decklist, GameTable, Observation, Opt, Profile, Replay,
    Scratch, State, apply,
};
use beryl_kernel::format::Format;
use beryl_kernel::two_player::TwoPlayerSeat as _;

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

fn shared_table(deck: &Decklist) -> GameTable {
    GameTable::new(Format::SHARED_DECK, &CARDS, [deck, &[]]).unwrap()
}

fn option_choice(d: &Decision, opt: Opt) -> Option<Choice> {
    (0..d.len()).find_map(|i| {
        let c = Choice(i as u16);
        (d.candidates.option(c) == Some(opt)).then_some(c)
    })
}

fn hand_lands(s: &State, t: &GameTable, player: Seat) -> usize {
    s.zone(ZoneKey::Of(player, Zone::Hand))
        .filter(|slot| s.copiable(slot).types(t).contains(Types::LAND))
        .count() as usize
}

#[test]
fn shared_deal_alternates_from_the_starting_player() {
    let t = shared_table(&[(40, "Test Land"), (40, "Test Spell A")]);
    for play_choice in [Opt::Play, Opt::Draw] {
        let mut s = State::new(&t, 19, Profile::EXACT).unwrap();
        let d = decide(&t, &mut s);
        assert_eq!(d.kind, DecisionKind::PlayOrDraw);
        let top14: Vec<_> = s.library_from_top(Seat::P0).take(14).collect();
        let c = option_choice(&d, play_choice).unwrap();
        apply(&mut s, &t, &mut Scratch::new(), &mut Replay, c);

        let starting = s.turn().starting_player();
        let other = starting.opponent();
        let starting_hand = s.zone(ZoneKey::Of(starting, Zone::Hand));
        let other_hand = s.zone(ZoneKey::Of(other, Zone::Hand));
        assert_eq!(starting_hand.count(), 7);
        assert_eq!(other_hand.count(), 7);
        for (i, &slot) in top14.iter().enumerate() {
            if i % 2 == 0 {
                assert!(starting_hand.contains(slot), "card {i} goes to starting");
            } else {
                assert!(other_hand.contains(slot), "card {i} goes to other");
            }
        }
        assert_eq!(s.library(Seat::P0).len(), 66);
        assert_eq!(s.library(Seat::P0), s.library(Seat::P1));
        assert_eq!(s.check_invariants(&t), Ok(()));
    }
}

#[test]
fn free_mulligan_is_offered_for_zero_one_six_or_seven_lands_only() {
    let all_spells = shared_table(&[(80, "Test Spell A")]);
    let all_lands = shared_table(&[(80, "Test Land")]);
    let mixed = shared_table(&[(40, "Test Land"), (40, "Test Spell A")]);
    let mut seen = [false; 8];

    // 0 and 7 lands directly from uniform decks; 1..=6 from seeded mixed deals.
    for (t, target) in [(&all_spells, 0usize), (&all_lands, 7usize)] {
        let mut s = State::new(t, 1, Profile::EXACT).unwrap();
        let play_or_draw = decide(t, &mut s);
        apply(&mut s, t, &mut Scratch::new(), &mut Replay, Choice(0));
        assert_eq!(play_or_draw.kind, DecisionKind::PlayOrDraw);
        let d = decide(t, &mut s);
        assert_eq!(d.kind, DecisionKind::KeepOrMulligan);
        assert_eq!(hand_lands(&s, t, d.player), target);
        assert!(option_choice(&d, Opt::FreeMulligan).is_some());
        seen[target] = true;
    }

    for seed in 0..500u64 {
        if seen.iter().all(|&b| b) {
            break;
        }
        let mut s = State::new(&mixed, seed, Profile::EXACT).unwrap();
        let _ = decide(&mixed, &mut s);
        apply(&mut s, &mixed, &mut Scratch::new(), &mut Replay, Choice(0));
        let d = decide(&mixed, &mut s);
        assert_eq!(d.kind, DecisionKind::KeepOrMulligan);
        let lands = hand_lands(&s, &mixed, d.player);
        let offered = option_choice(&d, Opt::FreeMulligan).is_some();
        assert_eq!(
            offered,
            !(2..=5).contains(&lands),
            "seed {seed} had {lands} lands"
        );
        seen[lands] = true;
    }
    assert!(seen.iter().all(|&b| b), "missing land counts: {seen:?}");

    // Constructed never offers a free mulligan, even with 7 lands.
    let constructed = table([LANDS, LANDS]);
    let mut s = State::new(&constructed, 1, Profile::EXACT).unwrap();
    let _ = decide(&constructed, &mut s);
    apply(
        &mut s,
        &constructed,
        &mut Scratch::new(),
        &mut Replay,
        Choice(0),
    );
    let d = decide(&constructed, &mut s);
    assert_eq!(d.kind, DecisionKind::KeepOrMulligan);
    assert!(option_choice(&d, Opt::FreeMulligan).is_none());
}

#[test]
fn no_free_mulligan_after_a_regular_mulligan_and_bottomed_card_goes_to_shared_library() {
    // Two distinct land cards: every seven-card hand is all lands, and a hand with both kinds
    // does not auto-collapse the PutOnBottom choice.
    let t = shared_table(&[(40, "Test Land"), (40, "Test Dual")]);
    let mut s = State::new(&t, 7, Profile::EXACT).unwrap();
    let _ = decide(&t, &mut s);
    apply(&mut s, &t, &mut Scratch::new(), &mut Replay, Choice(0));

    let starting = s.turn().starting_player();
    let other = starting.opponent();

    // Round 1: starting takes a regular mulligan even though a free mulligan is offered;
    // opponent keeps.
    let d = decide(&t, &mut s);
    assert_eq!(d.player, starting);
    assert!(option_choice(&d, Opt::FreeMulligan).is_some());
    let mull = option_choice(&d, Opt::Mulligan).unwrap();
    apply(&mut s, &t, &mut Scratch::new(), &mut Replay, mull);

    let d = decide(&t, &mut s);
    assert_eq!(d.player, other);
    let keep = option_choice(&d, Opt::Keep).unwrap();
    apply(&mut s, &t, &mut Scratch::new(), &mut Replay, keep);

    // Round 2: starting has a 7-land hand again, but has taken a regular mulligan, so no free
    // mulligan is offered.
    let d = decide(&t, &mut s);
    assert_eq!(d.player, starting);
    assert_eq!(s.player(starting).mulligans(), 1);
    assert_eq!(hand_lands(&s, &t, starting), 7);
    assert!(option_choice(&d, Opt::FreeMulligan).is_none());
    let keep = option_choice(&d, Opt::Keep).unwrap();
    apply(&mut s, &t, &mut Scratch::new(), &mut Replay, keep);

    // Starting owes one card to the bottom of the shared library.
    let d = decide(&t, &mut s);
    assert_eq!(d.prompt.purpose, Purpose::PutOnBottom);
    assert_eq!(d.player, starting);
    let chosen_slot = d.candidates.object(Choice(0)).unwrap();
    apply(&mut s, &t, &mut Scratch::new(), &mut Replay, Choice(0));

    assert_eq!(s.library(Seat::P0)[0], chosen_slot);
    assert_eq!(s.library(Seat::P1)[0], chosen_slot);
    assert!(!s.has_owner(chosen_slot));
    assert!(s.knows(starting, chosen_slot));
    assert!(!s.knows(other, chosen_slot));
    assert_eq!(s.library(Seat::P0).len(), 67);
    assert_eq!(s.check_invariants(&t), Ok(()));
}

#[test]
fn two_free_mulligans_in_a_row_cost_no_cards_and_reveal_the_hand() {
    let t = shared_table(&[(80, "Test Land")]);
    let mut s = State::new(&t, 13, Profile::EXACT).unwrap();
    let _ = decide(&t, &mut s);
    apply(&mut s, &t, &mut Scratch::new(), &mut Replay, Choice(0));

    let starting = s.turn().starting_player();
    let other = starting.opponent();
    let first_hand = s.zone(ZoneKey::Of(starting, Zone::Hand));
    assert_eq!(first_hand.count(), 7);
    assert!(first_hand.all(|slot| !s.knows(other, slot)));

    // First free mulligan: reveals starting's hand to the opponent before the opponent declares.
    let d = decide(&t, &mut s);
    let free = option_choice(&d, Opt::FreeMulligan).unwrap();
    let mut x = Scratch::recording();
    apply(&mut s, &t, &mut x, &mut Replay, free);

    let revealed: Vec<_> = x
        .events
        .drain()
        .filter_map(|e| match e {
            Event::Reveal { player, slot } if player == starting => Some(slot),
            _ => None,
        })
        .collect();
    assert_eq!(revealed.len(), 7);
    for slot in fixture_slots(first_hand) {
        assert!(revealed.contains(&slot));
        assert!(s.knows(other, slot));
        assert!(Observation::new(&s, other).card(slot).is_some());
    }

    // Opponent keeps; starting redraws seven with 0 mulligans counted.
    let d = decide(&t, &mut s);
    assert_eq!(d.player, other);
    let keep = option_choice(&d, Opt::Keep).unwrap();
    apply(&mut s, &t, &mut Scratch::new(), &mut Replay, keep);

    // Second free mulligan in a row.
    let d = decide(&t, &mut s);
    assert_eq!(d.player, starting);
    assert_eq!(s.player(starting).mulligans(), 0);
    let free = option_choice(&d, Opt::FreeMulligan).unwrap();
    apply(&mut s, &t, &mut Scratch::new(), &mut Replay, free);

    // Still 0 mulligans counted; keeping goes straight to priority with 7 cards in hand.
    let d = decide(&t, &mut s);
    assert_eq!(d.player, starting);
    assert_eq!(s.player(starting).mulligans(), 0);
    assert!(option_choice(&d, Opt::FreeMulligan).is_some());
    let keep = option_choice(&d, Opt::Keep).unwrap();
    apply(&mut s, &t, &mut Scratch::new(), &mut Replay, keep);

    let next_d = decide(&t, &mut s);
    assert_eq!(next_d.kind, DecisionKind::Priority);
    assert_eq!(s.zone(ZoneKey::Of(starting, Zone::Hand)).count(), 7);
    assert_eq!(s.check_invariants(&t), Ok(()));
}

#[test]
fn both_players_mulligan_in_one_round_with_one_shuffle_and_alternating_deal() {
    let t = shared_table(&[(40, "Test Land"), (40, "Test Spell A")]);
    let mut s = State::new(&t, 23, Profile::EXACT).unwrap();
    let _ = decide(&t, &mut s);
    apply(&mut s, &t, &mut Scratch::new(), &mut Replay, Choice(0));

    let starting = s.turn().starting_player();
    let other = starting.opponent();

    // Snapshot the 66 cards currently in the shared library and their incarnations.
    let stayed: Vec<_> = s
        .library(Seat::P0)
        .iter()
        .map(|&slot| (slot, s.object(slot).incarnation()))
        .collect();
    assert_eq!(stayed.len(), 66);

    // Both players declare a mulligan in the same round.
    let d = decide(&t, &mut s);
    assert_eq!(d.player, starting);
    let mull = option_choice(&d, Opt::Mulligan).unwrap();
    apply(&mut s, &t, &mut Scratch::new(), &mut Replay, mull);

    let d = decide(&t, &mut s);
    assert_eq!(d.player, other);
    let mull = option_choice(&d, Opt::Mulligan).unwrap();

    // Expected post-shuffle top 14 when both hands return before one shuffle.
    let mut expected = s;
    let mut sink = crate::replacement::event::Sink::off();
    expected.return_opening_hand(&mut sink, starting).unwrap();
    expected.return_opening_hand(&mut sink, other).unwrap();
    expected.shuffle_library(starting).unwrap();
    let top14: Vec<_> = expected.library_from_top(Seat::P0).take(14).collect();

    apply(&mut s, &t, &mut Scratch::new(), &mut Replay, mull);

    let starting_hand = s.zone(ZoneKey::Of(starting, Zone::Hand));
    let other_hand = s.zone(ZoneKey::Of(other, Zone::Hand));
    assert_eq!(starting_hand.count(), 7);
    assert_eq!(other_hand.count(), 7);
    for (i, &slot) in top14.iter().enumerate() {
        if i % 2 == 0 {
            assert!(starting_hand.contains(slot), "draw {i} goes to starting");
        } else {
            assert!(other_hand.contains(slot), "draw {i} goes to other");
        }
    }

    // One shuffle rebinds each card in the library once (incarnation + 1), not twice.
    for (slot, before) in stayed {
        let in_hand = s.object(slot).zone() == Zone::Hand;
        // Cards that stayed in the library were rebound once (+1); cards drawn after the shuffle
        // also changed zones from library to hand (+2 total).
        let expected_delta = if in_hand { 2 } else { 1 };
        assert_eq!(
            s.object(slot).incarnation() - before,
            expected_delta,
            "slot {slot:?} (in_hand={in_hand})"
        );
    }

    assert_eq!(s.player(starting).mulligans(), 1);
    assert_eq!(s.player(other).mulligans(), 1);
    assert_eq!(s.library(Seat::P0).len(), 66);
    assert_eq!(s.check_invariants(&t), Ok(()));
}
