//! Storage for the creatures a blocker was declared against.
//!
//! Implements: none. Stores combat relations chosen by the rules.

use crate::fixed::Full;
use crate::ids::SlotId;
use crate::layout::{Layout, Small};

/// Layout storage for the attackers a creature blocks, in declaration order.
pub trait BlockList<S: SlotId>: crate::layout::Item {
    const EMPTY: Self;
    const CAPACITY: usize;

    fn position(&self, slot: S) -> Option<usize>;
    fn get(&self, index: usize) -> Option<S>;
    fn push(&mut self, slot: S) -> Result<(), Full>;
    fn remove(&mut self, index: usize);
}

impl<S: SlotId> BlockList<S> for [Option<S>; 3] {
    const EMPTY: Self = [None; 3];
    const CAPACITY: usize = 3;

    #[inline]
    fn position(&self, slot: S) -> Option<usize> {
        self.iter().position(|&s| s == Some(slot))
    }

    #[inline]
    fn get(&self, index: usize) -> Option<S> {
        self.as_slice().get(index).copied().flatten()
    }

    #[inline]
    fn push(&mut self, slot: S) -> Result<(), Full> {
        let free = self.iter_mut().find(|s| s.is_none()).ok_or(Full)?;
        *free = Some(slot);
        Ok(())
    }

    #[inline]
    fn remove(&mut self, index: usize) {
        self.copy_within(index + 1.., index);
        self[Self::CAPACITY - 1] = None;
    }
}

impl<S: SlotId> BlockList<S> for Vec<S> {
    const EMPTY: Self = Vec::new();
    const CAPACITY: usize = usize::MAX;

    fn position(&self, slot: S) -> Option<usize> {
        self.iter().position(|&s| s == slot)
    }

    fn get(&self, index: usize) -> Option<S> {
        self.as_slice().get(index).copied()
    }

    fn push(&mut self, slot: S) -> Result<(), Full> {
        self.push(slot);
        Ok(())
    }

    fn remove(&mut self, index: usize) {
        self.remove(index);
    }
}

/// The attackers a blocking creature blocks, in declaration order. A creature blocks
/// one attacker unless an effect lets it block more (see [CR 509.1a]).
/// [`Small`] holds three attackers, and [`Large`](crate::Large) grows as needed.
///
/// [CR 509.1a]: https://yawgatog.com/resources/magic-rules/#R5091a
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SlotSet<L: Layout = Small>(L::BlockList);

impl<L: Layout> Copy for SlotSet<L> where L::BlockList: Copy {}

impl<L: Layout> SlotSet<L> {
    pub const CAPACITY: usize = L::BlockList::CAPACITY;
    pub const EMPTY: SlotSet<L> = SlotSet(L::BlockList::EMPTY);

    #[inline]
    pub fn contains(&self, slot: L::Slot) -> bool {
        self.0.position(slot).is_some()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.first().is_none()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.iter().count()
    }

    /// The first slot added that is still in the set.
    #[inline]
    pub fn first(&self) -> Option<L::Slot> {
        self.0.get(0)
    }

    /// Whether the creature has at least two attackers to divide damage among.
    #[inline]
    pub fn is_multiple(&self) -> bool {
        self.0.get(1).is_some()
    }

    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = L::Slot> + '_ {
        (0..).map_while(|index| self.0.get(index))
    }

    /// Adds `slot` after the others, unless it is in the set already.
    #[inline]
    pub(crate) fn insert(&mut self, slot: L::Slot) -> Result<(), Full> {
        if self.contains(slot) {
            return Ok(());
        }
        self.0.push(slot)
    }

    #[inline]
    pub(crate) fn remove(&mut self, slot: L::Slot) {
        if let Some(index) = self.0.position(slot) {
            self.0.remove(index);
        }
    }

    #[inline]
    pub(super) fn map<M: Layout>(&self, f: impl Fn(L::Slot) -> M::Slot) -> SlotSet<M> {
        let mut result = SlotSet::<M>::EMPTY;
        for slot in self.iter() {
            // State::fits checks the length before narrowing a game.
            assert!(result.0.push(f(slot)).is_ok(), "too many blocked attackers");
        }
        result
    }
}
