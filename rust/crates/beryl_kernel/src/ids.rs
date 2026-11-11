//! The engine's identifiers: object slots, players and game-table indices. Each is its own
//! type, so one cannot pass for another, and "no object" is `Option`, never a reserved value.
//! `Option<Slot>` and `Option<CardIx>` are one byte, like the `u8`s they replace.

use std::fmt;
use std::hash::Hash;
use std::num::{NonZeroU8, NonZeroU16};

pub use beryl_card::Seat;

/// An object slot type, as a [`crate::Layout`] numbers its objects: [`Slot`] for
/// [`crate::Small`], [`LargeSlot`] for [`crate::Large`]. Both number slots from 0, so
/// widening a game keeps every slot's number.
pub trait SlotId:
    Copy + fmt::Debug + fmt::Display + Default + Eq + Ord + Hash + Send + Sync + 'static
{
    /// How many slots there are: 0 to `COUNT - 1`.
    const COUNT: usize;
    /// The words of a [`crate::Bitboard`] over these slots, at least `COUNT` bits.
    type Words: Copy + fmt::Debug + Default + Eq + Hash + Send + Sync + AsRef<[u64]> + AsMut<[u64]>;
    /// Words with no bit set.
    const NO_WORDS: Self::Words;

    /// The slot at `index`, which must be below `COUNT`.
    fn from_index(index: usize) -> Self;

    /// The slot at `index`, if there is one.
    #[inline]
    fn try_from_index(index: usize) -> Option<Self> {
        (index < Self::COUNT).then(|| Self::from_index(index))
    }

    fn index(self) -> usize;

    /// The slot's number, as the UI's protocol names it.
    #[inline]
    fn num(self) -> u16 {
        u16::try_from(self.index()).expect("slot numbers fit 16 bits")
    }

    /// Every slot, in ascending order.
    fn all() -> impl Iterator<Item = Self> {
        (0..Self::COUNT).map(Self::from_index)
    }
}

/// An object slot: the index of an object in the state (architecture §4.2). It stores the
/// slot plus one, so `Option<Slot>` is one byte; slot 255 does not exist.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Slot(NonZeroU8);

impl Slot {
    /// How many slots there are: 0 to 254.
    pub const COUNT: usize = 255;

    /// The slot at `index`.
    ///
    /// # Panics
    ///
    /// If `index` is 255, which is no slot.
    #[inline]
    pub const fn new(index: u8) -> Slot {
        match NonZeroU8::new(index.wrapping_add(1)) {
            Some(n) => Slot(n),
            None => panic!("slot 255 does not exist"),
        }
    }

    /// The slot at `index`, if there is one.
    #[inline]
    pub const fn try_new(index: u8) -> Option<Slot> {
        match NonZeroU8::new(index.wrapping_add(1)) {
            Some(n) => Some(Slot(n)),
            None => None,
        }
    }

    #[inline]
    pub const fn get(self) -> u8 {
        self.0.get() - 1
    }

    #[inline]
    pub const fn index(self) -> usize {
        self.get() as usize
    }

    /// Every slot, in ascending order.
    pub fn all() -> impl Iterator<Item = Slot> {
        (0..Self::COUNT as u8).map(Slot::new)
    }
}

impl SlotId for Slot {
    const COUNT: usize = Slot::COUNT;
    type Words = [u64; 4];
    const NO_WORDS: [u64; 4] = [0; 4];

    #[inline]
    fn from_index(index: usize) -> Slot {
        debug_assert!(index < Self::COUNT);
        Slot::new(index as u8)
    }

    #[inline]
    fn index(self) -> usize {
        Slot::index(self)
    }

    fn all() -> impl Iterator<Item = Slot> {
        Slot::all()
    }
}

impl From<Slot> for LargeSlot {
    #[inline]
    fn from(slot: Slot) -> LargeSlot {
        LargeSlot::from_index(slot.index())
    }
}

/// A slot of [`crate::Large`]'s, four times as many as [`Slot`]'s. It stores the slot plus
/// one, so `Option<LargeSlot>` is two bytes.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LargeSlot(NonZeroU16);

