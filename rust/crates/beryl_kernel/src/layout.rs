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
use crate::ids::{LargeSlot, Slot, SlotId};
use crate::relayout::Relayout;
use crate::state::{State, TOKEN_SLOTS};

/// What a layout's arrays and queues hold: in [`Small`], always `Copy` values; in [`Large`],
/// values that may hold queues of their own.
pub trait Item: Clone + Debug + Eq + Hash + Send + Sync {}

impl<T: Clone + Debug + Eq + Hash + Send + Sync> Item for T {}

/// An ordered list of items, as [`Fixed`]. A push may fail with [`Full`], which the engine
/// turns into a stall.
pub trait Queue<T: Item>: Clone + Debug + Default + Eq + Hash + Send + Sync {
    fn as_slice(&self) -> &[T];
    fn as_mut_slice(&mut self) -> &mut [T];
    fn push(&mut self, item: T) -> Result<(), Full>;
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
    /// A table whose every entry is `item`.
    fn filled(item: T) -> Self;
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

/// A storage layout for [`State`]: the types of its slots, tables and queues. `N` is the
/// capacity a fixed layout gives the table or queue.
pub trait Layout: Copy + Debug + Default + Eq + Hash + Send + Sync + 'static {
    /// The object slots, and so how many objects a game holds.
    type Slot: SlotId;
    /// An index into a queue or a decision's options, as the state stores it: in a choice
    /// record or an [`Opt::Trigger`](crate::decision::Opt::Trigger).
    type Index: Item + Copy + Default + Ord + Into<usize> + Into<u16> + TryFrom<usize>;
    type Queue<T: Item + Default, const N: usize>: Queue<T>;
    type Table<T: Item, const N: usize>: Table<T>;

    /// The slots of the shared pool for tokens and spell copies, after both decks'.
    const TOKEN_POOL: Bitboard<Self::Slot>;

    /// A copy of `state`: for [`Small`], one `memcpy`.
    fn clone_state(state: &State<Self>) -> State<Self>;

    /// Of a value written once for each layout, this layout's: how code generic over the
    /// layout reads static data, which holds one concrete value per layout.
    fn of<F: PerLayout>(small: F::Of<Small>, large: F::Of<Large>) -> F::Of<Self>;
}

/// Today's layout: [`Fixed`] queues and arrays, so a [`State<Small>`] is `Copy` and at most
/// 20 KiB. Search and rollouts run on it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Small;

impl Layout for Small {
    type Slot = Slot;
    type Index = u8;
    type Queue<T: Item + Default, const N: usize> = Fixed<T, N>;
    type Table<T: Item, const N: usize> = [T; N];

    const TOKEN_POOL: Bitboard =
        Bitboard([0, !0 << (TOKEN_SLOTS - 64), !0, !0 >> (256 - Slot::COUNT)]);

    #[inline]
    fn clone_state(state: &State<Self>) -> State<Self> {
        *state
    }

    #[inline]
    fn of<F: PerLayout>(small: F::Of<Small>, _large: F::Of<Large>) -> F::Of<Small> {
        small
    }
}

/// The layout a game outgrowing [`Small`] goes on in: its queues are `Vec`s, whose push never
/// fails, and its tables have an entry for every slot, so the battlefield can hold every
/// object. It has four times [`Small`]'s slots ([`LargeSlot`]), and its indices are `u16`, so
/// a decision may offer more options than a `Small` choice record could name. A
/// [`State<Large>`] is `Clone`, not `Copy`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Large;

impl Layout for Large {
    type Slot = LargeSlot;
    type Index = u16;
    type Queue<T: Item + Default, const N: usize> = Vec<T>;
    type Table<T: Item, const N: usize> = PerSlot<T>;

    const TOKEN_POOL: Bitboard<LargeSlot> = {
        let mut words = [!0; 16];
        words[0] = 0;
        words[1] = !0 << (TOKEN_SLOTS - 64);
        Bitboard(words)
    };

    fn clone_state(state: &State<Self>) -> State<Self> {
        let State {
            objects,
            battlefield,
            derived,
            listening,
            zones,
            players,
            turn,
            stack,
            triggers,
            delayed,
            effects,
            permissions,
            entered_x,
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
            zones: zones.clone(),
            players: players.clone(),
            turn: *turn,
            stack: stack.clone(),
            triggers: triggers.clone(),
            delayed: delayed.clone(),
            effects: effects.clone(),
            permissions: permissions.clone(),
            entered_x: entered_x.clone(),
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
    /// number and object, every queue its order, every player their knowledge, and the random
    /// streams their place, so the game goes on exactly as it would have, past where `Small`
    /// runs out.
    pub fn widen(&self) -> State<Large> {
        self.relayout()
    }
}

impl<T: Item + Default, const N: usize> Queue<T> for Fixed<T, N> {
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
    fn filled(item: T) -> Self {
        std::array::from_fn(|_| item.clone())
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
