//! [`Piles`]: the storage of the libraries, or of the graveyards, of a game, which is one list
//! for each player or one list both players share ([`crate::format::ZoneRule`]).
//!
//! Implements: none. Stores the ordered lists of the libraries and graveyards.

use std::fmt::Debug;
use std::hash::{Hash, Hasher};

use crate::fixed::Full;
use crate::ids::{LargeSlot, Slot, SlotId};
use crate::layout::Item;

/// The libraries, or the graveyards, of a game: list 0 and list 1, each bottom first. In a game
/// where each player has their own, player `p`'s is list `p`, which holds at most
/// [`SlotId::DECK_SLOTS`] cards. In the shared-deck format the one shared list is list 0, which
/// may hold [`SlotId::SHARED_DECK_SLOTS`], and list 1 stays empty.
///
/// The state picks the list with `player & mask` ([`crate::state::Zones`]), so a read is the same
/// in both formats. Only a push or an insert asks which format it is in, by its name: `push`
/// and `insert` keep a per-player list within its room, and `push_shared` and `insert_shared`
/// give list 0 the room of both.
///
/// Equality reads only the live items, and a hash feeds exactly what an array of two lists of
/// the layout's [`crate::Layout::Deck`] fed, so the hashes of per-player games do not change.
pub trait Piles<S: Item + Copy>: Clone + Debug + Default + Eq + Hash + Send + Sync {
    /// List `list`, bottom first.
    fn list(&self, list: usize) -> &[S];
    fn list_mut(&mut self, list: usize) -> &mut [S];
    /// Puts `item` on top of a per-player list, or returns [`Full`] if the list holds
    /// [`SlotId::DECK_SLOTS`] cards.
    fn push(&mut self, list: usize, item: S) -> Result<(), Full>;
    /// Inserts `item` at `index` of a per-player list, shifting later items up.
    fn insert(&mut self, list: usize, index: usize, item: S) -> Result<(), Full>;
    /// Puts `item` on top of the shared list, list 0.
    fn push_shared(&mut self, item: S) -> Result<(), Full>;
    /// Inserts `item` at `index` of the shared list, list 0.
    fn insert_shared(&mut self, index: usize, item: S) -> Result<(), Full>;
    /// Removes the item at `index` of `list`, keeping the order of the rest.
    fn remove(&mut self, list: usize, index: usize) -> S;
    fn pop(&mut self, list: usize) -> Option<S>;

    #[inline]
    fn position(&self, list: usize, item: S) -> Option<usize> {
        self.list(list).iter().position(|&x| x == item)
    }

    /// Removes `item` from `list` if it is there.
    #[inline]
    fn remove_item(&mut self, list: usize, item: S) {
        if let Some(i) = self.position(list, item) {
            self.remove(list, i);
        }
    }
}

/// [`crate::Small`]'s piles: one array with room for both players' lists and their two
/// lengths, as large as the two [`crate::fixed::Fixed`] lists it replaces. List `p` starts at
/// `p * DECK_SLOTS`, so the shared list, list 0, may run on over list 1's room.
#[derive(Clone, Copy)]
pub struct SmallPiles {
    items: [Slot; 2 * Slot::DECK_SLOTS],
    lens: [u8; 2],
}

const _: () = assert!(size_of::<SmallPiles>() == 2 * size_of::<crate::fixed::Fixed<Slot, 60>>());

impl SmallPiles {
    #[inline]
    fn start(list: usize) -> usize {
        list * Slot::DECK_SLOTS
    }

    /// Inserts `item` at `index` of `list`, which has room for `room` items.
    #[inline]
    fn insert_within(
        &mut self,
        list: usize,
        room: usize,
        index: usize,
        item: Slot,
    ) -> Result<(), Full> {
        let len = usize::from(self.lens[list]);
        if len == room {
            return Err(Full);
        }
        let start = Self::start(list);
        // A loop, as `Fixed::insert`'s, which counts fewer instructions than a `memmove` call
        // for the few items a library or a graveyard shifts.
        for i in (start + index..start + len).rev() {
            self.items[i + 1] = self.items[i];
        }
        self.items[start + index] = item;
        self.lens[list] += 1;
        Ok(())
    }

