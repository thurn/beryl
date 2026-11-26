//! The Memos store's tests: what ends a memo, what it tells apart, what it keeps across
//! layouts and when it is full.

use beryl_card::{CardDef, Face, Types};

use super::*;
use crate::event::Sink;
use crate::format::Format;
use crate::layout::Large;
use crate::object::Derived;
use crate::profile::Profile;
use crate::table::CardTable;
use crate::zone::Place;
use crate::{LargeSlot, Seat, Slot};

static BEAR: CardDef = CardDef {
    name: "Test Bear",
    shape: beryl_card::definition::Shape::Single,
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
    let t = GameTable::new(
        Format::CONSTRUCTED,
        &CARDS,
        [&[(40, "Test Bear")], &[(40, "Test Bear")]],
    )
    .unwrap();
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
    cache.finish(
        crate::event::Listens::NONE,
        crate::event::Replaces::NONE,
        Default::default(),
        crate::static_ability::Inputs::NONE,
        false,
        false,
    );
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
fn chosen_names_omit_unknown_names_and_phased_out_permanents() {
    let (t, mut s, slots) = bears(2);
    let card = t.index_of(&BEAR).unwrap();
    let known = ChosenValue::Name(Some(card));
    s.remember(slots[0], Memo::Chosen(known)).unwrap();
    s.remember(slots[1], Memo::Chosen(ChosenValue::Name(None)))
        .unwrap();
    derive(&t, &mut s);

    assert_eq!(
        s.chosen_names(&t).collect::<Vec<_>>(),
        [(slots[0], BEAR.name)]
    );
    s.phase_out(s.permanents_in(Bitboard::single(slots[0])))
        .unwrap();
    assert_eq!(s.chosen(slots[0]), Some(known));
    assert_eq!(s.chosen_names(&t).next(), None);
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
    let paid = CastingKinds::NONE
        .with(CastingKind::Evoke)
        .with(CastingKind::Kicker);
    small.remember(slots[0], Memo::Paid(paid)).unwrap();
    small.remember(slots[1], Memo::X(5)).unwrap();
    derive(&t, &mut small);

    let mut large: State<Large> = small.widen();
    let wide = |slot: Slot| slot.to::<LargeSlot>().unwrap();
    assert_eq!(
        large.memos(wide(slots[0])).iter().collect::<Vec<_>>(),
        [Memo::Paid(paid)]
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
        assert_eq!(small.remember(slot, KICKED), Ok(()));
        assert_eq!(small.remember(slot, Memo::X(1)), Ok(()));
        let paid = CastingKinds::NONE.with(CastingKind::Kicker);
        assert_eq!(large.remember(wide(slot), Memo::Paid(paid)), Ok(()));
        assert_eq!(large.remember(wide(slot), Memo::X(1)), Ok(()));
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

/// The `n`th card of the first player's library, exiled.
fn exiled(s: &mut State, n: usize) -> Slot {
    let slot = s.library(Seat::P0)[n];
    s.board().place(slot, Zone::Exile).unwrap();
    slot
}

#[test]
fn a_card_stays_exiled_with_the_object_that_exiled_it_and_not_a_new_one_in_its_slot() {
    let (t, mut s, slots) = bears(2);
    let (old, other) = (s.object_ref(slots[0]), s.object_ref(slots[1]));
    let (first, second, third) = (exiled(&mut s, 0), exiled(&mut s, 1), exiled(&mut s, 2));
    s.remember(first, Memo::linked(LinkKind::ExiledWith, old))
        .unwrap();
    s.remember(second, Memo::linked(LinkKind::ExiledWith, old))
        .unwrap();
    s.remember(third, Memo::linked(LinkKind::ExiledWith, other))
        .unwrap();
    derive(&t, &mut s);
    assert_eq!(
        s.linked(LinkKind::ExiledWith, old),
        [first, second].into_iter().collect()
    );
    assert_eq!(
        s.linked(LinkKind::ExiledWith, other),
        Bitboard::single(third)
    );
    assert_eq!(s.memos(first).exiled_with(), Some(old.slot));
    assert_eq!(s.memos(first).exiled_until(), None);
    assert_eq!(s.check_stores(&t), Ok(()));
    // Cards exiled with one object are alike; with another, not.
    assert_eq!(s.signature(first), s.signature(second));
    assert_ne!(s.signature(first), s.signature(third));

    // A card that leaves exile forgets what exiled it: it is a new object (CR 400.7).
    s.move_object(&mut Sink::off(), second, Zone::Hand, Place::Top)
        .unwrap();
    assert_eq!(s.linked(LinkKind::ExiledWith, old), Bitboard::single(first));

    // The source leaves: an ability of it still on the stack finds the card (CR 607.2a).
    s.move_object(&mut Sink::off(), old.slot, Zone::Graveyard, Place::Top)
        .unwrap();
    assert_eq!(s.linked(LinkKind::ExiledWith, old), Bitboard::single(first));
    derive(&t, &mut s);
    assert_eq!(s.check_stores(&t), Ok(()));

    // The card returns to the battlefield as a new object, which exiled nothing. The old
    // object's ability still finds its card; the board shows the card exiled with nothing.
    s.move_object(&mut Sink::off(), old.slot, Zone::Battlefield, Place::Top)
        .unwrap();
    let new = s.object_ref(old.slot);
    assert_eq!(s.linked(LinkKind::ExiledWith, old), Bitboard::single(first));
    assert!(s.linked(LinkKind::ExiledWith, new).is_empty());
    assert_eq!(s.memos(first).exiled_with(), None);
    assert_eq!(
        s.linked(LinkKind::ExiledWith, other),
        Bitboard::single(third)
    );
    derive(&t, &mut s);
    assert_eq!(s.check_stores(&t), Ok(()));

    // The new object exiles a card of its own, which the old one's ability never finds.
    let fourth = exiled(&mut s, 3);
    s.remember(fourth, Memo::linked(LinkKind::ExiledWith, new))
        .unwrap();
    assert_eq!(s.linked(LinkKind::ExiledWith, old), Bitboard::single(first));
    assert_eq!(
        s.linked(LinkKind::ExiledWith, new),
        Bitboard::single(fourth)
    );
    assert_eq!(s.memos(fourth).exiled_with(), Some(new.slot));
    derive(&t, &mut s);
    assert_eq!(s.check_stores(&t), Ok(()));
}

/// A link holds the low byte of its object's incarnation. The 256th object after it in the
/// same slot would share that byte, so the link ends as that object arrives rather than
/// read as a link to it.
#[test]
fn a_link_ends_before_a_later_object_in_its_slot_could_read_as_its_object() {
    let (t, mut s, slots) = bears(1);
    let old = s.object_ref(slots[0]);
    let card = exiled(&mut s, 0);
    s.remember(card, Memo::linked(LinkKind::ExiledWith, old))
        .unwrap();
    // Each round trip is two new objects; the last arrival is 256 objects after `old`.
    for _ in 0..128 {
        assert_eq!(s.linked(LinkKind::ExiledWith, old), Bitboard::single(card));
        s.move_object(&mut Sink::off(), old.slot, Zone::Graveyard, Place::Top)
            .unwrap();
        s.move_object(&mut Sink::off(), old.slot, Zone::Battlefield, Place::Top)
            .unwrap();
    }
    let now = s.object_ref(old.slot);
    assert_eq!(now.incarnation, old.incarnation + 256);
    assert!(s.linked(LinkKind::ExiledWith, now).is_empty());
    assert!(s.linked(LinkKind::ExiledWith, old).is_empty());
    assert!(s.memos(card).iter().next().is_none());
    derive(&t, &mut s);
    assert_eq!(s.check_stores(&t), Ok(()));
}

#[test]
fn what_exiled_a_card_survives_small_to_large_and_back() {
    let (t, mut small, slots) = bears(1);
    let card = exiled(&mut small, 0);
    let until = Memo::ExiledUntil(Link::to(small.object_ref(slots[0])));
    small.remember(card, until).unwrap();
    derive(&t, &mut small);
    // The permanent is on the battlefield, as the store's check holds it to be.
    assert_eq!(small.check_stores(&t), Ok(()));

    let large: State<Large> = small.widen();
    let wide = |slot: Slot| slot.to::<LargeSlot>().unwrap();
    assert_eq!(
        large.exiled_until(wide(slots[0])),
        Bitboard::single(wide(card))
    );
    assert_eq!(large.memos(wide(card)).exiled_until(), Some(wide(slots[0])));
    assert!(
        large
            .linked(LinkKind::ExiledWith, large.object_ref(wide(slots[0])))
            .is_empty()
    );
    assert_eq!(large.check_stores(&t), Ok(()));
    let back: State<Small> = large.relayout();
    assert_eq!(back, small);
}

/// A card exiled until a permanent leaves names that permanent by a [`Link`]. If the
/// permanent leaves without returning the card and a new object takes its slot, the card is
/// not exiled until the new one.
#[test]
fn a_card_exiled_until_a_permanent_leaves_is_not_exiled_until_a_new_object_in_its_slot() {
    let (t, mut s, slots) = bears(1);
    let source = slots[0];
    let card = exiled(&mut s, 0);
    let until = Memo::ExiledUntil(Link::to(s.object_ref(source)));
    s.remember(card, until).unwrap();
    assert_eq!(s.exiled_until(source), Bitboard::single(card));
    assert_eq!(s.memos(card).exiled_until(), Some(source));

    // The kernel's move returns nothing: that is the rules' (see CR 610.3).
    s.move_object(&mut Sink::off(), source, Zone::Graveyard, Place::Top)
        .unwrap();
    s.move_object(&mut Sink::off(), source, Zone::Battlefield, Place::Top)
        .unwrap();
    assert!(s.exiled_until(source).is_empty());
    assert_eq!(s.memos(card).exiled_until(), None);
    derive(&t, &mut s);
    assert_eq!(s.check_stores(&t), Ok(()));
}

/// The cards that went to exile together from a merged permanent name the permanent's own
/// card by a [`Link`]. Once that card leaves exile it is a new object (see CR 400.7), and it
/// is not one of them even when it is exiled again.
#[test]
fn cards_exiled_together_do_not_include_a_new_object_in_the_leaders_slot() {
    let (t, mut s, _) = bears(0);
    let (leader, under, other) = (exiled(&mut s, 0), exiled(&mut s, 1), exiled(&mut s, 2));
    let pile = Memo::ExiledFromPileOf(Link::to(s.object_ref(leader)));
    s.remember(under, pile).unwrap();
    s.remember(other, pile).unwrap();
    let all: Bitboard<Slot> = [leader, under, other].into_iter().collect();
    assert_eq!(s.exiled_together(under), all);
    assert_eq!(s.exiled_together(leader), all);

    s.move_object(&mut Sink::off(), leader, Zone::Graveyard, Place::Top)
        .unwrap();
    s.move_object(&mut Sink::off(), leader, Zone::Exile, Place::Top)
        .unwrap();
    let rest: Bitboard<Slot> = [under, other].into_iter().collect();
    assert_eq!(s.exiled_together(under), rest);
    assert_eq!(s.exiled_together(leader), Bitboard::single(leader));
    derive(&t, &mut s);
    assert_eq!(s.check_stores(&t), Ok(()));
}
