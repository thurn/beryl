//! `Layout`: how a [`State`] stores its objects and queues (architecture §4.1).
//!
//! The engine is generic over the layout and resolves it at compile time, so no hot path asks
//! which one it runs on. [`Small`] is the fixed-capacity layout search runs on: one flat
//! `Copy` value, copied with one `memcpy` (decisions D3 and D4). [`Large`] grows its queues
//! instead, so a game that outgrows `Small` goes on in it ([`State::widen`]).

use std::fmt::Debug;
use std::hash::Hash;
use std::ops::{Index, IndexMut};

use crate::bitboard::Bitboard;
use crate::fixed::{Fixed, Full};
use crate::ids::{CardId, CardIx, LargeCardIx, LargeSlot, Seat, Slot, SlotId};
use crate::relayout::Relayout;
use crate::state::State;

/// What a layout's arrays and queues hold: in [`Small`], always `Copy` values; in [`Large`],
/// values that may hold queues of their own.
pub trait Item: Clone + Debug + Eq + Hash + Send + Sync {}

impl<T: Clone + Debug + Eq + Hash + Send + Sync> Item for T {}

/// An ordered list of items, as [`Fixed`]. A push may fail with [`Full`], which the engine
/// turns into a stall.
pub trait Queue<T: Item>: Clone + Debug + Default + Eq + Hash + Send + Sync {
    /// The most items it holds, or `usize::MAX` for one that grows.
    const CAPACITY: usize;

    fn as_slice(&self) -> &[T];
    fn as_mut_slice(&mut self) -> &mut [T];
    fn push(&mut self, item: T) -> Result<(), Full>;

    /// Pushes onto a list that cannot be full, by an argument the caller's comment makes.
    /// A debug build checks the argument and panics on a wrong one, so the scenarios and
    /// `just fuzz` trip on it. A list with no such argument propagates [`Full`]: rules code
    /// never throws a failed push away (the `discarded-push` check).
    #[inline]
    #[track_caller]
    fn push_bounded(&mut self, item: T) {
        let pushed = self.push(item);
        debug_assert!(pushed.is_ok(), "a list argued never to fill is full");
    }

    /// Pushes if there is room, and says whether there was: for a list a heuristic samples,
    /// where what does not fit is not considered. Never for what a rule reads.
    #[inline]
    fn push_if_room(&mut self, item: T) -> bool {
        self.push(item).is_ok()
    }

    /// Inserts at `index`, shifting later items up.
    fn insert(&mut self, index: usize, item: T) -> Result<(), Full>;
    fn pop(&mut self) -> Option<T>;
    /// Removes the item at `index`, keeping the order of the rest.
    fn remove(&mut self, index: usize) -> T;
    fn clear(&mut self);
    /// Keeps only the items for which `keep` returns true, in order.
    fn retain(&mut self, keep: impl FnMut(&T) -> bool);

    #[inline]
    fn len(&self) -> usize {
        self.as_slice().len()
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    fn iter(&self) -> std::slice::Iter<'_, T> {
        self.as_slice().iter()
    }

    #[inline]
    fn position(&self, item: T) -> Option<usize> {
        self.iter().position(|x| *x == item)
    }
}

/// A table with one entry per index, as `[T; N]`.
pub trait Table<T: Item>:
    Clone + Debug + Eq + Hash + Send + Sync + Index<usize, Output = T> + IndexMut<usize>
{
    /// A table whose entry at each index is `f` of it.
    fn from_fn(f: impl FnMut(usize) -> T) -> Self;

    /// A table whose every entry is `item`.
    #[inline]
    fn filled(item: T) -> Self {
        Self::from_fn(|_| item.clone())
    }

    fn as_slice(&self) -> &[T];
    fn as_mut_slice(&mut self) -> &mut [T];

    #[inline]
    fn iter(&self) -> std::slice::Iter<'_, T> {
        self.as_slice().iter()
    }
}

/// A family of types with one member for each layout, such as "a function over a state of
/// this layout". Static data that holds a value per layout, since it can hold only concrete
/// types, names its family so [`Layout::of`] can hand a generic caller its own.
pub trait PerLayout {
    /// The family's member for layout `L`.
    type Of<L: Layout>;
}

