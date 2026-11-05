//! `Bitboard`: one bit per object slot (architecture §4.1).

use std::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, Not};

use crate::ids::Slot;

/// A set of object slots, `[u64; 4]` for the 255 slots. Zone membership, knowledge and
/// filters are ANDs and ORs over these, in the style of chess bitboards.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Bitboard(pub [u64; 4]);

impl Bitboard {
    pub const EMPTY: Self = Self([0; 4]);

    pub fn single(slot: Slot) -> Self {
        let mut b = Self::EMPTY;
        b.insert(slot);
        b
    }

    pub fn contains(self, slot: Slot) -> bool {
        self.0[slot.index() >> 6] & (1 << (slot.get() & 63)) != 0
    }

    pub fn insert(&mut self, slot: Slot) {
        self.0[slot.index() >> 6] |= 1 << (slot.get() & 63);
    }

    pub fn remove(&mut self, slot: Slot) {
        self.0[slot.index() >> 6] &= !(1 << (slot.get() & 63));
    }

    pub fn is_empty(self) -> bool {
        self.0 == [0; 4]
    }

    pub fn count(self) -> u32 {
        self.0.iter().map(|w| w.count_ones()).sum()
    }

    /// The lowest slot in the set.
    pub fn first(self) -> Option<Slot> {
        self.iter().next()
    }

    /// The `n`th slot in ascending order, the meaning of a `Choice` over objects.
    pub fn nth(self, n: usize) -> Option<Slot> {
        self.iter().nth(n)
    }

    /// The slots in ascending order.
    pub fn iter(self) -> Slots {
        Slots(self)
    }
}

/// The slots of a [`Bitboard`], lowest first.
pub struct Slots(Bitboard);

impl Iterator for Slots {
    type Item = Slot;

    fn next(&mut self) -> Option<Slot> {
        for (i, word) in self.0.0.iter_mut().enumerate() {
            if *word != 0 {
                let bit = word.trailing_zeros();
                *word &= *word - 1;
                return Some(Slot::new((i as u32 * 64 + bit) as u8));
            }
        }
        None
    }
}

impl BitAnd for Bitboard {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(std::array::from_fn(|i| self.0[i] & rhs.0[i]))
    }
}

impl BitOr for Bitboard {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(std::array::from_fn(|i| self.0[i] | rhs.0[i]))
    }
}

impl Not for Bitboard {
    type Output = Self;
    /// Every slot not in the set. Bit 255 is no slot, so it stays clear.
    fn not(self) -> Self {
        let mut words = self.0.map(|w| !w);
        words[3] &= !(1 << 63);
        Self(words)
    }
}

impl BitAndAssign for Bitboard {
    fn bitand_assign(&mut self, rhs: Self) {
        *self = *self & rhs;
    }
}

impl BitOrAssign for Bitboard {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = *self | rhs;
    }
}

impl FromIterator<Slot> for Bitboard {
    fn from_iter<I: IntoIterator<Item = Slot>>(iter: I) -> Self {
        let mut b = Self::EMPTY;
        for slot in iter {
            b.insert(slot);
        }
        b
    }
}

#[cfg(test)]
mod tests {
    use super::Bitboard;
    use crate::ids::Slot;

    fn set(slots: &[u8]) -> Bitboard {
        slots.iter().map(|&s| Slot::new(s)).collect()
    }

    fn slots(b: Bitboard) -> Vec<u8> {
        b.iter().map(Slot::get).collect()
    }

    #[test]
    fn slots_iterate_in_ascending_order_across_words() {
        let b = set(&[254, 3, 64, 0, 130]);
        assert_eq!(slots(b), vec![0, 3, 64, 130, 254]);
        assert_eq!(b.count(), 5);
        assert_eq!(b.nth(2), Some(Slot::new(64)));
        assert!(b.contains(Slot::new(130)) && !b.contains(Slot::new(131)));
    }

    #[test]
    fn set_operations() {
        let a = set(&[1, 2, 200]);
        let b = set(&[2, 200, 201]);
        assert_eq!(slots(a & b), vec![2, 200]);
        assert_eq!((a | b).count(), 4);
        assert_eq!(slots(a & !b), vec![1]);
    }

    #[test]
    fn the_complement_holds_every_slot_and_no_more() {
        let all = !Bitboard::EMPTY;
        assert_eq!(all.count() as usize, Slot::COUNT);
        assert_eq!(all.iter().last(), Some(Slot::new(254)));
    }
}
