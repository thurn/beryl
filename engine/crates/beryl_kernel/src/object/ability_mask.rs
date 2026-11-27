//! The masks of a permanent's layer results: the abilities of its face it has lost, and the
//! rows of the table of grants that grant it one (architecture §9).
//!
//! Implements: none. Holds bit sets the layers write.

use std::fmt::Debug;
use std::hash::Hash;

use beryl_card::AbilityIx;

/// The abilities of a face a permanent has lost, one bit for each, by the index of the
/// ability on the face: as many as the layout's mask is wide ([`AbilityMask::WIDTH`]). An
/// effect that would remove an ability past that width, or all the abilities of a face
/// with more, has no place in the mask, and the recompute stalls the game into a layout
/// whose mask is wider.
pub trait AbilityMask: Copy + Debug + Default + Eq + Hash + Send + Sync + 'static {
    /// How many abilities of a face the mask can name.
    const WIDTH: usize;
    /// Every ability the mask can name: "loses all abilities".
    const ALL: Self;
    /// Whether the ability at `ability` is lost.
    fn contains(self, ability: AbilityIx) -> bool;
    /// The mask with the ability at `ability` lost too, or `None` past the mask's width.
    fn with(self, ability: AbilityIx) -> Option<Self>;
    /// Whether no ability is lost.
    fn is_empty(self) -> bool;
    /// The mask as bits, the ability at index `n` in bit `n`.
    fn bits(self) -> u128;
    /// The mask of `bits`, the ability at index `n` in bit `n`, dropping the bits past the
    /// mask's width.
    fn from_bits(bits: u128) -> Self;
    /// How many bits are set: as a mask of rows of the table of grants, how many rows grant
    /// the permanent an ability.
    fn count(self) -> u32;
    /// The place of the lowest set bit, or the mask's width if none is.
    fn lowest(self) -> usize;
    /// The mask without its lowest set bit.
    #[must_use]
    fn without_lowest(self) -> Self;

    /// The mask with bit `k` set too, or `None` past the mask's width: as a mask of rows of
    /// the table of grants ([`super::Derived::granted`]).
    #[inline]
    fn with_row(self, k: usize) -> Option<Self> {
        (k < Self::WIDTH).then(|| Self::from_bits(self.bits() | 1 << k))
    }

    /// Whether bit `k` is set: as a mask of rows of the table of grants.
    #[inline]
    fn has_row(self, k: usize) -> bool {
        k < Self::WIDTH && self.bits() & 1 << k != 0
    }

    /// The mask without bit `k`.
    #[inline]
    #[must_use]
    fn without_row(self, k: usize) -> Self {
        if k < Self::WIDTH {
            Self::from_bits(self.bits() & !(1u128 << k))
        } else {
            self
        }
    }
}

macro_rules! ability_mask {
    ($($bits:ty),*) => {$(
        impl AbilityMask for $bits {
            const WIDTH: usize = <$bits>::BITS as usize;
            const ALL: Self = <$bits>::MAX;

            #[inline]
            fn contains(self, ability: AbilityIx) -> bool {
                ability.index() < Self::WIDTH && self & (1 << ability.index()) != 0
            }

            #[inline]
            fn with(self, ability: AbilityIx) -> Option<Self> {
                (ability.index() < Self::WIDTH).then(|| self | (1 << ability.index()))
            }

            #[inline]
            fn is_empty(self) -> bool {
                self == 0
            }

            #[inline]
            fn bits(self) -> u128 {
                u128::from(self)
            }

            #[inline]
            fn from_bits(bits: u128) -> Self {
                let low_bits = bits & u128::from(<$bits>::MAX);
                <$bits>::try_from(low_bits).expect("masked to the layout's ability width")
            }

            #[inline]
            fn count(self) -> u32 {
                self.count_ones()
            }

            #[inline]
            fn lowest(self) -> usize {
                self.trailing_zeros() as usize
            }

            #[inline]
            fn without_lowest(self) -> Self {
                self & self.wrapping_sub(1)
            }
        }
    )*};
}

ability_mask!(u16, u128);
