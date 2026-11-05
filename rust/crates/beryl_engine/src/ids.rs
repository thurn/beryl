//! The engine's identifiers: object slots, players and game-table indices. Each is its own
//! type, so one cannot pass for another, and "no object" is `Option`, never a reserved value.
//! `Option<Slot>` and `Option<CardIx>` are one byte, like the `u8`s they replace.

use std::fmt;
use std::num::NonZeroU8;

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
    pub const fn new(index: u8) -> Slot {
        match NonZeroU8::new(index.wrapping_add(1)) {
            Some(n) => Slot(n),
            None => panic!("slot 255 does not exist"),
        }
    }

    /// The slot at `index`, if there is one.
    pub const fn try_new(index: u8) -> Option<Slot> {
        match NonZeroU8::new(index.wrapping_add(1)) {
            Some(n) => Some(Slot(n)),
            None => None,
        }
    }

    pub const fn get(self) -> u8 {
        self.0.get() - 1
    }

    pub const fn index(self) -> usize {
        self.get() as usize
    }

    /// Every slot, in ascending order.
    pub fn all() -> impl Iterator<Item = Slot> {
        (0..Self::COUNT as u8).map(Slot::new)
    }
}

/// Slot 0: filler for fixed-capacity arrays, never a meaning of its own.
impl Default for Slot {
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

/// One of the two players, by seat: the first decklist's and the second's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Seat {
    #[default]
    P0,
    P1,
}

impl Seat {
    /// Both seats, in order.
    pub const BOTH: [Seat; 2] = [Seat::P0, Seat::P1];

    /// The seat at `index`.
    ///
    /// # Panics
    ///
    /// If `index` is neither 0 nor 1.
    pub const fn new(index: u8) -> Seat {
        match index {
            0 => Seat::P0,
            1 => Seat::P1,
            _ => panic!("a seat is 0 or 1"),
        }
    }

    /// The seat at `index`, if there is one.
    pub const fn try_new(index: u8) -> Option<Seat> {
        match index {
            0 => Some(Seat::P0),
            1 => Some(Seat::P1),
            _ => None,
        }
    }

    /// The other player.
    #[must_use]
    pub const fn opponent(self) -> Seat {
        match self {
            Seat::P0 => Seat::P1,
            Seat::P1 => Seat::P0,
        }
    }

    pub const fn get(self) -> u8 {
        self as u8
    }

    /// The index of this seat's entry in a per-player array.
    pub const fn index(self) -> usize {
        self as usize
    }
}

impl fmt::Display for Seat {
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
    pub const fn new(index: u8) -> CardIx {
        match NonZeroU8::new(index.wrapping_add(1)) {
            Some(n) => CardIx(n),
            None => panic!("game index 255 does not exist"),
        }
    }

    /// The game index `index`, if there is one.
    pub const fn try_new(index: u8) -> Option<CardIx> {
        match NonZeroU8::new(index.wrapping_add(1)) {
            Some(n) => Some(CardIx(n)),
            None => None,
        }
    }

    pub const fn get(self) -> u8 {
        self.0.get() - 1
    }

    pub const fn index(self) -> usize {
        self.get() as usize
    }
}

/// Index 0: filler for fixed-capacity arrays and unused slots, never a meaning of its own.
impl Default for CardIx {
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
const _: () = assert!(size_of::<Option<CardIx>>() == 1);
const _: () = assert!(size_of::<Seat>() == 1);

#[cfg(test)]
mod tests {
    use super::{CardIx, Seat, Slot};

    #[test]
    fn slots_round_trip_and_255_is_none() {
        assert_eq!(Slot::new(0).get(), 0);
        assert_eq!(Slot::new(254).get(), 254);
        assert_eq!(Slot::try_new(255), None);
        assert_eq!(Slot::all().count(), Slot::COUNT);
        assert_eq!(CardIx::new(254).index(), 254);
    }

    #[test]
    fn seats_are_each_others_opponents() {
        assert_eq!(Seat::P0.opponent(), Seat::P1);
        assert_eq!(Seat::P1.opponent(), Seat::P0);
        assert_eq!(Seat::try_new(2), None);
        assert_eq!(Seat::new(1).index(), 1);
    }
}
