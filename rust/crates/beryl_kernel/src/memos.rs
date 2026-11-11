//! The Memos store: what an object remembers of how it was cast or entered (plan K4; the
//! crate's contract, "Stores").
//!
//! A memo is written once, with [`State::remember`], and read by the object's own abilities
//! through a query by kind: [`State::paid`], [`State::x_of`]. It ends with its object,
//! inside the zone move
//! ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007): "An object that moves
//! from one zone to another becomes a new object with no memory of, or relation to, its
//! previous existence."), so an entry names a slot and is always about the object now in
//! it. A kind with nothing live costs its reader one AND.
//!
//! Built so far (by-w8gv.7.6): what a permanent remembers of the spell it was, the costs
//! paid for it and the value of X. The cards an object exiled, a memo of the exiled card
//! naming the slot of what exiled it, are designed in the contract and built by
//! by-w8gv.7.7.

use beryl_card::{CastingKind, CastingKinds};

use crate::bitboard::Bitboard;
use crate::fit::{Misfit, room};
use crate::fixed::Full;
use crate::ids::SlotId;
use crate::invariants::{At, Violation, check};
use crate::layout::{Layout, Queue, Small};
use crate::observation::Observation;
use crate::relayout::{Relayout, queue, slot};
use crate::state::{State, Zone};
use crate::store::{Duration, Ended, Store};
use crate::table::GameTable;

/// The memos a [`Small`] game holds at once. One past it stalls the game into
/// [`crate::Large`], whose store grows.
pub const MEMOS: usize = 32;

/// One thing an object remembers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Memo {
    /// The permanent's spell was cast with these kinds of cost paid, a kicker cost among
    /// them or an evoke cost
    /// ([CR 702.33d](https://yawgatog.com/resources/magic-rules/#R70233d): "If a spell's
    /// controller declares the intention to pay any of that spell's kicker costs, that
    /// spell has been 'kicked.'"). Never the empty set: a permanent whose spell paid none
    /// remembers nothing.
    Paid(CastingKinds),
    /// The value of X the permanent's spell was cast with, for its enters abilities
    /// ([CR 107.3m](https://yawgatog.com/resources/magic-rules/#R1073m)): "the value of X
    /// for that ability is the same as the value of X for that spell". Never 0, which is
    /// what a permanent with no such memo answers.
    X(u8),
}

impl Memo {
    /// The bits of its kinds in a store's [`Kinds`].
    #[inline]
    fn kinds(self) -> u16 {
        match self {
            Memo::Paid(paid) => Kinds::paid(paid),
            Memo::X(_) => Kinds::X,
        }
    }

    /// Whether the layers read it: a static ability may apply only while a cost was paid,
    /// as one with impending does
    /// ([CR 702.176a](https://yawgatog.com/resources/magic-rules/#R702176a)).
    #[inline]
    fn layer_input(self) -> bool {
        matches!(self, Memo::Paid(_))
    }

    /// Whether `other` is of the same variant: an object has one memo of each.
    #[inline]
    fn same_variant(self, other: Memo) -> bool {
        std::mem::discriminant(&self) == std::mem::discriminant(&other)
    }
}

/// The kinds of memo among a store's entries: its index by kind, so that a reader of a kind
/// with nothing live stops at one AND. One bit for the value of X and one for each kind of
/// cost the vocabulary's list has.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Kinds(u16);

impl Kinds {
    const X: u16 = 1 << 0;

    /// The bits of the kinds of cost in `paid`.
    #[inline]
    const fn paid(paid: CastingKinds) -> u16 {
        (paid.bits() as u16) << 1
    }
}

/// An entry: a memo of the object now in a slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Remembered<L: Layout = Small> {
    slot: L::Slot,
    memo: Memo,
}

/// An entry is 3 bytes in [`Small`] and the store 100: the state's room (the crate's
/// contract, "Stores") counts on at most 4 and 136.
const _: () = assert!(size_of::<Remembered<Small>>() <= 4 && size_of::<Memos<Small>>() <= 136);

