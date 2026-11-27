//! Games that outgrow a width: more permanents than a byte numbers, more effects on them
//! than a fixed list holds. Each either plays right or stalls into [`Large`]; none plays on
//! wrong.

use crate::bitboard::Bitboard;
use crate::card::{FaceIx, Subtype, Types};
use crate::combat::damage;
use crate::game::Game;
use crate::ids::{Seat, SlotId};
use crate::replacement::event::{self, Event, Reason, Recipient};
use crate::state::{Continuous, CounterKind, Duration, LayerChange, Shield, Subject, Zone};
use crate::{
    GameTable, Large, Layout, Outcome, Replay, Scratch, StallReason, State, Step,
    continuous::layers, next, stack::trigger,
};

use super::events::{find, game};

/// Puts a card named `name` from the player's library or hand onto the battlefield.
fn put<L: Layout>(s: &mut State<L>, t: &GameTable, player: Seat, name: &str) -> L::Slot {
    let slot = find(s, t, player, name);
    let sink = &mut Scratch::new().events;
    event::move_one(s, t, sink, slot, Zone::Battlefield, Reason::Effect).unwrap();
    slot
}

/// The game-table card of the permanent in `slot`.
fn card_of<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> L::Card {
    s.copiable(slot).record(t).0
}

/// `count` tokens of `card` on the battlefield under `owner`, in the order they entered.
fn tokens<L: Layout>(s: &mut State<L>, owner: Seat, card: L::Card, count: usize) -> Vec<L::Slot> {
    (0..count)
        .map(|_| {
            s.board()
                .place_token(owner, card, FaceIx::FRONT, &[])
                .unwrap()
        })
        .collect()
}

fn set_of<S: SlotId>(slots: &[S]) -> Bitboard<S> {
    slots.iter().copied().collect()
}

fn refresh<L: Layout>(s: &mut State<L>, t: &GameTable) {
    layers::refresh(s, t, &mut Scratch::new().events).unwrap();
}

fn power_and_toughness<L: Layout>(s: &State<L>, slot: L::Slot) -> (L::Pt, L::Pt) {
    (s.derived(slot).power, s.derived(slot).toughness)
}

/// Each shield prevents one damage ([CR 615.7]): "Preventing 1 damage reduces the
/// remaining shield by 1." Seventeen shields require `Large` to consider and apply them all.
///
/// [CR 615.7]: https://yawgatog.com/resources/magic-rules/#R6157
#[test]
fn damage_uses_more_than_sixteen_shields_in_large() {
    let (t, mut small, player) = game();
    let source = put(&mut small, &t, player, "Test Bear");
    let card = card_of(&small, &t, source);
    let shield = Shield {
        left: Some(1),
        from: None,
    };
    for _ in 0..17 {
        small
            .start_effect(
                card,
                Continuous::Prevent(Subject::Player(player), shield),
                Duration::EndOfTurn,
            )
            .unwrap();
    }
    let mut large = small.widen();
    let reference = small.object_ref(source);
    let mut scratch = Scratch::new();
    assert_eq!(
        damage::deal(
            &mut Game::new(&mut small, &t, &mut scratch.events),
            reference,
            Recipient::Player(player),
            20,
            false,
        ),
        Err(crate::fixed::Full)
    );

    let reference = large.object_ref(source.to().unwrap());
    let mut scratch = Scratch::new();
    assert_eq!(
        damage::deal(
            &mut Game::new(&mut large, &t, &mut scratch.events),
            reference,
            Recipient::Player(player),
            20,
            false,
        ),
        Ok(3)
    );
    assert!(!large.has_prevention_shields());
}

#[test]
fn every_member_of_three_locked_in_sets_gets_all_three_effects() {
    // One effect is one entry of the layers' list, whatever it changes: 360 changes to
    // permanents, in a list of 192.
    let (t, mut s, p) = game();
    let bear = put(&mut s, &t, p, "Test Bear");
    let card = card_of(&s, &t, bear);
    let bears = tokens(&mut s, p, card, 120);
    for _ in 0..3 {
        let change = LayerChange::ModifyPt(1, 1);
        s.start_effect_on_set(
            card,
            s.permanents_in(set_of(&bears)),
            &[change],
            Duration::Indefinite,
        )
        .unwrap();
    }
    refresh(&mut s, &t);
    for &slot in &bears {
        assert_eq!(power_and_toughness(&s, slot), (5, 5));
    }
    assert_eq!(power_and_toughness(&s, bear), (2, 2), "not of the sets");
}