/// `i` as the layout's [`Layout::Index`].
///
/// # Panics
///
/// If the layout's index cannot name `i`: never for an index into one of its queues.
#[inline]
pub fn index<L: Layout>(i: usize) -> L::Index {
    L::Index::try_from(i).unwrap_or_else(|_| panic!("index {i} past the layout's"))
}

/// One value for each seat there is ([`Seat`] names two), indexed by [`Seat::index`]: the
/// per-player table of both layouts ([`Layout::PerPlayer`]), and of what is the same in every
/// layout, as a [`crate::profile::Profile`]'s stops and a [`crate::GameTable`]'s decks are.
pub type PerSeat<T> = [T; Seat::BOTH.len()];

/// One value for each player of a game in layout `L`, indexed by [`Seat::index`]
/// ([`Layout::PerPlayer`]).
pub type PerPlayer<L, T> = <L as Layout>::PerPlayer<T>;

/// A storage layout for [`State`]: the types of its slots, tables and queues. `N` is the
/// capacity a fixed layout gives the table or queue.
///
/// What a layout sets, and nothing outside it assumes:
///
/// - how many players a game has ([`SlotId::PLAYERS`] of its slots, the length of its
///   [`Layout::PerPlayer`]);
/// - how many cards a deck may have, and so a library and a graveyard
///   ([`SlotId::DECK_SLOTS`], the capacity of its [`Layout::Deck`]);
/// - how many tokens and spell copies exist at once (its [`Layout::TOKEN_POOL`]);
/// - how many objects a game holds ([`SlotId::COUNT`]) and how long each queue grows;
/// - how many distinct cards a game contains, tokens and emblems among them
///   ([`CardId::COUNT`] of its [`Layout::Card`]).
///
/// A game too large for a layout is refused where a state of it is made, with a [`Misfit`]
/// ([`State::deal`], [`State::try_relayout`]).
///
/// [`Misfit`]: crate::Misfit
pub trait Layout: Copy + Debug + Default + Eq + Hash + Send + Sync + 'static {
    /// The object slots: how many objects a game holds, and how they are dealt among the
    /// players' decks and the pool ([`SlotId`]).
    type Slot: SlotId;
    /// A card's index in the game table, as the state stores it for an object, a spell or
    /// ability, an effect or an emblem: how many distinct cards a game contains
    /// ([`CardId`]).
    type Card: CardId;
    /// An index into a queue or a decision's options, as the state stores it: in a choice
    /// record or an [`Opt::Trigger`](crate::decision::Opt::Trigger).
    type Index: Item + Copy + Default + Ord + Into<usize> + Into<u16> + TryFrom<usize>;
    type Queue<T: Item + Default, const N: usize>: Queue<T>;
    type Table<T: Item, const N: usize>: Table<T>;
    /// A list as long as a deck may be ([`SlotId::DECK_SLOTS`]): a library, a graveyard, the
    /// cards of one being dealt.
    type Deck<T: Item + Default>: Queue<T>;
    /// One value for each player, indexed by [`Seat::index`]: as many entries as
    /// [`SlotId::PLAYERS`].
    type PerPlayer<T: Item>: Table<T>;

    /// The slots of the shared pool for tokens and spell copies, after every deck's
    /// ([`SlotId::pool`]).
    const TOKEN_POOL: Bitboard<Self::Slot>;

    /// A copy of `state`: for [`Small`], one `memcpy`.
    fn clone_state(state: &State<Self>) -> State<Self>;

    /// Of a value written once for each layout, this layout's: how code generic over the
    /// layout reads static data, which holds one concrete value per layout.
    fn of<F: PerLayout>(small: F::Of<Small>, large: F::Of<Large>) -> F::Of<Self>;
}

