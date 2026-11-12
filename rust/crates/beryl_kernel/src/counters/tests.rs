//! The counters' tests: each kind its own count, what ends a row, what it tells apart, what
//! it keeps across layouts and when the table is full.

use beryl_card::{CardDef, Face, Types};

use super::*;
use crate::event::Sink;
use crate::layout::Large;
use crate::object::Derived;
use crate::profile::Profile;
use crate::table::CardTable;
use crate::zone::Place;
use crate::{LargeSlot, Seat, Slot};

static BEAR: CardDef = CardDef {
    name: "Test Bear",
    faces: &[Face {
        types: Types::CREATURE,
        power: Some(2),
        toughness: Some(2),
        ..Face::EMPTY
    }],
    tokens: &[],
};

static CARDS: CardTable = CardTable::new(&[&BEAR]);

/// A game of bears, and `n` of the first player's on the battlefield.
fn bears(n: usize) -> (GameTable, State, Vec<Slot>) {
    let t = GameTable::new(&CARDS, [&[(40, "Test Bear")], &[(40, "Test Bear")]]).unwrap();
    let mut s = State::new(&t, 7, Profile::EXACT).unwrap();
    let slots: Vec<Slot> = s.library(Seat::P0)[..n].to_vec();
    for &slot in &slots {
        s.board().place(slot, Zone::Battlefield).unwrap();
    }
    (t, s, slots)
}

/// Stores layer results with no continuous effect, as the rules' recompute would.
fn derive<L: Layout>(t: &GameTable, s: &mut State<L>) {
    let Some(mut cache) = s.rederive() else {
        return;
    };
    for i in 0..cache.battlefield().len() {
        let what = cache.copiable(cache.battlefield()[i]);
        cache.results()[i] = Derived {
            power: what.power(t),
            toughness: what.toughness(t),
            types: what.face(t).types,
            ..Derived::default()
        };
    }
    cache.finish(0, Default::default());
}

/// The first two kinds of the vocabulary's list that no object keeps in a field.
fn table_kinds() -> (CounterKind, CounterKind) {
    let mut kinds = CounterKind::ALL
        .iter()
        .copied()
        .filter(|&kind| Counters::NONE.field(kind).is_none());
    (kinds.next().unwrap(), kinds.next().unwrap())
}

fn current(s: &State, slot: Slot) -> ObjectRef {
    ObjectRef {
        slot,
        incarnation: s.object(slot).incarnation(),
    }
}

/// CR 122.1: counters of different names are different counters, so a permanent has any
/// number of kinds at once and each count changes alone.
#[test]
fn an_object_holds_each_kind_with_its_own_count() {
    let (t, mut s, slots) = bears(1);
    let bear = current(&s, slots[0]);
    let (first, second) = table_kinds();
    derive(&t, &mut s);
    assert_eq!(
        s.add_counters(bear, second, 2, false),
        Ok(Event::CountersAdded {
            object: bear,
            kind: second,
            amount: 2
        })
    );
    // The layers' results are stale only where the rules said the layers read the kind.
    assert!(!s.is_derived_stale());
    let _ = s.add_counters(bear, first, 3, false).unwrap();
    let _ = s.add_counters(bear, CounterKind::PlusOne, 1, true).unwrap();
    assert!(s.is_derived_stale());
    let _ = s.add_counters(bear, second, 1, false).unwrap();
    assert_eq!(
        (
            s.counters_of(bear.slot, first),
            s.counters_of(bear.slot, second),
            s.counters_of(bear.slot, CounterKind::PlusOne),
            s.counters_of(bear.slot, CounterKind::Loyalty),
        ),
        (3, 3, 1, 0)
    );
    // Listed in the order of the vocabulary's list, not the order they were put on.
    let mut listed = vec![(second, 3), (CounterKind::PlusOne, 1), (first, 3)];
    listed.sort_by_key(|&(kind, _)| kind as usize);
    assert_eq!(s.counters(bear.slot).iter().collect::<Vec<_>>(), listed);
    assert_eq!(s.check_stores(&t), Ok(()));
    // A checkpoint of the board records a permanent's counters with it.
    assert_eq!(s.stores_lingering(&t), None);

    assert_eq!(
        s.remove_counters(bear, first, 2, false),
        Event::CountersRemoved {
            object: bear,
            kind: first,
            amount: 2
        }
    );
    assert_eq!(
        (
            s.counters_of(bear.slot, first),
            s.counters_of(bear.slot, second)
        ),
        (1, 3)
    );
    // A row ends with its last counter, and the object's place among the keyed slots with
    // its last row.
    let _ = s.remove_counters(bear, first, 1, false);
    assert_eq!(s.counters_of(bear.slot, first), 0);
    assert!(s.keyed.contains(bear.slot));
    let _ = s.remove_counters(bear, second, 3, false);
    assert_eq!(s.counters, CounterTable::empty());
    assert!(s.keyed.is_empty());
    assert_eq!(s.counters_of(bear.slot, CounterKind::PlusOne), 1);
    assert_eq!(s.check_stores(&t), Ok(()));
}