#[test]
fn more_static_abilities_than_the_list_holds_stall_into_large_and_apply_there() {
    // 64 Auras of three static abilities each fill the list's 192 places; the 65th, on the
    // land, has none. It is not left out: the game stalls and goes on in `Large`. (In `Small`
    // the seventeenth Aura's grant of crew already has no row in the table of grants, which
    // stalls the game as well.) The land enters before the tokens, as its move refreshes the
    // layers.
    let (t, mut small, p) = game();
    let bear = put(&mut small, &t, p, "Test Bear");
    let aura = put(&mut small, &t, p, "Test Reconfiguration");
    small.board().attach(aura, bear);
    let land = put(&mut small, &t, p, "Test Land");
    let card = card_of(&small, &t, aura);
    for more in tokens(&mut small, p, card, 63) {
        small.board().attach(more, bear);
    }
    let last = tokens(&mut small, p, card, 1)[0];
    small.board().attach(last, land);
    let mut large = small.widen();

    assert_eq!(
        next(&mut small, &t, &mut Scratch::new(), &mut Replay),
        Step::Over(Outcome::Stalled(StallReason::Capacity))
    );
    let step = next(&mut large, &t, &mut Scratch::new(), &mut Replay);
    assert!(matches!(step, Step::Decide(_)), "the game goes on");
    for slot in [bear, land] {
        let derived = large.derived(slot.to().unwrap());
        assert_eq!(derived.types, Types::ARTIFACT);
        assert!(derived.subtypes.contains(t.subtypes(), Subtype::Vehicle));
    }
}

#[test]
fn an_effect_on_the_301st_permanent_changes_it_and_no_other() {
    let (t, small, p) = game();
    let mut s: State<Large> = small.widen();
    let bear = put(&mut s, &t, p, "Test Bear");
    let card = card_of(&s, &t, bear);
    tokens(&mut s, p, card, 300);
    assert_eq!(s.battlefield().len(), 301);
    let pumped = s.battlefield()[300];
    let change = Continuous::Layer(s.permanent_at(pumped).unwrap(), LayerChange::ModifyPt(3, 3));
    s.start_effect(card, change, Duration::Indefinite).unwrap();
    refresh(&mut s, &t);
    for (position, &slot) in s.battlefield().iter().enumerate() {
        let expected = if slot == pumped { (5, 5) } else { (2, 2) };
        assert_eq!(power_and_toughness(&s, slot), expected, "at {position}");
    }

    // A static ability reaches as far: an Aura on the permanent at position 299.
    let aura = put(&mut s, &t, p, "Test Reconfiguration");
    let enchanted = s.battlefield()[299];
    s.board().attach(aura, enchanted);
    refresh(&mut s, &t);
    for (position, &slot) in s.battlefield().iter().enumerate() {
        let expected = match slot {
            slot if slot == enchanted => Types::ARTIFACT,
            slot if slot == aura => Types::ENCHANTMENT,
            _ => Types::CREATURE,
        };
        assert_eq!(s.derived(slot).types, expected, "at {position}");
    }
}

#[test]
fn a_trigger_counting_three_hundred_objects_reads_no_wrapped_count() {
    // The amount a trigger carries is a byte. 300 of anything is 255 there, not 300's low
    // byte, 44.
    let (t, small, p) = game();
    let mut s: State<Large> = small.widen();
    put(&mut s, &t, p, "Test Adeline");
    put(&mut s, &t, p, "Test Staff");
    let card = s.card_behind(find(&s, &t, p, "Test Bear")).unwrap();
    let bears = set_of(&tokens(&mut s, p, card, 300));
    refresh(&mut s, &t);
    let waiting = s.triggers().len();

    // "Whenever you attack": the attackers declared.
    let attack = Event::Attack {
        player: p,
        attackers: bears,
    };
    trigger::match_event(&mut s, &t, &attack).unwrap();
    // "Whenever you create one or more creature tokens": the tokens created.
    let created = Event::TokensCreated {
        controller: p,
        tokens: bears,
    };
    trigger::match_event(&mut s, &t, &created).unwrap();
    let amounts: Vec<u8> = (s.triggers()[waiting..].iter())
        .map(|trigger| trigger.info.amount)
        .collect();
    assert_eq!(amounts, [255, 255]);
}