impl SlotId for LargeSlot {
    const COUNT: usize = 1024;
    type Words = [u64; 16];
    const NO_WORDS: [u64; 16] = [0; 16];

    #[inline]
    fn from_index(index: usize) -> LargeSlot {
        assert!(index < Self::COUNT, "slot {index} does not exist");
        LargeSlot(NonZeroU16::new(index as u16 + 1).expect("one more than an index"))
    }

    #[inline]
    fn index(self) -> usize {
        usize::from(self.0.get() - 1)
    }
}

/// Slot 0, as [`Slot`]'s default.
impl Default for LargeSlot {
    #[inline]
    fn default() -> LargeSlot {
        LargeSlot::from_index(0)
    }
}

impl fmt::Debug for LargeSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Slot({})", self.index())
    }
}

impl fmt::Display for LargeSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.index().fmt(f)
    }
}

/// Slot 0: filler for fixed-capacity arrays, never a meaning of its own.
impl Default for Slot {
    #[inline]
    fn default() -> Slot {
        Slot::new(0)
    }
}

impl fmt::Debug for Slot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Slot({})", self.get())
    }
}

impl fmt::Display for Slot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.get().fmt(f)
    }
}

/// A card's index in the game table (architecture §6.4): what `State` stores for a card. It
/// stores the index plus one, so `Option<CardIx>` is one byte; a table holds at most 255
/// cards.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CardIx(NonZeroU8);

impl CardIx {
    /// How many indices there are: 0 to 254.
    pub const COUNT: usize = 255;

    /// The game index `index`.
    ///
    /// # Panics
    ///
    /// If `index` is 255, beyond any table.
    #[inline]
    pub const fn new(index: u8) -> CardIx {
        match NonZeroU8::new(index.wrapping_add(1)) {
            Some(n) => CardIx(n),
            None => panic!("game index 255 does not exist"),
        }
    }

    /// The game index `index`, if there is one.
    #[inline]
    pub const fn try_new(index: u8) -> Option<CardIx> {
        match NonZeroU8::new(index.wrapping_add(1)) {
            Some(n) => Some(CardIx(n)),
            None => None,
        }
    }

    #[inline]
    pub const fn get(self) -> u8 {
        self.0.get() - 1
    }

    #[inline]
    pub const fn index(self) -> usize {
        self.get() as usize
    }
}

/// Index 0: filler for fixed-capacity arrays and unused slots, never a meaning of its own.
impl Default for CardIx {
    #[inline]
    fn default() -> CardIx {
        CardIx::new(0)
    }
}

impl fmt::Debug for CardIx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CardIx({})", self.get())
    }
}

const _: () = assert!(size_of::<Option<Slot>>() == 1);
const _: () = assert!(size_of::<Option<LargeSlot>>() == 2);
const _: () = assert!(size_of::<Option<CardIx>>() == 1);
const _: () = assert!(size_of::<Seat>() == 1);

#[cfg(test)]
mod tests {
    use super::{CardIx, LargeSlot, Seat, Slot, SlotId};

    #[test]
    fn slots_round_trip_and_255_is_none() {
        assert_eq!(Slot::new(0).get(), 0);
        assert_eq!(Slot::new(254).get(), 254);
        assert_eq!(Slot::try_new(255), None);
        assert_eq!(Slot::all().count(), Slot::COUNT);
        assert_eq!(CardIx::new(254).index(), 254);
        assert_eq!(LargeSlot::from(Slot::new(254)).index(), 254);
        assert_eq!(
            LargeSlot::try_from_index(1023).map(LargeSlot::index),
            Some(1023)
        );
        assert_eq!(LargeSlot::try_from_index(1024), None);
    }

    #[test]
    fn seats_are_each_others_opponents() {
        assert_eq!(Seat::P0.opponent(), Seat::P1);
        assert_eq!(Seat::P1.opponent(), Seat::P0);
        assert_eq!(Seat::try_new(2), None);
        assert_eq!(Seat::new(1).index(), 1);
    }
}
