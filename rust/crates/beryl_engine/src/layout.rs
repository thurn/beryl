//! `Layout`: how a [`State`] stores its objects and queues (architecture §4.1).
//!
//! The engine is generic over the layout and resolves it at compile time, so no hot path asks
//! which one it runs on. [`Small`] is the fixed-capacity layout search runs on: one flat
//! `Copy` value, copied with one `memcpy` (decisions D3 and D4).

use std::fmt::Debug;
use std::hash::Hash;
use std::ops::{Index, IndexMut};

use crate::card::{Ability, Flow};
use crate::effect::EffectCtx;
use crate::fixed::{Fixed, Full};
use crate::state::{PendingTrigger, State};
use crate::table::GameTable;

/// What a layout's arrays and queues hold.
pub trait Item: Copy + Debug + Eq + Hash + Send + Sync {}

impl<T: Copy + Debug + Eq + Hash + Send + Sync> Item for T {}

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
        self.iter().position(|&x| x == item)
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

/// The body of an ability, run on a state of layout `L`.
pub type Body<L = Small> = for<'a, 'b> fn(&'b mut EffectCtx<'a, L>) -> Flow;

/// A triggered ability's intervening "if" clause, read on a state of layout `L`.
pub type Condition<L = Small> = fn(&State<L>, &GameTable, &PendingTrigger) -> bool;

/// A storage layout for [`State`]: the types of its tables and queues. `N` is the capacity a
/// fixed layout gives the table or queue.
pub trait Layout: Copy + Debug + Default + Eq + Hash + Send + Sync + 'static {
    type Queue<T: Item + Default, const N: usize>: Queue<T>;
    type Table<T: Item, const N: usize>: Table<T>;

    /// A copy of `state`: for [`Small`], one `memcpy`.
    fn clone_state(state: &State<Self>) -> State<Self>;

    /// The body of `ability`, for states of this layout. Card definitions are static data,
    /// so each layout reads its own instance of the body from them.
    fn body(ability: &Ability) -> Body<Self>;

    /// The intervening "if" clause of `ability`, for states of this layout, as [`Self::body`].
    fn condition(ability: &Ability) -> Option<Condition<Self>>;
}

/// Today's layout: [`Fixed`] queues and arrays, so a [`State<Small>`] is `Copy` and at most
/// 20 KiB. Search and rollouts run on it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Small;

impl Layout for Small {
    type Queue<T: Item + Default, const N: usize> = Fixed<T, N>;
    type Table<T: Item, const N: usize> = [T; N];

    #[inline]
    fn clone_state(state: &State<Self>) -> State<Self> {
        *state
    }

    #[inline]
    fn body(ability: &Ability) -> Body<Self> {
        ability.body
    }

    #[inline]
    fn condition(ability: &Ability) -> Option<Condition<Self>> {
        ability.condition
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
        [item; N]
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