impl<L: Layout> Default for Remembered<L> {
    fn default() -> Self {
        Remembered {
            slot: L::Slot::default(),
            memo: Memo::X(0),
        }
    }
}

/// The Memos store: the entries in the order they were written, and the kinds among them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Memos<L: Layout = Small> {
    entries: L::Queue<Remembered<L>, MEMOS>,
    kinds: Kinds,
}

impl Copy for Memos<Small> {}

/// The entries alone: the kinds are an index of them, and a hash of a state is a hash of
/// the game.
impl<L: Layout> std::hash::Hash for Memos<L> {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.entries.hash(h);
    }
}

impl<L: Layout> Memos<L> {
    /// The entries, if one of them has a memo of a kind in `kinds`; none otherwise.
    #[inline]
    fn of(&self, kinds: u16) -> &[Remembered<L>] {
        if self.kinds.0 & kinds == 0 {
            return &[];
        }
        self.entries.as_slice()
    }
}

impl<L: Layout> Store<L> for Memos<L> {
    fn empty() -> Self {
        Memos {
            entries: Default::default(),
            kinds: Kinds::default(),
        }
    }

    fn keys(&self) -> Bitboard<L::Slot> {
        self.entries.iter().map(|e| e.slot).collect()
    }

    fn forget(&mut self, slot: L::Slot) -> Ended {
        let mut done = Ended::default();
        let mut kinds = 0;
        self.entries.retain(|e| {
            let kept = e.slot != slot;
            if kept {
                kinds |= e.memo.kinds();
            } else {
                done.any = true;
                done.layer_input |= e.memo.layer_input();
            }
            kept
        });
        self.kinds = Kinds(kinds);
        done
    }

    /// Nothing: a memo lasts as long as its object and no longer.
    #[inline]
    fn expire(&mut self, _: Duration) -> Ended {
        Ended::default()
    }

    fn fits<M: Layout>(&self) -> Result<(), Misfit> {
        room(
            "the memos",
            self.entries.len(),
            <M::Queue<Remembered<M>, MEMOS> as Queue<_>>::CAPACITY,
        )
    }

    fn check(&self, s: &State<L>, _: &GameTable) -> Result<(), Violation> {
        let mut kinds = 0;
        for (i, e) in self.entries.iter().enumerate() {
            kinds |= e.memo.kinds();
            let at = Some(At::Slot(e.slot.index()));
            check(
                s.object(e.slot).zone == Zone::Battlefield,
                "a memo of its spell on what is no permanent",
                at,
            )?;
            check(
                !matches!(e.memo, Memo::X(0)) && e.memo != Memo::Paid(CastingKinds::NONE),
                "a memo of nothing",
                at,
            )?;
            let again = |earlier: &Remembered<L>| {
                earlier.slot == e.slot && earlier.memo.same_variant(e.memo)
            };
            check(
                !self.entries.as_slice()[..i].iter().any(again),
                "an object remembers one thing twice",
                at,
            )?;
        }
        check(
            kinds == self.kinds.0,
            "the kinds of memo disagree with the memos",
            None,
        )
    }

    /// Nothing: a checkpoint of the board keeps the one memo read after the turn a
    /// permanent entered, the cost its static abilities ask about, with the permanent. The
    /// rest are read by its enters abilities alone, which have left the stack by then.
    fn lingering(&self, _: &State<L>, _: &GameTable) -> Option<String> {
        None
    }
}

impl<L: Layout> Relayout for Memos<L> {
    type In<M: Layout> = Memos<M>;

    fn relayout<M: Layout>(&self) -> Memos<M> {
        Memos {
            entries: queue(self.entries.as_slice(), |e| Remembered {
                slot: slot(e.slot),
                memo: e.memo,
            }),
            kinds: self.kinds,
        }
    }
}

