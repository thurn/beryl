//! `Bitboard`: one bit per object slot (architecture §4.1).
//!
//! Implements: none. The bit set stores slots for queries and mutators.

use std::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, Not};

use crate::ids::{Slot, SlotId};

/// A set of object slots of type `S`: for [`Slot`], `[u64; 4]` for the 255 slots. Zone
/// membership, knowledge and filters are ANDs and ORs over these, in the style of chess
/// bitboards.
///
/// Outside the kernel it is a set and nothing more: it has no first member and does not
/// iterate, because a slot number is no order the rules can see (the deal and every shuffle
/// put cards in slots at random, architecture §14). What a caller may ask is whatever the
/// answer to cannot depend on the order the members are met in: membership, the set algebra,
/// [`Bitboard::count`], [`Bitboard::any`], [`Bitboard::all`], [`Bitboard::at_least`],
/// [`Bitboard::filter`], [`Bitboard::fold`], [`Bitboard::sum`] and [`Bitboard::sole`].
/// A fold uses a combination independent of traversal order. A sequence is asked of the
/// state by the name of its order (the crate's contract, "Sets and orders").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Bitboard<S: SlotId = Slot>(pub(crate) S::Words);

impl<S: SlotId> Bitboard<S> {
    pub const EMPTY: Self = Self(S::NO_WORDS);

    #[inline]
    pub fn single(slot: S) -> Self {
        let mut b = Self::EMPTY;
        b.insert(slot);
        b
    }

    #[inline]
    pub fn contains(self, slot: S) -> bool {
        self.0.as_ref()[slot.index() >> 6] & (1 << (slot.index() & 63)) != 0
    }

    #[inline]
    pub fn insert(&mut self, slot: S) {
        self.0.as_mut()[slot.index() >> 6] |= 1 << (slot.index() & 63);
    }

    #[inline]
    pub fn remove(&mut self, slot: S) {
        self.0.as_mut()[slot.index() >> 6] &= !(1 << (slot.index() & 63));
    }

    #[inline]
    pub fn is_empty(self) -> bool {
        self.0 == S::NO_WORDS
    }

    #[inline]
    pub fn count(self) -> u32 {
        self.0.as_ref().iter().map(|w| w.count_ones()).sum()
    }

    /// Whether `f` holds for some member.
    #[inline]
    pub fn any(self, f: impl Fn(S) -> bool) -> bool {
        for (i, &w) in self.0.as_ref().iter().enumerate() {
            let mut word = w;
            if word == 0 {
                continue;
            }
            let base = i * 64;
            while word != 0 {
                let bit = word.trailing_zeros();
                word &= word - 1;
                if f(S::from_index(
                    base + usize::try_from(bit).expect("a bit index fits in usize"),
                )) {
                    return true;
                }
            }
        }
        false
    }

    /// Whether `f` holds for every member.
    #[inline]
    pub fn all(self, f: impl Fn(S) -> bool) -> bool {
        !self.any(|slot| !f(slot))
    }