#[test]
fn power_reaching_128_through_counters_stalls_in_small_and_is_exact_in_large() {
    let (t, mut small, p) = game();
    let bear = put(&mut small, &t, p, "Test Bear");
    let object = small.current(small.object_ref(bear)).unwrap();
    let _ = small
        .add_counters(object, CounterKind::PlusOne, 126, true)
        .unwrap();
    let mut large: State<Large> = small.widen();
    assert_eq!(
        next(&mut small, &t, &mut Scratch::new(), &mut Replay),
        Step::Over(Outcome::Stalled(StallReason::Capacity))
    );
    let step = next(&mut large, &t, &mut Scratch::new(), &mut Replay);
    assert!(matches!(step, Step::Decide(_)), "the game goes on");
    assert_eq!(power_and_toughness(&large, bear.to().unwrap()), (128, 128));

    // Negative overflow past `i8::MIN` (-128) widens the same way.
    let (t, mut small, p) = game();
    let bear = put(&mut small, &t, p, "Test Bear");
    let object = small.current(small.object_ref(bear)).unwrap();
    let _ = small
        .add_counters(object, CounterKind::MinusOne, 131, true)
        .unwrap();
    let mut large: State<Large> = small.widen();
    assert_eq!(
        layers::refresh(&mut small, &t, &mut Scratch::new().events),
        Err(crate::fixed::Full)
    );
    refresh(&mut large, &t);
    assert_eq!(
        power_and_toughness(&large, bear.to().unwrap()),
        (-129, -129)
    );
}

#[test]
fn power_reaching_128_through_a_pump_stalls_in_small_and_is_exact_in_large() {
    let (t, mut small, p) = game();
    let bear = put(&mut small, &t, p, "Test Bear");
    let card = card_of(&small, &t, bear);
    let perm = small.permanent_at(bear).unwrap();
    for _ in 0..2 {
        let change = Continuous::Layer(perm, LayerChange::ModifyPt(63, 0));
        small
            .start_effect(card, change, Duration::EndOfTurn)
            .unwrap();
    }
    let mut large: State<Large> = small.widen();
    assert_eq!(
        next(&mut small, &t, &mut Scratch::new(), &mut Replay),
        Step::Over(Outcome::Stalled(StallReason::Capacity))
    );
    let step = next(&mut large, &t, &mut Scratch::new(), &mut Replay);
    assert!(matches!(step, Step::Decide(_)), "the game goes on");
    assert_eq!(power_and_toughness(&large, bear.to().unwrap()), (128, 2));
}

#[test]
fn counters_reaching_256_stall_in_small_and_are_exact_in_large() {
    let (t, mut small, p) = game();
    let bear = put(&mut small, &t, p, "Test Bear");
    let object = small.current(small.object_ref(bear)).unwrap();
    let _ = small
        .add_counters(object, CounterKind::Lore, 200, false)
        .unwrap();
    let mut large: State<Large> = small.widen();

    assert_eq!(
        small.add_counters(object, CounterKind::Lore, 56, false),
        Err(crate::fixed::Full)
    );
    let large_bear = bear.to().unwrap();
    let large_obj = large.current(large.object_ref(large_bear)).unwrap();
    let _ = large
        .add_counters(large_obj, CounterKind::Lore, 56, false)
        .unwrap();
    assert_eq!(large.counters_of(large_bear, CounterKind::Lore), 256);
}

#[test]
fn marked_damage_reaching_256_stalls_in_small_and_is_exact_in_large() {
    let (t, mut small, p) = game();
    let bear = put(&mut small, &t, p, "Test Bear");
    let perm = small.permanent_at(bear).unwrap();
    let source = small.object_ref(bear);
    let _ = small.mark_damage(source, perm, 200, false).unwrap();
    let mut large: State<Large> = small.widen();

    assert_eq!(
        small.mark_damage(source, perm, 56, false),
        Err(crate::fixed::Full)
    );

    // Give the creature 300 toughness in `Large` so 256 marked damage is nonlethal.
    let large_bear = bear.to().unwrap();
    let card = card_of(&large, &t, large_bear);
    let large_perm = large.permanent_at(large_bear).unwrap();
    let change = Continuous::Layer(large_perm, LayerChange::ModifyPt(0, 298));
    large
        .start_effect(card, change, Duration::EndOfTurn)
        .unwrap();
    refresh(&mut large, &t);
    assert_eq!(power_and_toughness(&large, large_bear), (2, 300));
    let large_perm = large.permanent_at(large_bear).unwrap();
    let _ = large
        .mark_damage(large.object_ref(large_bear), large_perm, 56, false)
        .unwrap();
    assert_eq!(large.object(large_bear).damage(), 256);
    let step = next(&mut large, &t, &mut Scratch::new(), &mut Replay);
    assert!(matches!(step, Step::Decide(_)), "the game goes on");
    assert!(large.object(large_bear).on_battlefield());
    assert_eq!(large.object(large_bear).damage(), 256);
}

