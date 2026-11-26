//! What the storage of the libraries and graveyards keeps apart, and what it hashes.

use std::hash::{DefaultHasher, Hash, Hasher};

use super::{LargePiles, Piles, SmallPiles};
use crate::fixed::{Fixed, Full};
use crate::ids::{LargeSlot, Slot, SlotId};

fn hash(value: &impl Hash) -> u64 {
    let mut h = DefaultHasher::new();
    value.hash(&mut h);
    h.finish()
}

#[test]
fn a_per_player_list_holds_one_deck_and_leaves_the_other_alone() {
    let mut piles = SmallPiles::default();
    for i in 0..Slot::DECK_SLOTS {
        piles.push(0, Slot::from_index(i)).unwrap();
    }
    assert_eq!(piles.push(0, Slot::new(200)), Err(Full));
    assert_eq!(piles.insert(0, 0, Slot::new(200)), Err(Full));
    piles.push(1, Slot::new(100)).unwrap();
    assert_eq!(piles.list(0).len(), Slot::DECK_SLOTS);
    assert_eq!(piles.list(1), &[Slot::new(100)]);
}

#[test]
fn the_shared_list_holds_both_decks() {
    let mut piles = SmallPiles::default();
    for i in 0..Slot::SHARED_DECK_SLOTS {
        piles.push_shared(Slot::from_index(i)).unwrap();
    }
    assert_eq!(piles.push_shared(Slot::new(200)), Err(Full));
    assert_eq!(piles.list(0).len(), 120);
    assert!(piles.list(1).is_empty());
    assert_eq!(piles.remove(0, 0), Slot::new(0));
    piles.insert_shared(119, Slot::new(0)).unwrap();
    assert_eq!(piles.list(0)[119], Slot::new(0));
    assert_eq!(piles.list(0)[0], Slot::new(1));
    assert_eq!(piles.pop(0), Some(Slot::new(0)));
    assert_eq!(piles.position(0, Slot::new(119)), Some(118));
}

#[test]
fn insert_and_remove_keep_the_order_of_one_list() {
    let mut piles = SmallPiles::default();
    for i in [1, 2, 4] {
        piles.push(1, Slot::new(i)).unwrap();
    }
    piles.insert(1, 2, Slot::new(3)).unwrap();
    piles.insert(1, 0, Slot::new(0)).unwrap();
    assert_eq!(piles.list(1), &[0, 1, 2, 3, 4].map(Slot::new));
    assert_eq!(piles.remove(1, 1), Slot::new(1));
    piles.remove_item(1, Slot::new(3));
    assert_eq!(piles.list(1), &[0, 2, 4].map(Slot::new));
    assert!(piles.list(0).is_empty());
}

#[test]
fn equality_and_the_hash_read_the_live_items_as_two_lists_did() {
    let (mut a, mut b) = (SmallPiles::default(), SmallPiles::default());
    let mut lists: [Fixed<Slot, 60>; 2] = Default::default();
    a.push(0, Slot::new(9)).unwrap();
    a.pop(0);
    b.push(0, Slot::new(7)).unwrap();
    b.pop(0);
    for (list, slot) in [(0, 3), (1, 70), (1, 71)] {
        a.push(list, Slot::new(slot)).unwrap();
        b.push(list, Slot::new(slot)).unwrap();
        lists[list].push(Slot::new(slot)).unwrap();
    }
    assert_eq!(a, b);
    assert_eq!(hash(&a), hash(&b));
    assert_eq!(hash(&a), hash(&lists));

    let mut large = LargePiles::default();
    let mut vecs: [Vec<LargeSlot>; 2] = Default::default();
    for (list, slot) in [(0, 3), (1, 300)] {
        large.push(list, LargeSlot::from_index(slot)).unwrap();
        vecs[list].push(LargeSlot::from_index(slot));
    }
    assert_eq!(hash(&large), hash(&vecs));
}