/// Today's layout: [`Fixed`] queues and arrays, so a [`State<Small>`] is `Copy` and at most
/// 20 KiB. Search and rollouts run on it.
///
/// It is two players with 60 slots a deck, which is the least a constructed deck may have
/// ([CR 100.2a](https://yawgatog.com/resources/magic-rules/#R1002a): "each deck has a minimum
/// deck size of 60 cards"), and 135 slots for tokens and spell copies. A game between larger
/// decks is refused here and dealt in [`Large`] ([`State::deal`]), and so is one whose table
/// has more than the 255 distinct cards a one-byte [`CardIx`] numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Small;

impl Layout for Small {
    type Slot = Slot;
    type Card = CardIx;
    type Index = u8;
    type Queue<T: Item + Default, const N: usize> = Fixed<T, N>;
    type Table<T: Item, const N: usize> = [T; N];
    type Deck<T: Item + Default> = Fixed<T, { Slot::DECK_SLOTS }>;
    type PerPlayer<T: Item> = PerSeat<T>;

    const TOKEN_POOL: Bitboard =
        Bitboard([0, !0 << (Slot::POOL - 64), !0, !0 >> (256 - Slot::COUNT)]);

    #[inline]
    fn clone_state(state: &State<Self>) -> State<Self> {
        *state
    }

    #[inline]
    fn of<F: PerLayout>(small: F::Of<Small>, _large: F::Of<Large>) -> F::Of<Small> {
        small
    }
}

/// The layout a game outgrowing [`Small`] goes on in, and the one a game too large for
/// `Small` begins in: its queues are `Vec`s, whose push never fails, and its tables have an
/// entry for every slot, so the battlefield can hold every object. It has four times
/// [`Small`]'s slots ([`LargeSlot`]), and its indices are `u16`, so a decision may offer more
/// options than a `Small` choice record could name. A [`State<Large>`] is `Clone`, not
/// `Copy`.
///
/// A deck may have 256 cards. No rule sets that
/// ([CR 100.5](https://yawgatog.com/resources/magic-rules/#R1005): "There is no maximum deck
/// size for non-Commander decks."); it is the share of the 1,024 slots each deck is given,
/// which leaves 512 for the pool of tokens and spell copies, nearly four times `Small`'s
/// 135. Its game table may hold 65,535 distinct cards, tokens and emblems: what the two
/// bytes of a [`LargeCardIx`] number, which is room for a table over a whole card pool and
/// not only two decks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Large;

impl Layout for Large {
    type Slot = LargeSlot;
    type Card = LargeCardIx;
    type Index = u16;
    type Queue<T: Item + Default, const N: usize> = Vec<T>;
    type Table<T: Item, const N: usize> = PerSlot<T>;
    type Deck<T: Item + Default> = Vec<T>;
    type PerPlayer<T: Item> = PerSeat<T>;

    const TOKEN_POOL: Bitboard<LargeSlot> = {
        assert!(LargeSlot::POOL.is_multiple_of(64) && LargeSlot::COUNT == 16 * 64);
        let mut words = [0; 16];
        let mut word = LargeSlot::POOL / 64;
        while word < 16 {
            words[word] = !0;
            word += 1;
        }
        Bitboard(words)
    };

    fn clone_state(state: &State<Self>) -> State<Self> {
        let State {
            objects,
            battlefield,
            derived,
            listening,
            replacing,
            static_rules,
            zones,
            players,
            turn,
            stack,
            triggers,
            delayed,
            effects,
            keyed,
            memos,
            counters,
            trackers,
            pending,
            knowledge,
            rng,
            profile,
            budgets,
            flags,
            outcome,
        } = state;
        State {
            objects: objects.clone(),
            battlefield: battlefield.clone(),
            derived: derived.clone(),
            listening: *listening,
            replacing: *replacing,
            static_rules: *static_rules,
            zones: zones.clone(),
            players: players.clone(),
            turn: *turn,
            stack: stack.clone(),
            triggers: triggers.clone(),
            delayed: delayed.clone(),
            effects: effects.clone(),
            keyed: *keyed,
            memos: memos.clone(),
            counters: counters.clone(),
            trackers: trackers.clone(),
            pending: pending.clone(),
            knowledge: *knowledge,
            rng: *rng,
            profile: *profile,
            budgets: *budgets,
            flags: *flags,
            outcome: *outcome,
        }
    }

