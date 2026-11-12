//! Games that outgrow a width: more permanents than a byte numbers, more effects on them
//! than a fixed list holds. Each either plays right or stalls into [`Large`]; none plays on
//! wrong.

use crate::bitboard::Bitboard;
use crate::card::{FaceIx, Subtype, Types};
use crate::event::{self, Event, Reason};
use crate::ids::{Seat, SlotId};
use crate::state::{Continuous, Duration, LayerChange, Zone};
use crate::{
    GameTable, Large, Layout, Outcome, Replay, Scratch, StallReason, State, Step, layers, next,
    trigger,
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
                .place_token(owner, card, FaceIx::FRONT, false)
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

fn power_and_toughness<L: Layout>(s: &State<L>, slot: L::Slot) -> (i8, i8) {
    (s.derived(slot).power, s.derived(slot).toughness)
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
        s.start_effect_on_set(card, set_of(&bears), change, Duration::Indefinite)
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
    // land, has none. It is not left out: the game stalls and goes on in `Large`.
    let (t, mut small, p) = game();
    let bear = put(&mut small, &t, p, "Test Bear");
    let aura = put(&mut small, &t, p, "Test Reconfiguration");
    small.board().attach(aura, bear);
    let card = card_of(&small, &t, aura);
    for more in tokens(&mut small, p, card, 63) {
        small.board().attach(more, bear);
    }
    let land = put(&mut small, &t, p, "Test Land");
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
    let change = Continuous::Layer(s.object_ref(pumped), LayerChange::ModifyPt(3, 3));
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
