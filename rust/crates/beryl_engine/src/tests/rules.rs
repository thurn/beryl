//! State-based actions, shuffles, filters and mana abilities on hand-built positions.

use super::{DUAL, LANDS, MIXED, begin, decide, table};
use crate::card::{ManaColor, Types};
use crate::effect::EffectCtx;
use crate::filter::{Filter, Whose};
use crate::state::{ObjectFlags, Zone, opponent};
use crate::{
    Choice, DecisionKind, Decklist, Outcome, Profile, Replay, Scratch, State, Step, apply, next,
    zone,
};

const LEGENDS: &Decklist = &[(40, "Test Legend")];

/// Two legends from the starting player's library onto the battlefield.
fn two_legends(s: &mut State) -> (u8, u8, u8) {
    let p = s.turn().starting_player;
    let lib = s.library(p);
    let (a, b) = (lib[0], lib[1]);
    zone::move_to(s, a, Zone::Battlefield).unwrap();
    zone::move_to(s, b, Zone::Battlefield).unwrap();
    (p, a, b)
}

#[test]
fn the_legend_rule_asks_which_to_keep_when_the_two_differ() {
    let t = table([LEGENDS, LEGENDS]);
    let mut s = State::new(&t, 1, Profile::EXACT);
    begin(&t, &mut s);
    let (p, a, b) = two_legends(&mut s);
    s.object_mut(b).flags.set(ObjectFlags::TAPPED, true);
    let d = decide(&t, &mut s);
    assert_eq!(
        (d.kind, d.player, d.len()),
        (DecisionKind::LegendRule, p, 2)
    );
    let keep_b = (0..2)
        .find(|&i| d.candidates.object(Choice(i)) == Some(b))
        .unwrap();
    apply(&mut s, &t, &mut Scratch::new(), &mut Replay, Choice(keep_b));
    let d = decide(&t, &mut s);
    assert_eq!(d.kind, DecisionKind::Priority);
    assert!(s.battlefield().contains(&b));
    assert!(!s.battlefield().contains(&a));
    assert_eq!(s.object(a).zone, Zone::Graveyard);
    assert_eq!(s.graveyard(p).last(), Some(&a));
}

#[test]
fn the_legend_rule_keeps_one_of_two_interchangeable_legends_silently() {
    let t = table([LEGENDS, LEGENDS]);
    let mut s = State::new(&t, 1, Profile::EXACT);
    begin(&t, &mut s);
    let (p, a, b) = two_legends(&mut s);
    assert_eq!(decide(&t, &mut s).kind, DecisionKind::Priority);
    assert_eq!(s.zone(p, Zone::Battlefield).count(), 1);
    assert_eq!(s.zone(p, Zone::Graveyard).count(), 1);
    assert!(s.battlefield().contains(&a.min(b)));
}

#[test]
fn the_legend_rule_tells_apart_legends_with_different_owners() {
    let t = table([LEGENDS, LEGENDS]);
    let mut s = State::new(&t, 1, Profile::EXACT);
    begin(&t, &mut s);
    let p = s.turn().starting_player;
    let mine = s.library(p)[0];
    let stolen = s.library(opponent(p))[0];
    zone::move_to(&mut s, mine, Zone::Battlefield).unwrap();
    zone::move_to(&mut s, stolen, Zone::Battlefield).unwrap();
    s.object_mut(stolen).controller = p;
    // The one put into the graveyard goes to its owner's: a different game.
    let d = decide(&t, &mut s);
    assert_eq!(
        (d.kind, d.player, d.len()),
        (DecisionKind::LegendRule, p, 2)
    );
}

#[test]
fn a_card_put_into_a_hand_is_known_to_its_owner_alone() {
    let t = table([MIXED, MIXED]);
    let mut s = State::new(&t, 3, Profile::EXACT);
    begin(&t, &mut s);
    let p = s.turn().starting_player;
    let q = opponent(p);
    let from_library = s.library(p)[0];
    assert!(!s.knows(p, from_library));
    zone::move_to(&mut s, from_library, Zone::Hand).unwrap();
    assert!(s.knows(p, from_library) && !s.knows(q, from_library));
    // A card returned from the battlefield was seen by everyone on its way.
    let from_battlefield = s.library(p)[0];
    zone::move_to(&mut s, from_battlefield, Zone::Battlefield).unwrap();
    zone::move_to(&mut s, from_battlefield, Zone::Hand).unwrap();
    assert!(s.knows(p, from_battlefield) && s.knows(q, from_battlefield));
}