    #[inline]
    fn of<F: PerLayout>(_small: F::Of<Small>, large: F::Of<Large>) -> F::Of<Large> {
        large
    }
}

impl State<Small> {
    /// This game in the [`Large`] layout, losslessly ([`Relayout`]): every slot keeps its
    /// place and object, every queue its order, every player their knowledge, and the random
    /// streams their place, so the game goes on exactly as it would have, past where `Small`
    /// runs out.
    pub fn widen(&self) -> State<Large> {
        self.relayout()
    }
}

impl<T: Item + Default, const N: usize> Queue<T> for Fixed<T, N> {
    const CAPACITY: usize = N;

    #[inline]
    fn as_slice(&self) -> &[T] {
        Fixed::as_slice(self)
    }

    #[inline]
    fn as_mut_slice(&mut self) -> &mut [T] {
        Fixed::as_mut_slice(self)
    }

    #[inline]
    fn push(&mut self, item: T) -> Result<(), Full> {
        Fixed::push(self, item)
    }

    #[inline]
    fn insert(&mut self, index: usize, item: T) -> Result<(), Full> {
        Fixed::insert(self, index, item)
    }

    #[inline]
    fn pop(&mut self) -> Option<T> {
        Fixed::pop(self)
    }

    #[inline]
    fn remove(&mut self, index: usize) -> T {
        Fixed::remove(self, index)
    }

    #[inline]
    fn clear(&mut self) {
        Fixed::clear(self);
    }

    #[inline]
    fn retain(&mut self, keep: impl FnMut(&T) -> bool) {
        Fixed::retain(self, keep);
    }

    #[inline]
    fn len(&self) -> usize {
        Fixed::len(self)
    }

    #[inline]
    fn is_empty(&self) -> bool {
        Fixed::is_empty(self)
    }
}

impl<T: Item, const N: usize> Table<T> for [T; N] {
    #[inline]
    fn from_fn(f: impl FnMut(usize) -> T) -> Self {
        std::array::from_fn(f)
    }

    #[inline]
    fn as_slice(&self) -> &[T] {
        self
    }

    #[inline]
    fn as_mut_slice(&mut self) -> &mut [T] {
        self
    }
}

impl<T: Item + Default> Queue<T> for Vec<T> {
    const CAPACITY: usize = usize::MAX;

    fn as_slice(&self) -> &[T] {
        self
    }

    fn as_mut_slice(&mut self) -> &mut [T] {
        self
    }

    fn push(&mut self, item: T) -> Result<(), Full> {
        Vec::push(self, item);
        Ok(())
    }

    fn insert(&mut self, index: usize, item: T) -> Result<(), Full> {
        Vec::insert(self, index, item);
        Ok(())
    }

    fn pop(&mut self) -> Option<T> {
        Vec::pop(self)
    }

    fn remove(&mut self, index: usize) -> T {
        Vec::remove(self, index)
    }

    fn clear(&mut self) {
        Vec::clear(self);
    }

    fn retain(&mut self, keep: impl FnMut(&T) -> bool) {
        Vec::retain(self, keep);
    }
}

/// [`Large`]'s table: an entry for every slot, on the heap.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PerSlot<T>(Box<[T]>);

impl<T> Index<usize> for PerSlot<T> {
    type Output = T;

    fn index(&self, index: usize) -> &T {
        &self.0[index]
    }
}

impl<T> IndexMut<usize> for PerSlot<T> {
    fn index_mut(&mut self, index: usize) -> &mut T {
        &mut self.0[index]
    }
}

impl<T: Item> Table<T> for PerSlot<T> {
    fn from_fn(f: impl FnMut(usize) -> T) -> Self {
        PerSlot((0..LargeSlot::COUNT).map(f).collect())
    }

    fn filled(item: T) -> Self {
        PerSlot(vec![item; LargeSlot::COUNT].into_boxed_slice())
    }

    fn as_slice(&self) -> &[T] {
        &self.0
    }

    fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.0
    }
}
