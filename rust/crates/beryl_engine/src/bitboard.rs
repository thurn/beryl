//! `Bitboard`: one bit per object slot (architecture §4.1).

use std::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, Not};

/// A set of object slots, `[u64; 4]` for the 256 slots. Zone membership, knowledge and
/// filters are ANDs and ORs over these, in the style of chess bitboards.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Bitboard(pub [u64; 4]);

impl Bitboard {
    pub const EMPTY: Self = Self([0; 4]);

    pub fn single(slot: u8) -> Self {
        let mut b = Self::EMPTY;
        b.insert(slot);
        b
    }

    pub fn contains(self, slot: u8) -> bool {
        self.0[usize::from(slot >> 6)] & (1 << (slot & 63)) != 0
    }

    pub fn insert(&mut self, slot: u8) {
        self.0[usize::from(slot >> 6)] |= 1 << (slot & 63);
    }

    pub fn remove(&mut self, slot: u8) {
        self.0[usize::from(slot >> 6)] &= !(1 << (slot & 63));
    }

    pub fn is_empty(self) -> bool {
        self.0 == [0; 4]
    }

    pub fn count(self) -> u32 {
        self.0.iter().map(|w| w.count_ones()).sum()
    }

    /// The lowest slot in the set.
    pub fn first(self) -> Option<u8> {
        self.iter().next()
    }

    /// The `n`th slot in ascending order, the meaning of a `Choice` over objects.
    pub fn nth(self, n: usize) -> Option<u8> {
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
    type Item = u8;

    fn next(&mut self) -> Option<u8> {
        for (i, word) in self.0.0.iter_mut().enumerate() {
            if *word != 0 {
                let bit = word.trailing_zeros();
                *word &= *word - 1;
                return Some((i as u32 * 64 + bit) as u8);
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
    fn not(self) -> Self {
        Self(self.0.map(|w| !w))
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

impl FromIterator<u8> for Bitboard {
    fn from_iter<I: IntoIterator<Item = u8>>(iter: I) -> Self {
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

    #[test]
    fn slots_iterate_in_ascending_order_across_words() {
        let b: Bitboard = [255, 3, 64, 0, 130].into_iter().collect();
        assert_eq!(b.iter().collect::<Vec<_>>(), vec![0, 3, 64, 130, 255]);
        assert_eq!(b.count(), 5);
        assert_eq!(b.nth(2), Some(64));
        assert!(b.contains(130) && !b.contains(131));
    }

    #[test]
    fn set_operations() {
        let a: Bitboard = [1, 2, 200].into_iter().collect();
        let b: Bitboard = [2, 200, 201].into_iter().collect();
        assert_eq!((a & b).iter().collect::<Vec<_>>(), vec![2, 200]);
        assert_eq!((a | b).count(), 4);
        assert_eq!((a & !b).iter().collect::<Vec<_>>(), vec![1]);
    }
}
