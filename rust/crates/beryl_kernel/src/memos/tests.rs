//! The Memos store's tests: what ends a memo, what it tells apart, what it keeps across
//! layouts and when it is full.

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

const KICKED: Memo = Memo::Paid(CastingKinds::NONE.with(CastingKind::Kicker));

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

#[test]
fn a_memo_ends_as_its_permanent_changes_zones() {
    let (t, mut s, slots) = bears(2);
    let (kicked, other) = (slots[0], slots[1]);
    derive(&t, &mut s);
    s.remember(kicked, KICKED).unwrap();
    // The layers may read a cost paid, so their results are stale; they read no X.
    assert!(s.is_derived_stale());
    derive(&t, &mut s);
    s.remember(kicked, Memo::X(3)).unwrap();
    s.remember(other, Memo::X(1)).unwrap();
    assert!(!s.is_derived_stale());
    assert!(s.paid(kicked, CastingKind::Kicker));
    assert!(!s.paid(kicked, CastingKind::Evoke));
    assert!(!s.paid(other, CastingKind::Kicker));
    assert_eq!((s.x_of(kicked), s.x_of(other)), (3, 1));
    assert_eq!(s.check_stores(&t), Ok(()));
    // A checkpoint of the board loses nothing a later turn reads.
    assert_eq!(s.stores_lingering(&t), None);

    // The object the card becomes remembers nothing, wherever it goes, back to the
    // battlefield included; the other permanent's memo stays.
    s.move_object(&mut Sink::off(), kicked, Zone::Graveyard, Place::Top)
        .unwrap();
    assert!(!s.paid(kicked, CastingKind::Kicker));
    assert_eq!((s.x_of(kicked), s.x_of(other)), (0, 1));
    assert!(!s.keyed.contains(kicked) && s.keyed.contains(other));
    assert_eq!(s.memos(kicked).iter().count(), 0);
    assert_eq!(s.check_stores(&t), Ok(()));
    s.move_object(&mut Sink::off(), kicked, Zone::Battlefield, Place::Top)
        .unwrap();
    assert!(!s.paid(kicked, CastingKind::Kicker));
    assert_eq!(s.x_of(kicked), 0);

    // With the last memo of a kind gone, the kind is absent again.
    s.move_object(&mut Sink::off(), other, Zone::Exile, Place::Top)
        .unwrap();
    assert_eq!(s.memos, Memos::empty());
    assert!(s.keyed.is_empty());
}

#[test]
fn signature_tells_two_permanents_apart_by_a_memo() {
    let (t, mut s, slots) = bears(2);
    let (a, b) = (slots[0], slots[1]);
    let same = |t: &GameTable, s: &mut State| {
        derive(t, s);
        s.signature(a) == s.signature(b)
    };
    assert!(same(&t, &mut s));
    // Nothing else about a permanent says its spell was kicked.
    s.remember(a, KICKED).unwrap();
    assert_eq!(s.object(a).flags(), s.object(b).flags());
    assert!(!same(&t, &mut s));
    s.remember(b, KICKED).unwrap();
    assert!(same(&t, &mut s));
    s.remember(a, Memo::X(2)).unwrap();
    s.remember(b, Memo::X(3)).unwrap();
    assert!(!same(&t, &mut s));
}

#[test]
fn memos_survive_small_to_large_and_back() {
    let (t, mut small, slots) = bears(2);
    let evoked = Memo::Paid(
        CastingKinds::NONE
            .with(CastingKind::Evoke)
            .with(CastingKind::Kicker),
    );
    small.remember(slots[0], evoked).unwrap();
    small.remember(slots[1], Memo::X(5)).unwrap();
    derive(&t, &mut small);

    let mut large: State<Large> = small.widen();
    let wide = |slot: Slot| slot.to::<LargeSlot>().unwrap();
    assert_eq!(
        large.memos(wide(slots[0])).iter().collect::<Vec<_>>(),
        [evoked]
    );
    // The wide game reads by kind and ends a permanent's memos as the narrow one does.
    assert!(large.paid(wide(slots[0]), CastingKind::Evoke));
    assert!(!large.paid(wide(slots[0]), CastingKind::Impending));
    assert_eq!(large.x_of(wide(slots[1])), 5);
    assert_eq!(large.check_stores(&t), Ok(()));
    let back: State<Small> = large.relayout();
    assert_eq!(back, small);
    assert_eq!(back.memos.keys(), small.keyed);

    large
        .move_object(&mut Sink::off(), wide(slots[1]), Zone::Hand, Place::Top)
        .unwrap();
    assert_eq!(large.x_of(wide(slots[1])), 0);
    assert!(large.paid(wide(slots[0]), CastingKind::Kicker));
}

#[test]
fn a_full_store_returns_full_and_a_wide_one_grows() {
    // Two memos for each of sixteen permanents fill the store.
    let (t, mut small, slots) = bears(MEMOS / 2 + 1);
    let mut large: State<Large> = small.widen();
    let wide = |slot: Slot| slot.to::<LargeSlot>().unwrap();
    let (&last, filling) = slots.split_last().unwrap();
    for &slot in filling {
        for memo in [KICKED, Memo::X(1)] {
            assert_eq!(small.remember(slot, memo), Ok(()));
            assert_eq!(large.remember(wide(slot), memo), Ok(()));
        }
    }
    assert_eq!(small.remember(last, Memo::X(4)), Err(Full));
    // What did not fit is in no index.
    assert_eq!(small.x_of(last), 0);
    assert!(!small.keyed.contains(last));
    assert_eq!(small.check_stores(&t), Ok(()));

    assert_eq!(large.remember(wide(last), Memo::X(4)), Ok(()));
    assert_eq!(large.x_of(wide(last)), 4);
    assert_eq!(
        large.fits::<Small>(&t),
        Err(Misfit::ListTooLong {
            list: "the memos",
            len: MEMOS + 1,
            capacity: MEMOS,
        })
    );
}
