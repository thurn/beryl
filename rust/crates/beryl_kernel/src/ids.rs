//! The engine's identifiers: object slots, players and game-table indices. Each is its own
//! type, so one cannot pass for another, and "no object" is `Option`, never a reserved value.
//! `Option<Slot>` and `Option<CardIx>` are one byte, like the `u8`s they replace.

use std::fmt;
use std::hash::Hash;
use std::num::{NonZeroU8, NonZeroU16};

pub use beryl_card::Seat;

/// Where a slot sits among its game's, which is what it keeps in every numbering
/// ([`SlotId::place`]): its number is one numbering's name for the place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// The `usize`th slot of a player's deck.
    Deck(Seat, usize),
    /// The `usize`th slot of the shared pool for tokens and spell copies.
    Pool(usize),
}

/// An object slot type, as a [`crate::Layout`] numbers its objects: [`Slot`] for
/// [`crate::Small`], [`LargeSlot`] for [`crate::Large`].
///
/// Every numbering deals its slots out the same way: `DECK_SLOTS` in a row for each of
/// `PLAYERS` players' decks, by seat, then the shared pool for tokens and spell copies. How
/// many of each is the numbering's own, so a slot's number differs from one to another and
/// its [`Place`] does not: a game is taken to another layout place by place
/// ([`SlotId::to`]). Two layouts that deal their slots differently have different slot types,
/// so the number of one is never read as the number of another.
pub trait SlotId:
    Copy + fmt::Debug + fmt::Display + Default + Eq + Ord + Hash + Send + Sync + 'static
{
    /// How many slots there are: 0 to `COUNT - 1`.
    const COUNT: usize;
    /// How many players' decks have slots: the players of a game numbered this way.
    const PLAYERS: usize;
    /// Slots reserved for each player's deck, and so the most cards a deck, a library or a
    /// graveyard holds: player `p`'s cards sit in `p * DECK_SLOTS .. p * DECK_SLOTS + deck
    /// size`.
    const DECK_SLOTS: usize;
    /// The first slot of the shared pool for tokens and spell copies, after every deck's:
    /// the pool is `POOL .. COUNT`.
    const POOL: usize = Self::PLAYERS * Self::DECK_SLOTS;
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

    /// The slot of the `i`th card of `player`'s deck, which must be below `DECK_SLOTS`.
    #[inline]
    fn deck(player: Seat, i: usize) -> Self {
        debug_assert!(i < Self::DECK_SLOTS);
        Self::from_index(player.index() * Self::DECK_SLOTS + i)
    }

    /// The slots of the shared pool for tokens and spell copies, in ascending order
    /// ([`crate::Layout::TOKEN_POOL`]).
    #[inline]
    fn pool() -> impl Iterator<Item = Self> {
        (Self::POOL..Self::COUNT).map(Self::from_index)
    }

    /// Where the slot sits among its game's.
    #[inline]
    fn place(self) -> Place {
        let i = self.index();
        if i >= Self::POOL {
            Place::Pool(i - Self::POOL)
        } else {
            let player = Seat::new((i / Self::DECK_SLOTS) as u8);
            Place::Deck(player, i % Self::DECK_SLOTS)
        }
    }

    /// The slot at `place`, if this numbering has one there.
    #[inline]
    fn at(place: Place) -> Option<Self> {
        match place {
            Place::Deck(player, i) => (player.index() < Self::PLAYERS && i < Self::DECK_SLOTS)
                .then(|| Self::deck(player, i)),
            Place::Pool(i) => Self::try_from_index(Self::POOL + i),
        }
    }

    /// The slot at the same place in the numbering `T`, if `T` has one there. The order of
    /// slots is the same in every numbering, so a choice among objects by their order
    /// ([`crate::decision::Candidates`]) names the same object in each.
    #[inline]
    fn to<T: SlotId>(self) -> Option<T> {
        if Self::deals_as::<T>() {
            return T::try_from_index(self.index());
        }
        T::at(self.place())
    }

    /// Whether the numbering `T` deals its slots as this one does, so that a place has the
    /// same number in both.
    #[inline]
    fn deals_as<T: SlotId>() -> bool {
        Self::PLAYERS == T::PLAYERS && Self::DECK_SLOTS == T::DECK_SLOTS
    }

    /// The slot's number as the UI's protocol names it: its place's number among
    /// [`LargeSlot`]s, the widest numbering, so an object keeps its name when its game goes
    /// on in [`crate::Large`].
    #[inline]
    fn num(self) -> u16 {
        let wide: LargeSlot = self.to().expect("every place has a slot in Large");
        u16::try_from(wide.index()).expect("slot numbers fit 16 bits")
    }

    /// The slot the UI's protocol names `num` ([`SlotId::num`]), if this numbering has it.
    #[inline]
    fn from_num(num: u16) -> Option<Self> {
        LargeSlot::try_from_index(usize::from(num))?.to()
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

/// [`crate::Small`]'s deal: two players, 60 slots a deck, and the other 135 of its 255 slots
/// for the pool.
impl SlotId for Slot {
    const COUNT: usize = Slot::COUNT;
    const PLAYERS: usize = 2;
    const DECK_SLOTS: usize = 60;
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
    /// The slot at the same place ([`SlotId::to`]).
    #[inline]
    fn from(slot: Slot) -> LargeSlot {
        slot.to().expect("every place has a slot in Large")
    }
}

/// A slot of [`crate::Large`]'s, four times as many as [`Slot`]'s. It stores the slot plus
/// one, so `Option<LargeSlot>` is two bytes.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LargeSlot(NonZeroU16);

/// [`crate::Large`]'s deal: two players, 256 slots a deck, and the other 512 of its 1,024
/// slots for the pool.
impl SlotId for LargeSlot {
    const COUNT: usize = 1024;
    const PLAYERS: usize = 2;
    const DECK_SLOTS: usize = 256;
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
    use super::{CardIx, LargeSlot, Place, Seat, Slot, SlotId};

    #[test]
    fn slots_round_trip_and_255_is_none() {
        assert_eq!(Slot::new(0).get(), 0);
        assert_eq!(Slot::new(254).get(), 254);
        assert_eq!(Slot::try_new(255), None);
        assert_eq!(Slot::all().count(), Slot::COUNT);
        assert_eq!(CardIx::new(254).index(), 254);
        assert_eq!(LargeSlot::from(Slot::new(59)).index(), 59);
        assert_eq!(
            LargeSlot::try_from_index(1023).map(LargeSlot::index),
            Some(1023)
        );
        assert_eq!(LargeSlot::try_from_index(1024), None);
    }

    #[test]
    fn a_slot_keeps_its_place_in_every_numbering() {
        // The last card of the second deck, and the first slot of the pool.
        assert_eq!(Slot::new(119).place(), Place::Deck(Seat::P1, 59));
        assert_eq!(Slot::new(120).place(), Place::Pool(0));
        assert_eq!(Slot::deck(Seat::P1, 0), Slot::new(60));
        assert_eq!(Slot::pool().count(), 135);
        let mut last = None;
        for slot in Slot::all() {
            let wide: LargeSlot = slot.to().unwrap();
            assert_eq!(wide.place(), slot.place());
            assert_eq!(wide.to::<Slot>(), Some(slot));
            assert_eq!(slot.to::<Slot>(), Some(slot));
            // The order is the same, so a choice by order names the same object.
            assert!(last < Some(wide));
            last = Some(wide);
            assert_eq!(usize::from(slot.num()), wide.index());
            assert_eq!(Slot::from_num(slot.num()), Some(slot));
        }
    }

    #[test]
    fn a_place_past_a_numbering_has_no_slot_in_it() {
        let card_61 = LargeSlot::deck(Seat::P0, 60);
        assert_eq!(card_61.index(), 60);
        assert_eq!(card_61.to::<Slot>(), None);
        assert_eq!(Slot::at(Place::Deck(Seat::P1, 60)), None);
        let token_136 = LargeSlot::at(Place::Pool(135)).unwrap();
        assert_eq!(token_136.index(), 512 + 135);
        assert_eq!(token_136.to::<Slot>(), None);
        assert_eq!(Slot::from_num(token_136.num()), None);
        assert_eq!(LargeSlot::at(Place::Pool(512)), None);
        assert_eq!(LargeSlot::pool().count(), 512);
    }

    #[test]
    fn seats_are_numbered_from_zero() {
        assert_eq!(Seat::try_new(2), None);
        assert_eq!(Seat::new(1).index(), 1);
    }
}