#[test]
fn mana_pool_reaching_256_stalls_in_small_and_is_exact_in_large() {
    let (t, mut small, p) = game();
    let _ = small
        .add_mana(p, None, crate::card::ManaColor::Blue, 200)
        .unwrap();
    let mut large: State<Large> = small.widen();

    assert_eq!(
        small.add_mana(p, None, crate::card::ManaColor::Blue, 56),
        Err(crate::fixed::Full)
    );
    let _ = large
        .add_mana(p, None, crate::card::ManaColor::Blue, 56)
        .unwrap();
    assert_eq!(large.player(p).mana()[crate::card::ManaColor::Blue], 256);
    let _ = t;
}

#[test]
fn life_past_i16_max_gained_in_steps_stalls_in_small_and_is_exact_in_large() {
    let (_, mut small, p) = game();
    // Starting life is 20; gain 200 life 163 times to reach 32,620, then 140 to reach 32,760.
    for _ in 0..163 {
        let _ = small.gain_life(p, 200).unwrap();
    }
    let _ = small.gain_life(p, 140).unwrap();
    assert_eq!(small.player(p).life(), 32_760);
    let mut large: State<Large> = small.widen();

    assert_eq!(small.gain_life(p, 10), Err(crate::fixed::Full));
    let _ = large.gain_life(p, 10).unwrap();
    assert_eq!(large.player(p).life(), 32_770);
}

/// A `Small` game whose permanents were granted abilities, widened, is the game its `Large`
/// recompute gives: the same table of grants in the same order, and the same bits.
#[test]
fn a_game_with_grants_widens_to_the_large_one() {
    let (mut g, _) = super::grants::grunt();
    super::grants::give(&mut g, "Test Banner", 3);
    super::grants::give(&mut g, "Test Pennant", 1);
    super::grants::give(&mut g, "Test Grunt", 2);
    refresh(&mut g.s, &g.t);
    assert_eq!(g.s.grant_rows().len(), 4);
    let widened = g.s.widen();
    let mut large = widened.clone();
    large.forget_derived();
    refresh(&mut large, &g.t);
    assert_eq!(large.first_difference(&widened), None);
}

#[test]
fn a_small_state_with_quantities_widens_to_the_large_one() {
    fn build<L: Layout>(t: &GameTable, mut s: State<L>, p: Seat) -> State<L> {
        let bear = put(&mut s, t, p, "Test Bear");
        let card = card_of(&s, t, bear);
        let perm = s.permanent_at(bear).unwrap();
        let change = Continuous::Layer(perm, LayerChange::ModifyPt(L::Pt::from(3), L::Pt::from(2)));
        s.start_effect(card, change, Duration::EndOfTurn).unwrap();
        let object = s.current(s.object_ref(bear)).unwrap();
        let _ = s
            .add_counters(object, CounterKind::PlusOne, 2, true)
            .unwrap();
        let _ = s.add_counters(object, CounterKind::Lore, 3, false).unwrap();
        refresh(&mut s, t);
        let perm = s.permanent_at(bear).unwrap();
        let _ = s.mark_damage(s.object_ref(bear), perm, 1, false).unwrap();
        let _ = s
            .add_mana(p, None, crate::card::ManaColor::Blue, 4)
            .unwrap();
        let _ = s.gain_life(p, 5).unwrap();
        let _ = s.lose_life(s.next_player(p), 3).unwrap();
        refresh(&mut s, t);
        s
    }

    let (t, small, p) = game();
    let large = small.widen();
    let small = build(&t, small, p);
    let large = build(&t, large, p);
    let widened = small.widen();
    assert_eq!(widened.first_difference(&large), None);
    assert_eq!(widened, large);
}

#[test]
fn a_large_state_with_200_power_does_not_narrow() {
    let (t, small, p) = game();
    let mut large: State<Large> = small.widen();
    let bear = put(&mut large, &t, p, "Test Bear");
    let card = card_of(&large, &t, bear);
    let perm = large.permanent_at(bear).unwrap();
    for _ in 0..2 {
        let change = Continuous::Layer(perm, LayerChange::ModifyPt(99, 0));
        large
            .start_effect(card, change, Duration::EndOfTurn)
            .unwrap();
    }
    refresh(&mut large, &t);
    assert_eq!(power_and_toughness(&large, bear), (200, 2));
    let err = large.try_relayout::<beryl_kernel::Small>(&t).unwrap_err();
    assert_eq!(
        err,
        beryl_kernel::Misfit::ValueOutOfRange {
            quantity: "power",
            value: 200,
            min: i64::from(i8::MIN),
            max: i64::from(i8::MAX),
        }
    );
    assert!(err.to_string().contains("power"));
    assert!(err.to_string().contains("200"));
}