/// What the Memos store holds about one object, to compare two objects by
/// ([`State::signature`]): equal when the two remember the same things, written in the
/// same order.
#[derive(Clone, Copy, Debug)]
pub struct MemosOf<'a, L: Layout = Small> {
    /// The store's entries, or none where no store has an entry about the object.
    entries: &'a [Remembered<L>],
    slot: L::Slot,
}

impl<L: Layout> MemosOf<'_, L> {
    /// The object's memos, in the order they were written.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = Memo> + '_ {
        let slot = self.slot;
        self.entries
            .iter()
            .filter(move |e| e.slot == slot)
            .map(|e| e.memo)
    }

    /// Whether the object remembers a cost of this kind paid for its spell.
    #[inline]
    pub fn paid(&self, kind: CastingKind) -> bool {
        self.iter()
            .any(|memo| matches!(memo, Memo::Paid(paid) if paid.contains(kind)))
    }
}

impl<L: Layout> PartialEq for MemosOf<'_, L> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter())
    }
}

impl<L: Layout> Eq for MemosOf<'_, L> {}

impl<L: Layout> State<L> {
    /// Has the permanent in `slot` remember `memo` of the spell it was, as it enters. It
    /// remembers until it leaves the battlefield
    /// ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)), and one thing of
    /// each variant of [`Memo`]. The layer results are stale from here if the layers read
    /// the memo. No event: it is part of how the permanent entered.
    #[inline]
    pub fn remember(&mut self, slot: L::Slot, memo: Memo) -> Result<(), Full> {
        debug_assert_eq!(self.object(slot).zone, Zone::Battlefield);
        debug_assert!(!self.memos(slot).iter().any(|m| m.same_variant(memo)));
        self.memos.entries.push(Remembered { slot, memo })?;
        self.memos.kinds.0 |= memo.kinds();
        self.keyed.insert(slot);
        if memo.layer_input() {
            self.flags.derived_dirty = true;
        }
        Ok(())
    }

    /// Whether the spell that became the permanent in `slot` was cast with a cost of this
    /// kind paid ([CR 702.33d](https://yawgatog.com/resources/magic-rules/#R70233d)). With
    /// no permanent in the game that remembers one of the kind, one AND.
    #[inline]
    pub fn paid(&self, slot: L::Slot, kind: CastingKind) -> bool {
        let asked = CastingKinds::NONE.with(kind);
        self.memos
            .of(Kinds::paid(asked))
            .iter()
            .any(|e| e.slot == slot && matches!(e.memo, Memo::Paid(paid) if paid.contains(kind)))
    }

    /// The value of X the permanent in `slot` was cast with
    /// ([CR 107.3m](https://yawgatog.com/resources/magic-rules/#R1073m)), 0 if none. With
    /// no permanent in the game that remembers one, one AND.
    #[inline]
    pub fn x_of(&self, slot: L::Slot) -> u8 {
        self.memos
            .of(Kinds::X)
            .iter()
            .find_map(|e| match e.memo {
                Memo::X(x) if e.slot == slot => Some(x),
                _ => None,
            })
            .unwrap_or(0)
    }

    /// Everything the object in `slot` remembers: for [`State::signature`], tools, tests
    /// and a view. The rules ask by kind. With nothing stored about the object, one bit
    /// test.
    #[inline]
    pub fn memos(&self, slot: L::Slot) -> MemosOf<'_, L> {
        MemosOf {
            entries: if self.keyed.contains(slot) {
                self.memos.entries.as_slice()
            } else {
                &[]
            },
            slot,
        }
    }
}

impl<'a, L: Layout> Observation<'a, L> {
    /// Everything the object in `slot` remembers. A memo is as public as its object: how a
    /// spell was cast is announced
    /// ([CR 601.2b](https://yawgatog.com/resources/magic-rules/#R6012b)), and the permanent
    /// it became is on the battlefield for every player to see.
    pub fn memos(&self, slot: L::Slot) -> MemosOf<'a, L> {
        self.shown().memos(slot)
    }
}

#[cfg(test)]
mod tests;
