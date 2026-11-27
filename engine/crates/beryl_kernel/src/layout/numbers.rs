//! Numeric types of a [`Layout`](super::Layout): power and toughness, counter and damage counts,
//! mana pool amounts, and life totals.
//!
//! [`Small`](super::Small) keeps narrow primitives (`i8`, `u8`, `i16`) so [`State<Small>`] stays
//! within its 20 KiB budget; [`Large`](super::Large) widens them to 32-bit integers.
//!
//! Implements: none. Defines the numeric bounds of a storage layout.
//!
//! [`State<Small>`]: crate::State

use std::fmt::Display;
use std::ops::{Add, AddAssign, Neg, Sub, SubAssign};

use super::Item;

/// A layout's integer quantity: arithmetic, ordering and conversion to and from `i64` for
/// callers that print or compare across layouts.
pub trait Number:
    Item
    + Copy
    + Display
    + Default
    + Ord
    + Into<i64>
    + TryFrom<i64>
    + Add<Output = Self>
    + Sub<Output = Self>
    + AddAssign
    + SubAssign
    + 'static
{
    const ZERO: Self;
    const ONE: Self;
    const MIN: Self;
    const MAX: Self;

    fn checked_add(self, rhs: Self) -> Option<Self>;
    fn checked_sub(self, rhs: Self) -> Option<Self>;
    fn checked_mul(self, rhs: Self) -> Option<Self>;
    // Saturating helpers are for display/UI/heuristic conversions, never for rules quantities.
    #[must_use]
    fn saturating_add(self, rhs: Self) -> Self;
    #[must_use]
    fn saturating_sub(self, rhs: Self) -> Self;
    #[must_use]
    fn saturating_mul(self, rhs: Self) -> Self;

    #[inline]
    fn to_i64(self) -> i64 {
        self.into()
    }

    #[inline]
    fn try_from_i64(n: i64) -> Option<Self> {
        Self::try_from(n).ok()
    }

    /// `n` clamped to `Self::MIN..=Self::MAX`.
    #[inline]
    #[must_use]
    fn saturating_from_i64(n: i64) -> Self {
        Self::try_from(n).unwrap_or(if n < 0 { Self::MIN } else { Self::MAX })
    }

    #[inline]
    fn try_from_usize(n: usize) -> Option<Self> {
        i64::try_from(n).ok().and_then(Self::try_from_i64)
    }

    #[inline]
    #[must_use]
    fn saturating_from_usize(n: usize) -> Self {
        Self::try_from_usize(n).unwrap_or(Self::MAX)
    }

    #[inline]
    fn is_zero(self) -> bool {
        self == Self::ZERO
    }

    /// Converts to another layout's numeric type `M`, or `None` if out of range.
    #[inline]
    fn to_layout<M: Number>(self) -> Option<M> {
        M::try_from_i64(self.to_i64())
    }
}

/// A signed layout quantity (`Layout::Pt`, `Layout::Life`).
pub trait SignedNumber: Number + From<i8> + Neg<Output = Self> {
    /// Converts from a vocabulary `u8`, saturating at `Self::MAX` (for `i8`, `127`).
    #[must_use]
    fn from_u8_saturating(n: u8) -> Self;
    fn checked_neg(self) -> Option<Self>;
    #[must_use]
    fn saturating_neg(self) -> Self;
    fn to_i32(self) -> i32;

    #[inline]
    fn to_i8_saturating(self) -> i8 {
        i8::try_from(self.to_i64()).unwrap_or(if self < Self::ZERO { i8::MIN } else { i8::MAX })
    }

    #[inline]
    fn to_i16_saturating(self) -> i16 {
        i16::try_from(self.to_i64()).unwrap_or(if self < Self::ZERO {
            i16::MIN
        } else {
            i16::MAX
        })
    }
}

/// A layout's life total (`i16` in `Small`, `i32` in `Large`).
pub trait LifeNumber: SignedNumber + From<u8> + From<i16> {}

/// An unsigned layout quantity (`Layout::Count`, `Layout::Mana`).
pub trait UnsignedNumber: Number + From<u8> {
    fn to_u32(self) -> u32;
    fn to_usize(self) -> usize;

    #[inline]
    fn to_u8_saturating(self) -> u8 {
        u8::try_from(self.to_i64()).unwrap_or(u8::MAX)
    }

    #[inline]
    fn to_i32_saturating(self) -> i32 {
        i32::try_from(self.to_i64()).unwrap_or(i32::MAX)
    }
}

macro_rules! impl_number {
    ($t:ty) => {
        impl Number for $t {
            const ZERO: Self = 0;
            const ONE: Self = 1;
            const MIN: Self = <$t>::MIN;
            const MAX: Self = <$t>::MAX;

            #[inline]
            fn checked_add(self, rhs: Self) -> Option<Self> {
                <$t>::checked_add(self, rhs)
            }

            #[inline]
            fn checked_sub(self, rhs: Self) -> Option<Self> {
                <$t>::checked_sub(self, rhs)
            }

            #[inline]
            fn checked_mul(self, rhs: Self) -> Option<Self> {
                <$t>::checked_mul(self, rhs)
            }

            #[inline]
            fn saturating_add(self, rhs: Self) -> Self {
                <$t>::saturating_add(self, rhs)
            }

            #[inline]
            fn saturating_sub(self, rhs: Self) -> Self {
                <$t>::saturating_sub(self, rhs)
            }

            #[inline]
            fn saturating_mul(self, rhs: Self) -> Self {
                <$t>::saturating_mul(self, rhs)
            }
        }
    };
}

impl_number!(i8);
impl_number!(i16);
impl_number!(i32);
impl_number!(u8);
impl_number!(u32);

impl SignedNumber for i8 {
    #[inline]
    fn from_u8_saturating(n: u8) -> Self {
        i8::try_from(n).unwrap_or(i8::MAX)
    }

    #[inline]
    fn checked_neg(self) -> Option<Self> {
        i8::checked_neg(self)
    }

    #[inline]
    fn saturating_neg(self) -> Self {
        i8::saturating_neg(self)
    }

    #[inline]
    fn to_i32(self) -> i32 {
        i32::from(self)
    }
}

impl SignedNumber for i16 {
    #[inline]
    fn from_u8_saturating(n: u8) -> Self {
        i16::from(n)
    }

    #[inline]
    fn checked_neg(self) -> Option<Self> {
        i16::checked_neg(self)
    }

    #[inline]
    fn saturating_neg(self) -> Self {
        i16::saturating_neg(self)
    }

    #[inline]
    fn to_i32(self) -> i32 {
        i32::from(self)
    }
}

impl SignedNumber for i32 {
    #[inline]
    fn from_u8_saturating(n: u8) -> Self {
        i32::from(n)
    }

    #[inline]
    fn checked_neg(self) -> Option<Self> {
        i32::checked_neg(self)
    }

    #[inline]
    fn saturating_neg(self) -> Self {
        i32::saturating_neg(self)
    }

    #[inline]
    fn to_i32(self) -> i32 {
        self
    }
}

impl LifeNumber for i16 {}
impl LifeNumber for i32 {}

impl UnsignedNumber for u8 {
    #[inline]
    fn to_u32(self) -> u32 {
        u32::from(self)
    }

    #[inline]
    fn to_usize(self) -> usize {
        usize::from(self)
    }
}

impl UnsignedNumber for u32 {
    #[inline]
    fn to_u32(self) -> u32 {
        self
    }

    #[inline]
    fn to_usize(self) -> usize {
        usize::try_from(self).expect("u32 fits in usize")
    }
}