#[test]
fn a_player_at_zero_life_loses_and_two_at_once_draw() {
    let t = table([LANDS, LANDS]);
    for (lives, outcome) in [
        ([0, 20], Outcome::Win(1)),
        ([20, -3], Outcome::Win(0)),
        ([0, 0], Outcome::Draw),
    ] {
        let mut s = State::new(&t, 1, Profile::EXACT);
        begin(&t, &mut s);
        s.player_mut(0).life = lives[0];
        s.player_mut(1).life = lives[1];
        assert_eq!(
            next(&mut s, &t, &mut Scratch::new(), &mut Replay),
            Step::Over(outcome)
        );
    }
}

#[test]
fn a_shuffle_rebinds_identities_and_clears_knowledge() {
    let t = table([MIXED, MIXED]);
    let mut s = State::new(&t, 9, Profile::EXACT);
    begin(&t, &mut s);
    let p = s.turn().starting_player;
    let hand: Vec<u8> = s.zone(p, Zone::Hand).iter().collect();
    for &slot in &hand {
        assert!(s.knows(p, slot));
        zone::move_to(&mut s, slot, Zone::Library).unwrap();
    }
    let before: Vec<(u8, u8, u16)> = s
        .library(p)
        .iter()
        .map(|&slot| (slot, s.object(slot).card, s.object(slot).incarnation))
        .collect();
    zone::shuffle_library(&mut s, p);
    let after = |slot: u8| (s.object(slot).card, s.object(slot).incarnation);
    let mut cards_before: Vec<u8> = before.iter().map(|b| b.1).collect();
    let mut cards_after: Vec<u8> = before.iter().map(|b| after(b.0).0).collect();
    assert_ne!(cards_before, cards_after, "identities were dealt anew");
    cards_before.sort_unstable();
    cards_after.sort_unstable();
    assert_eq!(cards_before, cards_after, "the same cards");
    for &(slot, _, incarnation) in &before {
        assert!(!s.knows(0, slot) && !s.knows(1, slot));
        assert_eq!(after(slot).1, incarnation.wrapping_add(1), "a new object");
    }
    let mut slots: Vec<u8> = s.library(p).to_vec();
    slots.sort_unstable();
    let mut slots_before: Vec<u8> = before.iter().map(|b| b.0).collect();
    slots_before.sort_unstable();
    assert_eq!(slots, slots_before);
}

#[test]
fn filters_select_by_type_and_owner() {
    let t = table([MIXED, MIXED]);
    let mut s = State::new(&t, 9, Profile::EXACT);
    begin(&t, &mut s);
    let p = s.turn().starting_player;
    let hand = s.zone(p, Zone::Hand);
    let lands = Filter::Types(Types::LAND).select(&s, &t, hand, p);
    let others = Filter::Not(&Filter::Types(Types::LAND)).select(&s, &t, hand, p);
    assert_eq!((lands | others), hand);
    assert!((lands & others).is_empty());
    for slot in lands.iter() {
        assert_eq!(t.def(s.object(slot).card).name, "Test Land");
    }
    assert_eq!(Filter::Whose(Whose::You).select(&s, &t, hand, p), hand);
    assert!(
        Filter::Whose(Whose::Opponent)
            .select(&s, &t, hand, p)
            .is_empty()
    );
    assert_eq!(
        Filter::Whose(Whose::Opponent).select(&s, &t, hand, opponent(p)),
        hand
    );
}

#[test]
fn a_dual_land_has_a_mana_ability_for_each_basic_land_type() {
    let t = table([&[(40, "Test Dual")], LANDS]);
    let mut s = State::new(&t, 1, Profile::EXACT);
    let source = s.object_ref(0);
    for (ability, color) in DUAL
        .front()
        .abilities
        .iter()
        .zip([ManaColor::White, ManaColor::Blue])
    {
        let mut ctx = EffectCtx {
            state: &mut s,
            source,
            controller: 0,
            ability,
            mana: color,
        };
        (ability.body)(&mut ctx).unwrap();
    }
    assert_eq!(s.player(0).mana, [1, 1, 0, 0, 0, 0]);
}