    /// Whether `f` holds for at least `n` members. It stops counting at `n`, which
    /// `filter(f).count() >= n` would not.
    #[inline]
    pub fn at_least(self, n: usize, f: impl Fn(S) -> bool) -> bool {
        if n == 0 {
            return true;
        }
        let mut found = 0;
        for (i, &w) in self.0.as_ref().iter().enumerate() {
            let mut word = w;
            if word == 0 {
                continue;
            }
            let base = i * 64;
            while word != 0 {
                let bit = word.trailing_zeros();
                word &= word - 1;
                if f(S::from_index(
                    base + usize::try_from(bit).expect("a bit index fits in usize"),
                )) {
                    found += 1;
                    if found >= n {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// The members `f` holds for.
    #[inline]
    #[must_use]
    pub fn filter(self, f: impl Fn(S) -> bool) -> Self {
        let mut out = Self::EMPTY;
        for (i, &w) in self.0.as_ref().iter().enumerate() {
            let mut word = w;
            if word == 0 {
                continue;
            }
            let mut kept = 0u64;
            let base = i * 64;
            while word != 0 {
                let bit = word.trailing_zeros();
                word &= word - 1;
                if f(S::from_index(
                    base + usize::try_from(bit).expect("a bit index fits in usize"),
                )) {
                    kept |= 1u64 << bit;
                }
            }
            out.0.as_mut()[i] = kept;
        }
        out
    }

    /// Runs `f` on each member in ascending slot order, stopping at the first error.
    #[inline]
    pub(crate) fn try_for_each<E>(self, mut f: impl FnMut(S) -> Result<(), E>) -> Result<(), E> {
        for (i, &w) in self.0.as_ref().iter().enumerate() {
            let mut word = w;
            if word == 0 {
                continue;
            }
            let base = i * 64;
            while word != 0 {
                let bit = word.trailing_zeros();
                word &= word - 1;
                f(S::from_index(
                    base + usize::try_from(bit).expect("a bit index fits in usize"),
                ))?;
            }
        }
        Ok(())
    }

    /// The sum of `f` over the members: of numbers that add exactly, so that the order they
    /// are added in is nothing.
    #[inline]
    pub fn sum<T: core::iter::Sum<T>>(self, f: impl Fn(S) -> T) -> T {
        self.iter().map(f).sum()
    }

    /// Combines independent contributions from the members. The result must be the same
    /// in any order, such as a union or the subtraction of each member's mana.
    #[inline]
    pub fn fold<T>(self, initial: T, add: impl Fn(T, S) -> T) -> T {
        self.iter().fold(initial, add)
    }

    /// Whether a pair of distinct members satisfies the symmetric predicate `matches`.
    #[inline]
    pub fn any_pair(self, matches: impl Fn(S, S) -> bool) -> bool {
        let mut rest = self.iter();
        while let Some(slot) = rest.next() {
            if rest.0.any(|other| matches(slot, other)) {
                return true;
            }
        }
        false
    }

    /// The mapped values in sorted order, retaining repeated values.
    pub fn sorted<T: Ord>(self, map: impl Fn(S) -> T) -> Vec<T> {
        let mut values: Vec<_> = self.iter().map(map).collect();
        values.sort_unstable();
        values
    }

    /// The least of the values `key` gives the members, if any.
    #[inline]
    pub fn min<T: Ord>(self, key: impl Fn(S) -> T) -> Option<T> {
        self.iter().map(key).min()
    }

    /// The greatest of the values `key` gives the members, if any.
    #[inline]
    pub fn max<T: Ord>(self, key: impl Fn(S) -> T) -> Option<T> {
        self.iter().map(key).max()
    }

    /// The one member of a set of one, and nothing for any other set.
    #[inline]
    pub fn sole(self) -> Option<S> {
        let mut rest = self.iter();
        let slot = rest.next()?;
        rest.next().is_none().then_some(slot)
    }

    /// The `n`th slot in ascending order, the meaning of a `Choice` over objects.
    #[inline]
    pub(crate) fn nth(self, n: usize) -> Option<S> {
        self.iter().nth(n)
    }

    /// The slots in ascending order, for kernel use.
    #[inline]
    pub(crate) fn iter(self) -> Slots<S> {
        Slots(self)
    }

    /// Each word of `self` and `rhs` combined by `f`.
    #[inline]
    fn zip(mut self, rhs: Self, f: impl Fn(u64, u64) -> u64) -> Self {
        for (a, &b) in self.0.as_mut().iter_mut().zip(rhs.0.as_ref()) {
            *a = f(*a, b);
        }
        self
    }
}

impl From<Bitboard<Slot>> for Bitboard<crate::ids::LargeSlot> {
    /// The slots at the same places ([`SlotId::to`]), as [`crate::ids::LargeSlot`]s.
    fn from(b: Bitboard<Slot>) -> Self {
        crate::relayout::bits(b)
    }
}

/// The slots of a [`Bitboard`], lowest first: the candidate order.
pub struct Slots<S: SlotId>(Bitboard<S>);

impl<S: SlotId> Iterator for Slots<S> {
    type Item = S;

    #[inline]
    fn next(&mut self) -> Option<S> {
        for (i, word) in self.0.0.as_mut().iter_mut().enumerate() {
            if *word != 0 {
                let bit = word.trailing_zeros();
                *word &= *word - 1;
                return Some(S::from_index(
                    i * 64 + usize::try_from(bit).expect("a bit index fits in usize"),
                ));
            }
        }
        None
    }

    #[inline]
    fn fold<B, F>(self, mut init: B, mut f: F) -> B
    where
        F: FnMut(B, S) -> B,
    {
        for (i, &w) in self.0.0.as_ref().iter().enumerate() {
            let mut word = w;
            if word == 0 {
                continue;
            }
            let base = i * 64;
            while word != 0 {
                let bit = word.trailing_zeros();
                word &= word - 1;
                init = f(
                    init,
                    S::from_index(base + usize::try_from(bit).expect("a bit index fits in usize")),
                );
            }
        }
        init
    }
}

impl<S: SlotId> BitAnd for Bitboard<S> {
    type Output = Self;
    #[inline]
    fn bitand(self, rhs: Self) -> Self {
        self.zip(rhs, |a, b| a & b)
    }
}

impl<S: SlotId> BitOr for Bitboard<S> {
    type Output = Self;
    #[inline]
    fn bitor(self, rhs: Self) -> Self {
        self.zip(rhs, |a, b| a | b)
    }
}

impl<S: SlotId> Not for Bitboard<S> {
    type Output = Self;
    /// Every slot not in the set. Bits past the last slot are no slots, so they stay clear.
    #[inline]
    fn not(mut self) -> Self {
        for (i, w) in self.0.as_mut().iter_mut().enumerate() {
            // Remaining slots in this 64-bit word; saturating at 0 masks unused trailing words.
            let slots = S::COUNT.saturating_sub(i * 64).min(64);
            let mask = if slots == 64 { !0 } else { (1 << slots) - 1 };
            *w = !*w & mask;
        }
        self
    }
}

impl<S: SlotId> BitAndAssign for Bitboard<S> {
    #[inline]
    fn bitand_assign(&mut self, rhs: Self) {
        *self = *self & rhs;
    }
}

impl<S: SlotId> BitOrAssign for Bitboard<S> {
    #[inline]
    fn bitor_assign(&mut self, rhs: Self) {
        *self = *self | rhs;
    }
}

impl<S: SlotId> FromIterator<S> for Bitboard<S> {
    fn from_iter<I: IntoIterator<Item = S>>(iter: I) -> Self {
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
    use crate::ids::{LargeSlot, Slot, SlotId};
    use crate::layout::{Large, Layout, Small};

    fn set(slots: &[u8]) -> Bitboard {
        slots.iter().map(|&s| Slot::new(s)).collect()
    }

    fn slots(b: Bitboard) -> Vec<u8> {
        b.iter().map(Slot::get).collect()
    }

    #[test]
    fn the_token_pool_is_the_token_slots() {
        let slots: Bitboard = Slot::pool().collect();
        assert_eq!(Small::TOKEN_POOL, slots);
        assert_eq!(slots.count(), 135);
        let slots: Bitboard<LargeSlot> = LargeSlot::pool().collect();
        assert_eq!(Large::TOKEN_POOL, slots);
        assert_eq!(slots.count(), 512);
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
    fn order_free_questions() {
        let b = set(&[3, 64, 130, 254]);
        let even = |slot: Slot| slot.get().is_multiple_of(2);
        assert!(b.any(even) && !b.all(even));
        assert!(!Bitboard::<Slot>::EMPTY.any(even) && Bitboard::<Slot>::EMPTY.all(even));
        assert_eq!(slots(b.filter(even)), vec![64, 130, 254]);
        assert!(b.at_least(3, even) && !b.at_least(4, even));
        assert!(Bitboard::<Slot>::EMPTY.at_least(0, even));
        assert_eq!(b.sum(|slot| u32::from(slot.get())), 451);
    }

    #[test]
    fn only_a_set_of_one_has_a_sole_member() {
        assert_eq!(set(&[130]).sole(), Some(Slot::new(130)));
        assert_eq!(set(&[3, 130]).sole(), None);
        assert_eq!(Bitboard::<Slot>::EMPTY.sole(), None);
    }

    #[test]
    fn the_complement_holds_every_slot_and_no_more() {
        let all = !Bitboard::<Slot>::EMPTY;
        assert_eq!(all.count() as usize, Slot::COUNT);
        assert_eq!(all.iter().last(), Some(Slot::new(254)));
        let all = !Bitboard::<LargeSlot>::EMPTY;
        assert_eq!(all.count() as usize, LargeSlot::COUNT);
        assert_eq!(all.iter().last(), LargeSlot::try_from_index(1023));
    }

    #[test]
    fn widening_keeps_each_slot_at_its_place() {
        // The first and last card of each deck, and of the pool.
        let wide = Bitboard::<LargeSlot>::from(set(&[0, 59, 60, 119, 120, 254]));
        let indices: Vec<usize> = wide.iter().map(LargeSlot::index).collect();
        assert_eq!(indices, vec![0, 59, 256, 315, 512, 646]);
    }
}