    #[inline]
    fn push_within(&mut self, list: usize, room: usize, item: Slot) -> Result<(), Full> {
        let len = usize::from(self.lens[list]);
        if len == room {
            return Err(Full);
        }
        self.items[Self::start(list) + len] = item;
        self.lens[list] += 1;
        Ok(())
    }
}

impl Default for SmallPiles {
    fn default() -> Self {
        SmallPiles {
            items: [Slot::default(); 2 * Slot::DECK_SLOTS],
            lens: [0; 2],
        }
    }
}

impl Piles<Slot> for SmallPiles {
    #[inline]
    fn list(&self, list: usize) -> &[Slot] {
        let start = Self::start(list);
        &self.items[start..start + usize::from(self.lens[list])]
    }

    #[inline]
    fn list_mut(&mut self, list: usize) -> &mut [Slot] {
        let start = Self::start(list);
        &mut self.items[start..start + usize::from(self.lens[list])]
    }

    #[inline]
    fn push(&mut self, list: usize, item: Slot) -> Result<(), Full> {
        self.push_within(list, Slot::DECK_SLOTS, item)
    }

    #[inline]
    fn insert(&mut self, list: usize, index: usize, item: Slot) -> Result<(), Full> {
        self.insert_within(list, Slot::DECK_SLOTS, index, item)
    }

    #[inline]
    fn push_shared(&mut self, item: Slot) -> Result<(), Full> {
        self.push_within(0, Slot::SHARED_DECK_SLOTS, item)
    }

    #[inline]
    fn insert_shared(&mut self, index: usize, item: Slot) -> Result<(), Full> {
        self.insert_within(0, Slot::SHARED_DECK_SLOTS, index, item)
    }

    #[inline]
    fn remove(&mut self, list: usize, index: usize) -> Slot {
        let start = Self::start(list);
        let len = usize::from(self.lens[list]);
        let item = self.list(list)[index];
        for i in start + index + 1..start + len {
            self.items[i - 1] = self.items[i];
        }
        self.lens[list] -= 1;
        item
    }

    #[inline]
    fn pop(&mut self, list: usize) -> Option<Slot> {
        let len = self.lens[list].checked_sub(1)?;
        self.lens[list] = len;
        Some(self.items[Self::start(list) + usize::from(len)])
    }
}

impl PartialEq for SmallPiles {
    fn eq(&self, other: &Self) -> bool {
        self.list(0) == other.list(0) && self.list(1) == other.list(1)
    }
}

impl Eq for SmallPiles {}

/// What `[Fixed<Slot, 60>; 2]` fed: the array's length, then each list's length and items.
impl Hash for SmallPiles {
    fn hash<H: Hasher>(&self, h: &mut H) {
        [self.list(0), self.list(1)].hash(h);
    }
}

impl Debug for SmallPiles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entry(&self.list(0))
            .entry(&self.list(1))
            .finish()
    }
}

/// [`crate::Large`]'s piles: two `Vec`s, of which a shared game uses the first.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct LargePiles([Vec<LargeSlot>; 2]);

impl Piles<LargeSlot> for LargePiles {
    fn list(&self, list: usize) -> &[LargeSlot] {
        &self.0[list]
    }

    fn list_mut(&mut self, list: usize) -> &mut [LargeSlot] {
        &mut self.0[list]
    }

    fn push(&mut self, list: usize, item: LargeSlot) -> Result<(), Full> {
        self.0[list].push(item);
        Ok(())
    }

    fn insert(&mut self, list: usize, index: usize, item: LargeSlot) -> Result<(), Full> {
        self.0[list].insert(index, item);
        Ok(())
    }

    fn push_shared(&mut self, item: LargeSlot) -> Result<(), Full> {
        self.push(0, item)
    }

    fn insert_shared(&mut self, index: usize, item: LargeSlot) -> Result<(), Full> {
        self.insert(0, index, item)
    }

    fn remove(&mut self, list: usize, index: usize) -> LargeSlot {
        self.0[list].remove(index)
    }

    fn pop(&mut self, list: usize) -> Option<LargeSlot> {
        self.0[list].pop()
    }
}

#[cfg(test)]
mod tests;