/// CR 122.2: "Counters on an object are not retained if that object moves from one zone to
/// another."
#[test]
fn counters_end_as_their_object_changes_zones() {
    let (t, mut s, slots) = bears(2);
    let (moved, stays) = (slots[0], slots[1]);
    let (first, second) = table_kinds();
    for kind in [first, second, CounterKind::MinusOne] {
        let _ = s.add_counters(current(&s, moved), kind, 2, false).unwrap();
    }
    let _ = s.add_counters(current(&s, stays), first, 4, false).unwrap();

    s.move_object(&mut Sink::off(), moved, Zone::Graveyard, Place::Top)
        .unwrap();
    assert_eq!(s.counters(moved).iter().count(), 0);
    assert!(!s.keyed.contains(moved) && s.keyed.contains(stays));
    assert_eq!(s.counters_of(stays, first), 4);
    assert_eq!(s.check_stores(&t), Ok(()));
    // The object the card returns as is new (CR 400.7).
    s.move_object(&mut Sink::off(), moved, Zone::Battlefield, Place::Top)
        .unwrap();
    assert_eq!(s.counters(moved).iter().count(), 0);
}

#[test]
fn signature_tells_two_permanents_apart_by_a_counter() {
    let (t, mut s, slots) = bears(2);
    let (a, b) = (slots[0], slots[1]);
    let (first, second) = table_kinds();
    let same = |t: &GameTable, s: &mut State| {
        derive(t, s);
        s.signature(a) == s.signature(b)
    };
    let add = |s: &mut State, slot: Slot, kind: CounterKind, n: u8| {
        let _ = s.add_counters(current(s, slot), kind, n, false).unwrap();
    };
    assert!(same(&t, &mut s));
    // Nothing else about a permanent says it has the counter.
    add(&mut s, a, first, 1);
    assert_eq!(s.object(a).counters(), s.object(b).counters());
    assert!(!same(&t, &mut s));
    add(&mut s, b, second, 1);
    assert!(!same(&t, &mut s));
    // The same counters, put on in another order.
    add(&mut s, a, second, 1);
    add(&mut s, b, first, 1);
    assert!(same(&t, &mut s));
    add(&mut s, b, first, 1);
    assert!(!same(&t, &mut s));
}

#[test]
fn counters_survive_small_to_large_and_back() {
    let (t, mut small, slots) = bears(2);
    let (first, second) = table_kinds();
    let _ = small
        .add_counters(current(&small, slots[0]), first, 2, false)
        .unwrap();
    let _ = small
        .add_counters(current(&small, slots[0]), second, 5, false)
        .unwrap();
    let _ = small
        .add_counters(current(&small, slots[1]), CounterKind::Loyalty, 3, false)
        .unwrap();
    derive(&t, &mut small);

    let mut large: State<Large> = small.widen();
    let wide = |slot: Slot| slot.to::<LargeSlot>().unwrap();
    assert_eq!(
        large.counters(wide(slots[0])).iter().collect::<Vec<_>>(),
        [(first, 2), (second, 5)]
    );
    assert_eq!(large.counters_of(wide(slots[1]), CounterKind::Loyalty), 3);
    assert_eq!(large.check_stores(&t), Ok(()));
    let back: State<Small> = large.relayout();
    assert_eq!(back, small);

    // The wide game ends a permanent's counters as the narrow one does.
    large
        .move_object(&mut Sink::off(), wide(slots[0]), Zone::Hand, Place::Top)
        .unwrap();
    assert_eq!(large.counters_of(wide(slots[0]), second), 0);
}

#[test]
fn a_full_table_returns_full_and_a_wide_one_grows() {
    // Two kinds on each of sixteen permanents fill the table.
    let (t, mut small, slots) = bears(COUNTERS / 2 + 1);
    let mut large: State<Large> = small.widen();
    let wide = |slot: Slot| slot.to::<LargeSlot>().unwrap();
    let (first, second) = table_kinds();
    let (&last, filling) = slots.split_last().unwrap();
    for &slot in filling {
        for kind in [first, second] {
            let object = current(&small, slot);
            assert!(small.add_counters(object, kind, 1, false).is_ok());
            let object = ObjectRef {
                slot: wide(slot),
                incarnation: object.incarnation,
            };
            assert!(large.add_counters(object, kind, 1, false).is_ok());
        }
    }
    let object = current(&small, last);
    assert_eq!(small.add_counters(object, first, 1, false), Err(Full));
    // What did not fit is nowhere, and a kind kept in a field needs no room.
    assert_eq!(small.counters_of(last, first), 0);
    assert!(!small.keyed.contains(last));
    assert!(
        small
            .add_counters(object, CounterKind::PlusOne, 1, false)
            .is_ok()
    );
    // Nor do more counters of a kind an object already has.
    assert!(
        small
            .add_counters(current(&small, filling[0]), first, 1, false)
            .is_ok()
    );
    assert_eq!(small.check_stores(&t), Ok(()));

    let object = ObjectRef {
        slot: wide(last),
        incarnation: object.incarnation,
    };
    assert!(large.add_counters(object, first, 1, false).is_ok());
    assert_eq!(large.counters_of(wide(last), first), 1);
    assert_eq!(
        large.fits::<Small>(&t),
        Err(Misfit::ListTooLong {
            list: "the counters",
            len: COUNTERS + 1,
            capacity: COUNTERS,
        })
    );
}

/// CR 608.2h: what an object's counters were is still known once it has moved.
#[test]
fn the_counters_an_object_held_outlive_it() {
    let (_, mut s, slots) = bears(1);
    let (first, second) = table_kinds();
    let _ = s
        .add_counters(current(&s, slots[0]), second, 2, false)
        .unwrap();
    let _ = s
        .add_counters(current(&s, slots[0]), CounterKind::MinusOne, 1, false)
        .unwrap();
    let held = s.counters(slots[0]).held();
    s.move_object(&mut Sink::off(), slots[0], Zone::Exile, Place::Top)
        .unwrap();
    assert_eq!(
        (
            held.get(first),
            held.get(second),
            held.get(CounterKind::MinusOne)
        ),
        (0, 2, 1)
    );
}
